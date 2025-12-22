//! Codegen-oriented, language-agnostic IR produced from OpenAPI v3.x.
//! This IR is resolved, stable, and designed for templating.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub type Map<K, V> = BTreeMap<K, V>;

/// A globally stable identifier for types/operations/services usable as filename, symbol, etc.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(untagged)]
pub enum StableId {
    /// A primitive type (built-in)
    Primitive(Primitive),
    /// A named type (user-defined from schema)
    Named(String),
}

impl std::fmt::Display for StableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StableId::Primitive(p) => write!(f, "Primitive_{:?}", p),
            StableId::Named(s) => write!(f, "{}", s),
        }
    }
}

/// Precomputed, language-agnostic names to avoid case-munging in templates.
/// Generators pick the right field for their target.
#[derive(Debug, Clone, Serialize)]
pub struct CanonicalName {
    pub canonical: String, // "ListPets"
    pub snake: String,     // "list_pets"
    pub camel: String,     // "listPets"
    pub pascal: String,    // "ListPets"
    pub kebab: String,     // "list-pets"
    pub upper: String,     // "LIST_PETS"
    pub lower: String,     // "listpets" (rarely used)
}

/// Documentation payload ready to be dropped into comment blocks.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Docs {
    pub summary: Option<String>,
    pub description: Option<String>, // already combined & cleaned (markdown allowed)
    pub deprecated: bool,
    pub since: Option<String>,     // e.g., version tag if derivable
    pub examples: Vec<DocExample>, // pre-renderable code/doc examples
    pub external_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocExample {
    /// Human title shown above the example.
    pub title: Option<String>,
    /// JSON or textual representation to embed in comments or samples.
    pub value: String,
}

/// #### Top-level IR ##########################################################

