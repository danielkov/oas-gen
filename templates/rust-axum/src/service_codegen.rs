//! Service module code generation

use askama::Template;
use ir::gen_ir::{ApiKeyLocation, AuthKind, AuthScheme, HttpMethod, Operation, Service, StableId};
use std::collections::{BTreeMap, BTreeSet};

/// Runtime render context. Threaded into the type-reference rendering path so
/// both the single-crate and multi-crate layouts can emit the right module
/// prefix without any post-hoc string rewriting.
#[derive(Debug, Clone)]
pub struct RenderCtx {
    /// Maps every named type in the IR to the bucket it belongs to (either a
    /// tag or the common bucket). Computed once per generation run.
    pub type_bucket: BTreeMap<StableId, crate::Bucket>,
    /// The Rust module ident of the common crate (e.g. `"fixture_api_common"`).
    /// `None` in single-crate mode.
    pub common_ident: Option<String>,
    /// Are we emitting into a multi-crate layout?
    pub multi_crate: bool,
    /// Rust ident of the crate currently being rendered. Used to qualify
    /// type references from *outside* the crate (e.g. doctests, which
    /// compile as external consumers). Defaults to the root package ident
    /// in single-crate mode.
    pub self_crate: String,
}

impl RenderCtx {
    /// Qualify a PascalCase named type reference with the right module prefix.
    /// Used from `render_type_ref` (the path that emits bare names inside the
    /// types file). In single-crate mode this is unqualified (matches the
    /// pre-existing `use super::*;` import convention). In multi-crate mode a
    /// cross-bucket ref gets the common crate prefix; same-bucket stays bare
    /// because it lives in the same file.
    pub fn qualify_named(&self, target: &StableId, name: &str, source: &StableId) -> String {
        if !self.multi_crate {
            return name.to_string();
        }
        let (Some(source_bucket), Some(target_bucket)) =
            (self.type_bucket.get(source), self.type_bucket.get(target))
        else {
            return name.to_string();
        };
        if source_bucket == target_bucket {
            return name.to_string();
        }
        // Cross-bucket: the target must be in the common bucket in this
        // codebase (tag types never reference other tags' tag-local types —
        // any such ref would have promoted the target to the common bucket).
        if matches!(target_bucket, crate::Bucket::Common)
            && let Some(ident) = &self.common_ident
        {
            return format!("{}::{}", ident, name);
        }
        name.to_string()
    }

    /// Qualify a named type for the askama filter (used from service modules,
    /// whose baseline prefix is `crate::types::<Name>`). In multi-crate mode,
    /// cross-bucket refs swap the prefix for the common crate ident.
    pub fn qualify_for_service(&self, target: &StableId, name: &str) -> String {
        if self.multi_crate
            && matches!(self.type_bucket.get(target), Some(crate::Bucket::Common))
            && let Some(ident) = &self.common_ident
        {
            return format!("{}::{}", ident, name);
        }
        format!("crate::types::{}", name)
    }

    /// Qualify a named type for a doctest, which compiles as an external
    /// consumer of the generated crate and therefore cannot use `crate::…`
    /// paths. Resolves to the common crate for shared types and to the
    /// service's own crate (`self_crate`) for per-bucket types.
    pub fn qualify_for_doc(&self, target: &StableId, name: &str) -> String {
        if self.multi_crate
            && matches!(self.type_bucket.get(target), Some(crate::Bucket::Common))
            && let Some(ident) = &self.common_ident
        {
            return format!("{}::{}", ident, name);
        }
        format!("{}::types::{}", self.self_crate, name)
    }
}

/// Lookup key used to find the generated auth extractor type name for an
/// operation. Includes whether anonymous access is allowed so that operations
/// with the same scheme set but different optionality can resolve to
/// different extractors (`Auth` vs `OptionalAuth`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuthTypeKey {
    pub schemes: BTreeSet<StableId>,
    pub allows_anonymous: bool,
}

