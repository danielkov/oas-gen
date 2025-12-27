//! Service module code generation

use askama::Template;
use ir::gen_ir::{AuthKind, AuthScheme, HttpMethod, Operation, Service};

/// Escape Rust keywords with r# prefix
fn escape_keyword(name: &str) -> String {
    match name {
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
        | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
        | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
        | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await"
        | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override"
        | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "try" => format!("r#{}", name),
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
    pub fn render_type(type_id: &StableId, _: &dyn askama::Values) -> askama::Result<String> {
        match type_id {
            StableId::Primitive(p) => {
                use ir::gen_ir::Primitive;
                let rust_type = match p {
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
                };
                Ok(rust_type.to_string())
            }
            StableId::Named(name) => {
                // Use CanonicalName to get proper pascal case (handles acronyms like FAQItem -> FaqItem)
                let pascal = CanonicalName::from_string(name).pascal;
                Ok(format!("crate::types::{}", pascal))
            }
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
                return Ok(format!("StatusCode::from_u16_unchecked({})", code));
            }
        };
        Ok(result.to_string())
    }
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
        ct == "application/json"
            || ct.starts_with("application/json;")
            || ct.ends_with("+json")
    }

    fn new(operation: &'a Operation) -> Self {
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
    has_bearer_auth: bool,
    has_api_key_auth: bool,
    operations: Vec<OperationTemplate<'a>>,
    methods: Vec<&'static str>,
}

pub struct ServiceModuleGenerator<'a> {
    service: &'a Service,
    auth_schemes: &'a [AuthScheme],
    package_name: &'a str,
}

impl<'a> ServiceModuleGenerator<'a> {
    pub fn new(
        service: &'a Service,
        auth_schemes: &'a [AuthScheme],
        package_name: &'a str,
    ) -> Self {
        Self {
            service,
            auth_schemes,
            package_name,
        }
    }

    pub fn generate(&self) -> String {
        // Determine which auth wrappers to generate
        let mut has_bearer_auth = false;
        let mut has_api_key_auth = false;

        for scheme in self.auth_schemes {
            match &scheme.kind {
                AuthKind::Http { scheme: s, .. } if s == "bearer" => {
                    has_bearer_auth = true;
                }
                AuthKind::ApiKey { .. } => {
                    has_api_key_auth = true;
                }
                _ => {}
            }
        }

        // Wrap operations with preprocessed data
        let operations: Vec<OperationTemplate> = self
            .service
            .operations
            .iter()
            .map(OperationTemplate::new)
            .collect();

        // Collect unique HTTP methods used
        let mut methods: Vec<&'static str> = operations
            .iter()
            .map(|op| op.method_fn)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        methods.sort();

        let template = ServiceModuleTemplate {
            trait_name: &self.service.name.pascal,
            module_name: &self.service.name.snake,
            package_name: self.package_name,
            has_bearer_auth,
            has_api_key_auth,
            operations,
            methods,
        };

        template
            .render()
            .unwrap_or_else(|e| panic!("Failed to render service module template: {}", e))
    }
}