#[derive(Debug, Clone, Serialize)]
pub struct GenIr {
    pub api: ApiMeta,
    pub types: Map<StableId, TypeDecl>, // all declared, deduped, named types
    pub services: Vec<Service>,         // SDK/Server “modules”
    pub auth_schemes: Vec<AuthScheme>,  // normalized from securitySchemes
    pub errors: Vec<ErrorDecl>,         // shared error union(s) if you build them
    pub server_sets: Vec<ServerSet>,    // base URLs and variables
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiMeta {
    pub title: String,
    pub version: String,
    pub package_name: CanonicalName, // root package/module name for codegen
    pub docs: Docs,
}

/// Server groups (global and/or per-tag overrides), already expanded for templating.
#[derive(Debug, Clone, Serialize)]
pub struct ServerSet {
    pub name: CanonicalName,
    pub urls: Vec<ServerUrl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerUrl {
    pub template: String,         // "https://{region}.api.example.com/v1"
    pub resolved_preview: String, // best-effort concrete URL for samples
    pub variables: Map<String, ServerVar>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerVar {
    pub name: CanonicalName,
    pub default: String,
    pub allowed: Vec<String>,
    pub docs: Option<String>,
}

/// #### Types #################################################################
/// Reference to a declared type with view modifiers (nullability, containers).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TypeRef {
    pub target: StableId,        // points into GenIr.types
    pub optional: bool,          // may be absent (e.g., not in required-set)
    pub nullable: bool,          // may be null on the wire
    pub by_ref: bool,            // hint for languages that distinguish references (Rust & others)
    pub modifiers: Vec<TypeMod>, // list<>, map<>, set<>, non-empty, etc.
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum TypeMod {
    List,
    Set,
    Map(Box<TypeRef>), // value type
    NonEmpty,
    Bounded { min: Option<u64>, max: Option<u64> },
}

/// Declared, named shapes: structs, enums, unions, aliases.
#[derive(Debug, Clone, Serialize)]
pub struct TypeDecl {
    pub id: StableId,
    pub name: CanonicalName, // type name for templates
    pub docs: Docs,
    pub kind: TypeKind,
    /// Optional source hints (original schema pointer) for traceability.
    pub origin: Option<String>,
    /// Tags associated with this type (from OpenAPI operation tags).
    /// Used for feature-flag based code organization.
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum TypeKind {
    // Object/record with fields and an “additional properties” policy:
    Struct {
        fields: Vec<Field>,
        additional: Additional,
        discriminator: Option<Discriminator>, // for polymorphic hierarchies
    },
    // Closed set of alternatives. Template to sum/union/ADT.
    Union {
        /// If discriminated, every variant has a tag value.
        style: UnionStyle,
        variants: Vec<Variant>,
    },
    // Closed set of literal values (string/number/bool/nullable supported).
    Enum {
        base: Primitive,
        values: Vec<EnumValue>,
    },
    // Type alias to a primitive or composite (after inlining simple schemas).
    Alias {
        aliased: AliasTarget,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum Additional {
    Forbidden, // additionalProperties: false
    Any,       // additionalProperties: true or absent
    Typed(Box<TypeRef>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Discriminator {
    pub property: String,
    /// tag literal -> target variant type (usually also appears in Union.variants)
    pub mapping: Map<String, StableId>,
}

#[derive(Debug, Clone, Serialize)]
pub enum UnionStyle {
    Discriminated { tag: String }, // explicit discriminator property
    OneOf,                         // structure-only; choose best strategy per language
    AnyOf,                         // permissive; often map to “any”/type assertions
    AllOf,                         // composition; often flattens into struct
}

#[derive(Debug, Clone, Serialize)]
pub struct Variant {
    pub name: CanonicalName,
    pub docs: Docs,
    pub ty: TypeRef,               // usually a Struct or Alias
    pub tag_value: Option<String>, // when discriminated
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumValue {
    pub name: CanonicalName, // precomputed symbol (e.g., "PetTypeDog")
    pub docs: Docs,
    pub wire: Literal, // exact on-the-wire value
}

#[derive(Debug, Clone, Serialize)]
pub enum AliasTarget {
    Primitive(Primitive),
    Composite(Composite),
    Reference(TypeRef), // alias to another named type
}

/// First-class primitives commonly used by generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Primitive {
    Any,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    Date,
    DateTime,
    Uuid,
    Bytes,   // base64/binary
    Decimal, // high-precision numeric
}

/// “Inline” composite types for aliases: list/map/tuple.
#[derive(Debug, Clone, Serialize)]
pub enum Composite {
    List(Box<TypeRef>),
    Map { key: Primitive, value: Box<TypeRef> }, // keys are strings in JSON, but allow extensibility
    Tuple(Vec<TypeRef>),
}

/// Strongly-typed literal for defaults/enums/examples.
#[derive(Debug, Clone, Serialize)]
pub enum Literal {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Array(Vec<Literal>),
    Object(Map<String, Literal>),
}

/// Fields carry effective optionality & nullability already resolved.
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: CanonicalName,
    pub docs: Docs,
    pub ty: TypeRef,
    pub default: Option<Literal>,
    pub deprecated: bool,
    /// If present, this field has a constant value that must always be this literal.
    pub const_value: Option<Literal>,
    /// Hints for serde/jackson/etc. (e.g., wire name differs; explode styles are gone at this layer)
    pub wire_name: String,
}

/// #### Services & Operations #################################################

#[derive(Debug, Clone, Serialize)]
pub struct Service {
    pub id: StableId,
    pub name: CanonicalName, // e.g., "Pets"
    pub docs: Docs,
    pub server_set: Option<StableId>, // reference to ServerSet if overridden
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Delete,
    Patch,
    Head,
    Options,
    Trace,
}

#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub id: StableId,
    pub name: CanonicalName, // "ListPets"
    pub docs: Docs,
    pub deprecated: bool,

    // HTTP request shape (all parameters normalized):
    pub http: HttpShape,

    // Primary success result (already chosen & normalized), plus alternative successes if any:
    pub success: Option<Payload>,  // e.g., 200 application/json PetList
    pub alt_success: Vec<Payload>, // rare multi-variant success

    // Error union for this op (status grouping + content types):
    pub errors: ErrorUse, // points to shared ErrorDecl or inline

    // Auth & paging hints:
    pub auth: Vec<AuthUse>,             // concrete requirements resolved
    pub pagination: Option<PagingHint>, // cursor/offset etc., if detected

    // Idempotency & retry info:
    pub idempotent: bool,
    pub retryable_statuses: BTreeSet<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpShape {
    pub method: HttpMethod,
    pub path_template: String, // "/pets/{id}"
    pub segments: Vec<PathSeg>,
    pub query: Vec<QueryParam>, // fully normalized (style/explode resolved)
    pub headers: Vec<HeaderParam>,
    pub cookies: Vec<CookieParam>,
    pub path_params: Vec<PathParam>,
    pub body: Option<Body>,    // content-type keyed payload
    pub consumes: Vec<String>, // e.g., ["application/json"]
    pub produces: Vec<String>, // e.g., ["application/json","text/csv"]
}

#[derive(Debug, Clone, Serialize)]
pub enum PathSeg {
    Static(String),
    Param { name: CanonicalName, wire: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryParam {
    pub name: CanonicalName,
    pub wire: String,
    pub docs: Docs,
    pub ty: TypeRef,
    pub required: bool,
    pub default: Option<Literal>,
}
#[derive(Debug, Clone, Serialize)]
pub struct HeaderParam {
    pub name: CanonicalName,
    pub wire: String,
    pub docs: Docs,
    pub ty: TypeRef,
    pub required: bool,
    pub default: Option<Literal>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CookieParam {
    pub name: CanonicalName,
    pub wire: String,
    pub docs: Docs,
    pub ty: TypeRef,
    pub required: bool,
    pub default: Option<Literal>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PathParam {
    pub name: CanonicalName,
    pub wire: String,
    pub docs: Docs,
    pub ty: TypeRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct Body {
    pub variants: Vec<BodyVariant>, // multiple content-types possible
    /// Choose a preferred content type (e.g., JSON) for samples.
    pub preferred: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BodyVariant {
    pub content_type: String, // "application/json", "multipart/form-data", etc.
    pub ty: TypeRef,          // effective input type (Io::Input applied)
    pub docs: Docs,
    pub encoding: Vec<PartEncoding>, // for multipart: per-part encoding headers/content-type
}

#[derive(Debug, Clone, Serialize)]
pub struct PartEncoding {
    pub field: String,
    pub content_type: Option<String>,
    pub headers: Vec<HeaderParam>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Payload {
    pub status: StatusSpec,           // exact 200 or a named “2xx-success”
    pub content_type: Option<String>, // None for empty body
    pub ty: Option<TypeRef>,          // None for empty/Unit
    pub headers: Vec<HeaderParam>,
    pub docs: Docs,
}

#[derive(Debug, Clone, Serialize)]
pub enum StatusSpec {
    Code(u16),
    Range(String), // "2XX"
    Default,
}

/// #### Errors ################################################################

#[derive(Debug, Clone, Serialize)]
pub enum ErrorUse {
    Inline(Box<ErrorDecl>), // small API: inline per-op
    // NOTE: this isn't currently used - maybe in future, auto-merge common error schemas, or remove this
    Shared(StableId), // reference into GenIr.errors
    None,             // no errors (rare)
}

/// Error declaration can model status → tagged payloads → render to exception/union.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDecl {
    pub id: StableId,
    pub name: CanonicalName, // e.g., "PetsErrors"
    pub docs: Docs,
    pub variants: Vec<ErrorVariant>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorVariant {
    pub name: CanonicalName,          // e.g., "NotFound"
    pub status: StatusSpec,           // 404
    pub content_type: Option<String>, // application/json, text/plain, …
    pub ty: Option<TypeRef>,          // decoded payload shape
    pub docs: Docs,
}

/// #### Auth ##################################################################

#[derive(Debug, Clone, Serialize)]
pub struct AuthScheme {
    pub id: StableId,
    pub name: CanonicalName, // "BearerAuth"
    pub kind: AuthKind,
    pub docs: Docs,
}

#[derive(Debug, Clone, Serialize)]
pub enum AuthKind {
    ApiKey {
        location: ApiKeyLocation,
        param_name: String,
    },
    Http {
        scheme: String,
        bearer_format: Option<String>,
    }, // basic/bearer/digest
    OAuth2 {
        flows: Vec<OAuthFlow>,
    },
    OpenIdConnect {
        url: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ApiKeyLocation {
    Query,
    Header,
    Cookie,
}

#[derive(Debug, Clone, Serialize)]
pub struct OAuthFlow {
    pub kind: OAuthFlowKind,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub refresh_url: Option<String>,
    pub scopes: Vec<OAuthScope>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OAuthFlowKind {
    Implicit,
    Password,
    ClientCredentials,
    AuthorizationCode,
}
#[derive(Debug, Clone, Serialize)]
pub struct OAuthScope {
    pub name: String,
    pub description: Option<String>,
}

/// Concrete requirement bound to an operation after security resolution.
#[derive(Debug, Clone, Serialize)]
pub struct AuthUse {
    pub scheme: StableId,    // reference to AuthScheme
    pub scopes: Vec<String>, // for OAuth2
    pub optional: bool,      // if anyOf/OR semantics allow anonymous use
}

/// #### Pagination ############################################################

#[derive(Debug, Clone, Serialize)]
pub enum PagingKind {
    Cursor {
        param: String,
        item_path: ResponsePath,
        next_path: ResponsePath,
    },
    Offset {
        limit_param: String,
        offset_param: String,
        item_path: ResponsePath,
    },
    TokenInHeader {
        header: String,
        item_path: ResponsePath,
    },
    None,
}

/// A JSONPath-like pointer into a response body for extracting items/next cursors.
#[derive(Debug, Clone, Serialize)]
pub struct ResponsePath(pub Vec<PathElem>);
#[derive(Debug, Clone, Serialize)]
pub enum PathElem {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, Serialize)]
pub struct PagingHint {
    pub kind: PagingKind,
    pub default_page_size: Option<u32>,
    pub max_page_size: Option<u32>,
    pub docs: Docs,
}

/// ############### Helper: Building two “views” from one type #################
/// For convenience, keep track of per-IO (input/output) mirrors when needed.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IoView {
    Input,
    Output,
}

/// Optional cross-reference for templates wanting both views (e.g., request/response DTOs).
#[derive(Debug, Clone, Serialize)]
pub struct DualView {
    pub input: Option<StableId>,
    pub output: Option<StableId>,
}

// ############### Helper Functions ###############################################

impl StableId {
    /// Create a named type identifier
    pub fn new(s: impl Into<String>) -> Self {
        Self::Named(s.into())
    }

    /// Create a primitive type identifier
    pub fn primitive(p: Primitive) -> Self {
        Self::Primitive(p)
    }
}

impl CanonicalName {
    /// Create a CanonicalName from a raw string by applying case conversions.
    pub fn from_string(s: impl AsRef<str>) -> Self {
        let s = s.as_ref();
        let canonical = s.to_string();

        // Convert to snake_case as base
        let snake = to_snake_case(&canonical);
        let pascal = to_pascal_case(&canonical);
        let camel = to_camel_case(&canonical);
        let kebab = to_kebab_case(&canonical);
        let upper = snake.to_uppercase();
        let lower = canonical
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();

        Self {
            canonical,
            snake,
            camel,
            pascal,
            kebab,
            upper,
            lower,
        }
    }
}

/// Convert string to snake_case
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_is_upper = false;
    let mut prev_is_separator = false;

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 && !prev_is_upper && !prev_is_separator {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
            prev_is_upper = true;
            prev_is_separator = false;
        } else if ch.is_alphanumeric() {
            result.push(ch);
            prev_is_upper = false;
            prev_is_separator = false;
        } else {
            if !result.is_empty() && !prev_is_separator {
                result.push('_');
                prev_is_separator = true;
            }
            prev_is_upper = false;
        }
    }

    result
}

/// Convert string to PascalCase
fn to_pascal_case(s: &str) -> String {
    // First convert to snake_case to normalize word boundaries
    let snake = to_snake_case(s);
    snake
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(|c| c.to_lowercase()))
                    .collect(),
            }
        })
        .collect()
}

/// Convert string to camelCase
fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    if pascal.is_empty() {
        return pascal;
    }
    let mut chars = pascal.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

/// Convert string to kebab-case
fn to_kebab_case(s: &str) -> String {
    to_snake_case(s).replace('_', "-")
}

impl From<oas3::spec::Info> for ApiMeta {
    fn from(info: oas3::spec::Info) -> Self {
        let title = info.title.clone();
        let package_name = CanonicalName::from_string(&title);

        let docs = Docs {
            summary: None,
            description: info.description.clone(),
            deprecated: false,
            since: None,
            examples: Vec::new(),
            external_urls: Vec::new(),
        };

        Self {
            title,
            version: info.version,
            package_name,
            docs,
        }
    }
}