/// Escape Rust keywords with r# prefix.
/// Note: `self` and `Self` cannot be raw identifiers, so they use underscore prefix instead.
fn escape_keyword(name: &str) -> String {
    match name {
        // self and Self cannot use r# prefix - use underscore instead
        "self" | "Self" => format!("_{}", name),
        // All other keywords can use r# prefix
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
        | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
        | "mut" | "pub" | "ref" | "return" | "static" | "struct" | "super" | "trait" | "true"
        | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await" | "dyn"
        | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override" | "priv"
        | "typeof" | "unsized" | "virtual" | "yield" | "try" => format!("r#{}", name),
        _ => name.to_string(),
    }
}

mod filters {
    use ir::gen_ir::{CanonicalName, StableId};

    pub fn escape_rust_keyword(name: &str, _: &dyn askama::Values) -> askama::Result<String> {
        Ok(super::escape_keyword(name))
    }

    /// Convert a string to PascalCase using CanonicalName
    pub fn to_pascal_case(name: &str, _: &dyn askama::Values) -> askama::Result<String> {
        Ok(CanonicalName::from_string(name).pascal)
    }

    /// Render a type reference to its Rust type
    /// Uses CanonicalName to ensure consistent pascal case naming
    pub fn render_type(type_id: &StableId, values: &dyn askama::Values) -> askama::Result<String> {
        match type_id {
            StableId::Primitive(p) => Ok(render_primitive(p).to_string()),
            StableId::Named(name) => {
                // Use CanonicalName to get proper pascal case (handles acronyms like FAQItem -> FaqItem)
                let pascal = CanonicalName::from_string(name).pascal;
                // If a RenderCtx was passed via askama Values, use bucket-aware
                // qualification. Otherwise keep the single-crate default.
                if let Ok(ctx) = askama::get_value::<super::RenderCtx>(values, "render_ctx") {
                    return Ok(ctx.qualify_for_service(type_id, &pascal));
                }
                Ok(format!("crate::types::{}", pascal))
            }
        }
    }

    /// Render a type reference for use inside a doctest, where `crate::…`
    /// paths do not resolve because rustdoc compiles the example as an
    /// external consumer. Named types are qualified with their owning crate;
    /// the self-crate ident is read from the `RenderCtx`.
    pub fn render_type_doc(
        type_id: &StableId,
        values: &dyn askama::Values,
    ) -> askama::Result<String> {
        match type_id {
            StableId::Primitive(p) => Ok(render_primitive(p).to_string()),
            StableId::Named(name) => {
                let pascal = CanonicalName::from_string(name).pascal;
                let ctx =
                    askama::get_value::<super::RenderCtx>(values, "render_ctx").map_err(|_| {
                        askama::Error::Custom(
                            "render_type_doc requires a RenderCtx askama value".into(),
                        )
                    })?;
                Ok(ctx.qualify_for_doc(type_id, &pascal))
            }
        }
    }

    fn render_primitive(p: &ir::gen_ir::Primitive) -> &'static str {
        use ir::gen_ir::Primitive;
        match p {
            Primitive::String => "String",
            Primitive::Bool => "bool",
            Primitive::I32 => "i32",
            Primitive::I64 => "i64",
            Primitive::F32 => "f32",
            Primitive::F64 => "f64",
            Primitive::Date => "jiff::civil::Date",
            Primitive::DateTime => "jiff::Timestamp",
            Primitive::Uuid => "uuid::Uuid",
            Primitive::Bytes => "bytes::Bytes",
            Primitive::Decimal => "rust_decimal::Decimal",
            Primitive::Any => "serde_json::Value",
        }
    }

    /// Convert HTTP status code to StatusCode expression (constant or unsafe constructor)
    pub fn status_code_const(code: &u16, _: &dyn askama::Values) -> askama::Result<String> {
        let result = match *code {
            100 => "StatusCode::CONTINUE",
            101 => "StatusCode::SWITCHING_PROTOCOLS",
            102 => "StatusCode::PROCESSING",
            200 => "StatusCode::OK",
            201 => "StatusCode::CREATED",
            202 => "StatusCode::ACCEPTED",
            203 => "StatusCode::NON_AUTHORITATIVE_INFORMATION",
            204 => "StatusCode::NO_CONTENT",
            205 => "StatusCode::RESET_CONTENT",
            206 => "StatusCode::PARTIAL_CONTENT",
            207 => "StatusCode::MULTI_STATUS",
            208 => "StatusCode::ALREADY_REPORTED",
            226 => "StatusCode::IM_USED",
            300 => "StatusCode::MULTIPLE_CHOICES",
            301 => "StatusCode::MOVED_PERMANENTLY",
            302 => "StatusCode::FOUND",
            303 => "StatusCode::SEE_OTHER",
            304 => "StatusCode::NOT_MODIFIED",
            305 => "StatusCode::USE_PROXY",
            307 => "StatusCode::TEMPORARY_REDIRECT",
            308 => "StatusCode::PERMANENT_REDIRECT",
            400 => "StatusCode::BAD_REQUEST",
            401 => "StatusCode::UNAUTHORIZED",
            402 => "StatusCode::PAYMENT_REQUIRED",
            403 => "StatusCode::FORBIDDEN",
            404 => "StatusCode::NOT_FOUND",
            405 => "StatusCode::METHOD_NOT_ALLOWED",
            406 => "StatusCode::NOT_ACCEPTABLE",
            407 => "StatusCode::PROXY_AUTHENTICATION_REQUIRED",
            408 => "StatusCode::REQUEST_TIMEOUT",
            409 => "StatusCode::CONFLICT",
            410 => "StatusCode::GONE",
            411 => "StatusCode::LENGTH_REQUIRED",
            412 => "StatusCode::PRECONDITION_FAILED",
            413 => "StatusCode::PAYLOAD_TOO_LARGE",
            414 => "StatusCode::URI_TOO_LONG",
            415 => "StatusCode::UNSUPPORTED_MEDIA_TYPE",
            416 => "StatusCode::RANGE_NOT_SATISFIABLE",
            417 => "StatusCode::EXPECTATION_FAILED",
            418 => "StatusCode::IM_A_TEAPOT",
            421 => "StatusCode::MISDIRECTED_REQUEST",
            422 => "StatusCode::UNPROCESSABLE_ENTITY",
            423 => "StatusCode::LOCKED",
            424 => "StatusCode::FAILED_DEPENDENCY",
            426 => "StatusCode::UPGRADE_REQUIRED",
            428 => "StatusCode::PRECONDITION_REQUIRED",
            429 => "StatusCode::TOO_MANY_REQUESTS",
            431 => "StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE",
            451 => "StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS",
            500 => "StatusCode::INTERNAL_SERVER_ERROR",
            501 => "StatusCode::NOT_IMPLEMENTED",
            502 => "StatusCode::BAD_GATEWAY",
            503 => "StatusCode::SERVICE_UNAVAILABLE",
            504 => "StatusCode::GATEWAY_TIMEOUT",
            505 => "StatusCode::HTTP_VERSION_NOT_SUPPORTED",
            506 => "StatusCode::VARIANT_ALSO_NEGOTIATES",
            507 => "StatusCode::INSUFFICIENT_STORAGE",
            508 => "StatusCode::LOOP_DETECTED",
            510 => "StatusCode::NOT_EXTENDED",
            511 => "StatusCode::NETWORK_AUTHENTICATION_REQUIRED",
            0 => {
                return Err(askama::Error::Custom(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Status code cannot be 0 (does not fit in NonZeroU16)".to_string(),
                ))));
            }
            code => {
                // Use try_from with compile-time const assertion for safety
                // StatusCode is valid for 100-999, OpenAPI specs should always have valid codes
                return Ok(format!(
                    "{{ StatusCode::from_u16({}).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR) }}",
                    code
                ));
            }
        };
        Ok(result.to_string())
    }
}

/// Resolved authentication requirement for a single operation
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub kind: ResolvedAuthKind,
}

#[derive(Debug, Clone)]
pub enum ResolvedAuthKind {
    Bearer,
    ApiKeyHeader { header_name: String },
    ApiKeyQuery { param_name: String },
    ApiKeyCookie { cookie_name: String },
}

/// Resolve auth requirements for an operation against the global auth schemes.
pub fn resolve_operation_auth(
    operation: &Operation,
    auth_schemes: &[AuthScheme],
) -> Vec<ResolvedAuth> {
    operation
        .auth
        .iter()
        .filter_map(|auth_use| {
            auth_schemes
                .iter()
                .find(|s| s.id == auth_use.scheme)
                .map(|scheme| match &scheme.kind {
                    AuthKind::Http { scheme: s, .. } if s == "bearer" => ResolvedAuth {
                        kind: ResolvedAuthKind::Bearer,
                    },
                    AuthKind::ApiKey {
                        location,
                        param_name,
                    } => match location {
                        ApiKeyLocation::Header => ResolvedAuth {
                            kind: ResolvedAuthKind::ApiKeyHeader {
                                header_name: param_name.clone(),
                            },
                        },
                        ApiKeyLocation::Query => ResolvedAuth {
                            kind: ResolvedAuthKind::ApiKeyQuery {
                                param_name: param_name.clone(),
                            },
                        },
                        ApiKeyLocation::Cookie => ResolvedAuth {
                            kind: ResolvedAuthKind::ApiKeyCookie {
                                cookie_name: param_name.clone(),
                            },
                        },
                    },
                    _ => ResolvedAuth {
                        kind: ResolvedAuthKind::Bearer,
                    },
                })
        })
        .collect()
}

/// Wrapper for operations with preprocessed data for templates
#[derive(Debug, Clone)]
struct OperationTemplate<'a> {
    operation: &'a Operation,
    method_fn: &'static str,
    request_content_type: RequestContentType,
    response_content_type: ResponseContentType,
    /// For binary responses with multiple content types, this contains all supported types
    binary_content_types: Vec<String>,
    /// Auth type name for this operation (None if no auth required)
    auth_type_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestContentType {
    Json,
    Multipart,
    OctetStream,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseContentType {
    Json,
    /// Single binary content type (e.g., "application/octet-stream")
    Binary,
    /// Multiple binary content types - caller must specify which to return
    MultipleBinary,
    None,
}

impl<'a> OperationTemplate<'a> {
    /// Check if a content type is a JSON media type
    /// Only application/json and application/*+json are considered JSON
    /// text/* types like text/csv are NOT JSON
    fn is_json_content_type(ct: &str) -> bool {
        ct == "application/json" || ct.starts_with("application/json;") || ct.ends_with("+json")
    }

    fn new(operation: &'a Operation, auth_type_name: Option<String>) -> Self {
        let method_fn = match operation.http.method {
            HttpMethod::Get => "get",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Delete => "delete",
            HttpMethod::Patch => "patch",
            HttpMethod::Head => "head",
            HttpMethod::Options => "options",
            HttpMethod::Trace => "trace",
        };

        // Detect request content type
        let request_content_type = if let Some(body) = &operation.http.body {
            if let Some(variant) = body.variants.first() {
                if variant.content_type.starts_with("multipart/") {
                    RequestContentType::Multipart
                } else if variant.content_type == "application/octet-stream" {
                    RequestContentType::OctetStream
                } else {
                    RequestContentType::Json
                }
            } else {
                RequestContentType::None
            }
        } else {
            RequestContentType::None
        };

        // Detect response content type and collect binary content types
        let mut binary_content_types = Vec::new();

        // First check if we have multiple produces (from http.produces)
        // Collect all non-JSON content types for binary/text handling
        for ct in &operation.http.produces {
            if !Self::is_json_content_type(ct) {
                binary_content_types.push(ct.clone());
            }
        }

        let response_content_type = if let Some(success) = &operation.success {
            if let Some(ct) = &success.content_type {
                if Self::is_json_content_type(ct) {
                    ResponseContentType::Json
                } else if binary_content_types.len() > 1 {
                    ResponseContentType::MultipleBinary
                } else {
                    // Single binary type - add to list if not already there
                    if binary_content_types.is_empty() {
                        binary_content_types.push(ct.clone());
                    }
                    ResponseContentType::Binary
                }
            } else {
                ResponseContentType::None
            }
        } else {
            ResponseContentType::None
        };

        Self {
            operation,
            method_fn,
            request_content_type,
            response_content_type,
            binary_content_types,
            auth_type_name,
        }
    }
}

/// Template data for service module generation
#[derive(Template)]
#[template(path = "service_module.rs.jinja", escape = "none")]
struct ServiceModuleTemplate<'a> {
    trait_name: &'a str,
    module_name: &'a str,
    package_name: &'a str,
    auth_imports: Vec<String>,
    operations: Vec<OperationTemplate<'a>>,
    methods: Vec<&'static str>,
    /// Module path prefix for shared items (`RequestContext`, auth extractors).
    /// Single-crate: `crate::shared`. Multi-crate: common crate ident.
    shared_prefix: String,
    /// Module path prefix for multipart utilities.
    /// Single-crate: `crate::multipart`. Multi-crate: `<common_ident>::multipart`.
    multipart_prefix: String,
    /// Path used in doc examples to reach shared items. Single-crate:
    /// `<package_name>::shared`. Multi-crate: common crate ident.
    shared_doc_prefix: String,
}

pub struct ServiceModuleGenerator<'a> {
    service: &'a Service,
    package_name: &'a str,
    auth_type_map: &'a BTreeMap<AuthTypeKey, String>,
    render_ctx: &'a RenderCtx,
}

impl<'a> ServiceModuleGenerator<'a> {
    pub fn new(
        service: &'a Service,
        package_name: &'a str,
        auth_type_map: &'a BTreeMap<AuthTypeKey, String>,
        render_ctx: &'a RenderCtx,
    ) -> Self {
        Self {
            service,
            package_name,
            auth_type_map,
            render_ctx,
        }
    }

    pub fn generate(&self) -> String {
        let operations: Vec<OperationTemplate> = self
            .service
            .operations
            .iter()
            .map(|op| {
                let auth_type_name = if op.auth.is_empty() {
                    None
                } else {
                    let key = AuthTypeKey {
                        schemes: op.auth.iter().map(|a| a.scheme.clone()).collect(),
                        allows_anonymous: op.auth.iter().any(|a| a.optional),
                    };
                    self.auth_type_map
                        .get(&key)
                        .cloned()
                        .filter(|n| !n.is_empty())
                };
                OperationTemplate::new(op, auth_type_name)
            })
            .collect();

        // Collect unique auth type imports needed by this module
        let auth_imports: Vec<String> = {
            let mut set = BTreeSet::new();
            for op in &operations {
                if let Some(name) = &op.auth_type_name {
                    set.insert(name.clone());
                }
            }
            set.into_iter().collect()
        };

        // Collect unique HTTP methods used
        let mut methods: Vec<&'static str> = operations
            .iter()
            .map(|op| op.method_fn)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        methods.sort();

        let (shared_prefix, multipart_prefix, shared_doc_prefix) =
            match (self.render_ctx.multi_crate, &self.render_ctx.common_ident) {
                (true, Some(ident)) => (
                    ident.clone(),
                    format!("{}::multipart", ident),
                    ident.clone(),
                ),
                _ => (
                    "crate::shared".to_string(),
                    "crate::multipart".to_string(),
                    format!("{}::shared", self.package_name),
                ),
            };

        let template = ServiceModuleTemplate {
            trait_name: &self.service.name.pascal,
            module_name: &self.service.name.snake,
            package_name: self.package_name,
            auth_imports,
            operations,
            methods,
            shared_prefix,
            multipart_prefix,
            shared_doc_prefix,
        };

        use askama::Template;
        let values: &dyn askama::Values = self.render_ctx;
        template
            .render_with_values(values)
            .unwrap_or_else(|e| panic!("Failed to render service module template: {}", e))
    }
}

impl askama::Values for RenderCtx {
    fn get_value<'a>(&'a self, key: &str) -> Option<&'a dyn std::any::Any> {
        if key == "render_ctx" {
            Some(self as &dyn std::any::Any)
        } else {
            None
        }
    }
}
