use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};

pub mod gen_ir;

use gen_ir::*;

/// Builder context for tracking state during conversion
#[allow(dead_code)]
struct BuildContext {
    types: BTreeMap<StableId, TypeDecl>,
    schema_cache: HashMap<String, StableId>, // JSON pointer -> StableId
    type_counter: usize,
}

impl BuildContext {
    fn new() -> Self {
        Self {
            types: BTreeMap::new(),
            schema_cache: HashMap::new(),
            type_counter: 0,
        }
    }

    #[allow(dead_code)]
    fn next_type_id(&mut self, base: &str) -> StableId {
        self.type_counter += 1;
        StableId::new(format!("{}_{}", base, self.type_counter))
    }

    fn add_type(&mut self, decl: TypeDecl) {
        self.types.insert(decl.id.clone(), decl);
    }
}

// build an AST from an OpenAPI 3.0 document
impl From<oas::OpenAPIV3> for gen_ir::GenIr {
    fn from(document: oas::OpenAPIV3) -> Self {
        let mut ctx = BuildContext::new();

        // Convert API metadata
        let api = ApiMeta::from(document.info);

        // Convert schemas to types
        if let Some(components) = &document.components {
            convert_schemas(&mut ctx, components);
        }

        // Convert servers to ServerSets
        let server_sets = convert_servers(&document.servers);

        // Convert security schemes to AuthSchemes
        let auth_schemes = if let Some(components) = &document.components {
            convert_security_schemes(components)
        } else {
            Vec::new()
        };

        // Convert paths to Services and Operations
        let services = convert_paths(
            &mut ctx,
            &document.paths,
            &document.security,
            document.components.as_ref(),
        );

        gen_ir::GenIr {
            api,
            types: ctx.types,
            services,
            auth_schemes,
            errors: Vec::new(), // Will be populated from operations
            server_sets,
        }
    }
}

/// Convert OpenAPI components/schemas to TypeDecl
fn convert_schemas(ctx: &mut BuildContext, components: &oas::Components) {
    if let Some(schemas) = &components.schemas {
        for (name, schema_ref) in schemas {
            let schema = resolve_schema_ref(schema_ref, components);
            if let Some(schema) = schema {
                let type_decl = convert_schema_to_type(ctx, name, &schema, components);
                if let Some(decl) = type_decl {
                    ctx.add_type(decl);
                }
            }
        }
    }
}

/// Resolve a schema reference or return inline schema
fn resolve_schema_ref<'a>(
    schema_ref: &'a oas::Referenceable<oas::Schema>,
    _components: &'a oas::Components,
) -> Option<&'a oas::Schema> {
    // In the oas crate, we need to extract the data from Referenceable
    match schema_ref {
        oas::Referenceable::Data(schema) => Some(schema),
        oas::Referenceable::Reference(_) => None, // TODO: resolve references
    }
}

/// Convert a single schema to TypeDecl
fn convert_schema_to_type(
    ctx: &mut BuildContext,
    name: &str,
    schema: &oas::Schema,
    components: &oas::Components,
) -> Option<TypeDecl> {
    let id = StableId::new(name);
    let canonical_name = CanonicalName::from_string(name);

    let docs = Docs {
        summary: None,
        description: schema.description.clone(),
        deprecated: false,
        since: None,
        examples: Vec::new(),
        external_urls: Vec::new(),
    };

    // Determine the type kind based on schema properties
    let kind = infer_type_kind(ctx, schema, components);

    Some(TypeDecl {
        id,
        name: canonical_name,
        docs,
        kind,
        origin: None,
    })
}

/// Infer the TypeKind from a schema
fn infer_type_kind(
    ctx: &mut BuildContext,
    schema: &oas::Schema,
    components: &oas::Components,
) -> TypeKind {
    // Check for enum values in extras
    if let Some(JsonValue::Array(enum_values)) = schema.extras.get("enum") {
        let base = infer_primitive_from_schema(schema);
        let values = convert_enum_values(enum_values, base);
        return TypeKind::Enum { base, values };
    }

    // Check schema type
    match schema._type.as_deref() {
        Some("object") | None => {
            // Object/Struct type
            let fields = convert_properties(ctx, schema, components);
            let additional =
                if let Some(JsonValue::Bool(false)) = schema.extras.get("additionalProperties") {
                    Additional::Forbidden
                } else {
                    Additional::Any
                };

            TypeKind::Struct {
                fields,
                additional,
                discriminator: None, // TODO: handle discriminator
            }
        }
        Some("string") => {
            // String primitive as alias
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(infer_primitive_from_schema(schema)),
            }
        }
        Some("integer") | Some("number") => TypeKind::Alias {
            aliased: AliasTarget::Primitive(infer_primitive_from_schema(schema)),
        },
        Some("boolean") => TypeKind::Alias {
            aliased: AliasTarget::Primitive(Primitive::Bool),
        },
        Some("array") => {
            // Array type - create as alias to list
            if let Some(items_val) = schema.extras.get("items") {
                // Try to deserialize items as a Referenceable<Schema>
                if let Ok(items_ref) =
                    serde_json::from_value::<oas::Referenceable<oas::Schema>>(items_val.clone())
                {
                    let item_type_ref = convert_schema_to_type_ref(ctx, &items_ref, components);
                    return TypeKind::Alias {
                        aliased: AliasTarget::Composite(Composite::List(Box::new(item_type_ref))),
                    };
                }
            }
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Any),
            }
        }
        _ => {
            // Default to struct
            TypeKind::Struct {
                fields: Vec::new(),
                additional: Additional::Forbidden,
                discriminator: None,
            }
        }
    }
}

/// Convert schema properties to fields
fn convert_properties(
    ctx: &mut BuildContext,
    schema: &oas::Schema,
    components: &oas::Components,
) -> Vec<Field> {
    // Get properties from extras
    let properties_val = match schema.extras.get("properties") {
        Some(val) => val,
        None => return Vec::new(),
    };

    // Deserialize properties as a map of Referenceable<Schema>
    let properties: BTreeMap<String, oas::Referenceable<oas::Schema>> =
        match serde_json::from_value(properties_val.clone()) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

    // Get required fields from extras
    let required_fields: std::collections::HashSet<String> = schema
        .extras
        .get("required")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    properties
        .iter()
        .map(|(prop_name, prop_schema_ref)| {
            let prop_schema = resolve_schema_ref(prop_schema_ref, components)
                .unwrap_or_else(|| panic!("Failed to resolve property schema for {}", prop_name));

            let ty = convert_schema_to_type_ref(ctx, prop_schema_ref, components);
            let is_required = required_fields.contains(prop_name);
            let is_nullable = prop_schema.nullable.unwrap_or(false);

            Field {
                name: CanonicalName::from_string(prop_name),
                docs: Docs {
                    summary: prop_schema.description.clone(),
                    description: None,
                    deprecated: false,
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
                ty: TypeRef {
                    target: ty.target,
                    optional: !is_required,
                    nullable: is_nullable,
                    by_ref: false,
                    modifiers: ty.modifiers,
                },
                default: None, // TODO: parse default values
                deprecated: false,
                wire_name: prop_name.clone(),
            }
        })
        .collect()
}

/// Convert enum values
fn convert_enum_values(enum_values: &[JsonValue], _base: Primitive) -> Vec<EnumValue> {
    enum_values
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            let (name, literal) = match value {
                JsonValue::String(s) => {
                    let name = s.clone();
                    (name, Literal::String(s.clone()))
                }
                JsonValue::Number(n) => {
                    let name = format!("Value{}", idx);
                    let literal = if let Some(i) = n.as_i64() {
                        Literal::I64(i)
                    } else if let Some(f) = n.as_f64() {
                        Literal::F64(f)
                    } else {
                        Literal::String(n.to_string())
                    };
                    (name, literal)
                }
                JsonValue::Bool(b) => {
                    let name = if *b { "True" } else { "False" };
                    (name.to_string(), Literal::Bool(*b))
                }
                JsonValue::Null => ("Null".to_string(), Literal::Null),
                _ => (format!("Value{}", idx), Literal::String(value.to_string())),
            };

            EnumValue {
                name: CanonicalName::from_string(&name),
                docs: Docs::default(),
                wire: literal,
            }
        })
        .collect()
}

/// Convert a schema to a TypeRef
fn convert_schema_to_type_ref(
    ctx: &mut BuildContext,
    schema_ref: &oas::Referenceable<oas::Schema>,
    components: &oas::Components,
) -> TypeRef {
    match schema_ref {
        oas::Referenceable::Reference(reference) => {
            // Extract type name from reference like "#/components/schemas/Pet"
            let type_name = reference
                ._ref
                .split('/')
                .last()
                .unwrap_or("Unknown")
                .to_string();

            TypeRef {
                target: StableId::new(&type_name),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            }
        }
        oas::Referenceable::Data(schema) => {
            // Inline schema - need to determine type
            let target = if let Some(_type) = &schema._type {
                match _type.as_str() {
                    "string" | "integer" | "number" | "boolean" => {
                        // For primitives, create a synthetic ID based on the type
                        let primitive = infer_primitive_from_schema(schema);
                        // Return a TypeRef that points to the primitive
                        return TypeRef {
                            target: StableId::new(&format!("Primitive_{:?}", primitive)),
                            optional: false,
                            nullable: schema.nullable.unwrap_or(false),
                            by_ref: false,
                            modifiers: Vec::new(),
                        };
                    }
                    "array" => {
                        if let Some(items_val) = schema.extras.get("items") {
                            if let Ok(items_ref) = serde_json::from_value::<
                                oas::Referenceable<oas::Schema>,
                            >(items_val.clone())
                            {
                                let inner_ref =
                                    convert_schema_to_type_ref(ctx, &items_ref, components);
                                return TypeRef {
                                    target: inner_ref.target,
                                    optional: false,
                                    nullable: schema.nullable.unwrap_or(false),
                                    by_ref: false,
                                    modifiers: vec![TypeMod::List],
                                };
                            }
                        }
                    }
                    _ => {}
                }
                StableId::new("Any")
            } else {
                // No _type field - check if this is a $ref in extras
                if let Some(JsonValue::String(ref_str)) = schema.extras.get("$ref") {
                    // Extract type name from reference like "#/components/schemas/Pet"
                    let type_name = ref_str.split('/').last().unwrap_or("Unknown").to_string();
                    return TypeRef {
                        target: StableId::new(&type_name),
                        optional: false,
                        nullable: schema.nullable.unwrap_or(false),
                        by_ref: false,
                        modifiers: Vec::new(),
                    };
                }
                StableId::new("Any")
            };

            TypeRef {
                target,
                optional: false,
                nullable: schema.nullable.unwrap_or(false),
                by_ref: false,
                modifiers: Vec::new(),
            }
        }
    }
}

/// Infer primitive type from schema
fn infer_primitive_from_schema(schema: &oas::Schema) -> Primitive {
    match schema._type.as_deref() {
        Some("string") => match schema.format.as_deref() {
            Some("date") => Primitive::Date,
            Some("date-time") => Primitive::DateTime,
            Some("uuid") => Primitive::Uuid,
            Some("byte") | Some("binary") => Primitive::Bytes,
            _ => Primitive::String,
        },
        Some("integer") => match schema.format.as_deref() {
            Some("int64") => Primitive::I64,
            _ => Primitive::I32,
        },
        Some("number") => match schema.format.as_deref() {
            Some("double") => Primitive::F64,
            Some("decimal") => Primitive::Decimal,
            _ => Primitive::F32,
        },
        Some("boolean") => Primitive::Bool,
        _ => Primitive::Any,
    }
}

/// Convert servers to ServerSets
fn convert_servers(servers: &Option<Vec<oas::Server>>) -> Vec<ServerSet> {
    let servers = match servers {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };

    let urls: Vec<ServerUrl> = servers
        .iter()
        .map(|server| {
            let template = server.url.clone();
            let resolved_preview = template.clone(); // TODO: resolve variables

            ServerUrl {
                template,
                resolved_preview,
                variables: BTreeMap::new(), // TODO: convert server variables
            }
        })
        .collect();

    vec![ServerSet {
        name: CanonicalName::from_string("default"),
        urls,
    }]
}

/// Convert security schemes to AuthSchemes
fn convert_security_schemes(components: &oas::Components) -> Vec<AuthScheme> {
    let schemes = match &components.security_schemes {
        Some(s) => s,
        None => return Vec::new(),
    };

    schemes
        .iter()
        .filter_map(|(name, scheme_ref)| {
            // Extract scheme from Referenceable
            let scheme = match scheme_ref {
                oas::Referenceable::Data(s) => s,
                oas::Referenceable::Reference(_) => return None, // TODO: resolve references
            };

            let id = StableId::new(name);
            let canonical_name = CanonicalName::from_string(name);

            let kind = match &scheme._type {
                oas::SecurityType::ApiKey {
                    name: param_name,
                    _in,
                } => {
                    let api_location = match _in {
                        oas::ParameterIn::Query => ApiKeyLocation::Query,
                        oas::ParameterIn::Header => ApiKeyLocation::Header,
                        oas::ParameterIn::Cookie => ApiKeyLocation::Cookie,
                        _ => ApiKeyLocation::Header,
                    };

                    AuthKind::ApiKey {
                        location: api_location,
                        param_name: param_name.clone(),
                    }
                }
                oas::SecurityType::Http {
                    scheme,
                    bearer_format,
                } => AuthKind::Http {
                    scheme: scheme.clone(),
                    bearer_format: bearer_format.clone(),
                },
                oas::SecurityType::Oauth2 { flows: _ } => {
                    AuthKind::OAuth2 {
                        flows: Vec::new(), // TODO: convert OAuth flows
                    }
                }
                oas::SecurityType::OpenIdConnect {
                    open_id_connect_url,
                } => AuthKind::OpenIdConnect {
                    url: open_id_connect_url.clone(),
                },
            };

            Some(AuthScheme {
                id,
                name: canonical_name,
                kind,
                docs: Docs {
                    summary: None,
                    description: scheme.description.clone(),
                    deprecated: false,
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
            })
        })
        .collect()
}

/// Convert paths to Services and Operations
fn convert_paths(
    ctx: &mut BuildContext,
    paths: &BTreeMap<String, oas::PathItem>,
    _security: &Option<Vec<oas::SecurityRequirement>>,
    components: Option<&oas::Components>,
) -> Vec<Service> {
    // Group operations by tag (or use "default" if no tag)
    let mut services_map: BTreeMap<String, Vec<Operation>> = BTreeMap::new();

    for (path, path_item) in paths {
        convert_path_item(ctx, path, path_item, &mut services_map, components);
    }

    // Convert grouped operations into Services
    services_map
        .into_iter()
        .map(|(tag, operations)| {
            let id = StableId::new(&tag);
            let name = CanonicalName::from_string(&tag);

            Service {
                id,
                name,
                docs: Docs::default(),
                server_set: None,
                operations,
            }
        })
        .collect()
}

/// Convert a single PathItem to operations
fn convert_path_item(
    ctx: &mut BuildContext,
    path: &str,
    path_item: &oas::PathItem,
    services_map: &mut BTreeMap<String, Vec<Operation>>,
    components: Option<&oas::Components>,
) {
    let methods = [
        ("get", &path_item.get),
        ("post", &path_item.post),
        ("put", &path_item.put),
        ("delete", &path_item.delete),
        ("patch", &path_item.patch),
        ("head", &path_item.head),
        ("options", &path_item.options),
        ("trace", &path_item.trace),
    ];

    for (method_name, operation_opt) in methods {
        if let Some(operation) = operation_opt {
            let op = convert_operation(ctx, path, method_name, operation, components);

            // Group by first tag or "default"
            let tag = operation
                .tags
                .as_ref()
                .and_then(|tags| tags.first())
                .map(|s| s.clone())
                .unwrap_or_else(|| "default".to_string());

            services_map.entry(tag).or_insert_with(Vec::new).push(op);
        }
    }
}

/// Convert an OpenAPI operation to our Operation type
fn convert_operation(
    ctx: &mut BuildContext,
    path: &str,
    method_name: &str,
    operation: &oas::Operation,
    components: Option<&oas::Components>,
) -> Operation {
    let operation_id = operation
        .operation_id
        .clone()
        .unwrap_or_else(|| format!("{}_{}", method_name, path.replace('/', "_")));

    let id = StableId::new(&operation_id);
    let name = CanonicalName::from_string(&operation_id);

    let method = match method_name {
        "get" => HttpMethod::Get,
        "post" => HttpMethod::Post,
        "put" => HttpMethod::Put,
        "delete" => HttpMethod::Delete,
        "patch" => HttpMethod::Patch,
        "head" => HttpMethod::Head,
        "options" => HttpMethod::Options,
        "trace" => HttpMethod::Trace,
        _ => HttpMethod::Get,
    };

    let docs = Docs {
        summary: operation.summary.clone(),
        description: operation.description.clone(),
        deprecated: operation.deprecated.unwrap_or(false),
        since: None,
        examples: Vec::new(),
        external_urls: Vec::new(),
    };

    // Convert parameters
    let mut path_params = Vec::new();
    let mut query = Vec::new();
    let mut headers = Vec::new();
    let mut cookies = Vec::new();

    if let Some(parameters) = &operation.parameters {
        for param_ref in parameters {
            if let oas::Referenceable::Data(param) = param_ref {
                convert_parameter(
                    ctx,
                    param,
                    &mut path_params,
                    &mut query,
                    &mut headers,
                    &mut cookies,
                    components,
                );
            }
        }
    }

    // Convert request body
    let (body, consumes) = if let Some(request_body) = &operation.request_body {
        convert_request_body(ctx, request_body, components)
    } else {
        (None, Vec::new())
    };

    // Convert responses
    let (success, produces) = convert_responses(ctx, &operation.responses, components);

    let http = HttpShape {
        method,
        path_template: path.to_string(),
        segments: Vec::new(), // TODO: parse path segments from template
        query,
        headers,
        cookies,
        path_params,
        body,
        consumes,
        produces,
    };

    Operation {
        id,
        name,
        docs,
        deprecated: operation.deprecated.unwrap_or(false),
        http,
        success,
        alt_success: Vec::new(),
        errors: ErrorUse::None,
        auth: Vec::new(), // TODO: convert security requirements
        pagination: None,
        idempotent: matches!(
            method,
            HttpMethod::Get | HttpMethod::Put | HttpMethod::Delete
        ),
        retryable_statuses: Default::default(),
    }
}

/// Convert a parameter to the appropriate parameter type
fn convert_parameter(
    ctx: &mut BuildContext,
    param: &oas::Parameter,
    path_params: &mut Vec<PathParam>,
    query: &mut Vec<QueryParam>,
    headers: &mut Vec<HeaderParam>,
    cookies: &mut Vec<CookieParam>,
    components: Option<&oas::Components>,
) {
    let name = CanonicalName::from_string(&param.name);
    let docs = Docs {
        summary: param.description.clone(),
        description: None,
        deprecated: param.deprecated.unwrap_or(false),
        since: None,
        examples: Vec::new(),
        external_urls: Vec::new(),
    };

    // Get type from schema
    let ty = if let Some(schema_ref) = &param.schema {
        // Use the actual components passed down from the document
        let components_ref = components.unwrap_or(&oas::Components {
            schemas: None,
            responses: None,
            parameters: None,
            examples: None,
            request_bodies: None,
            headers: None,
            security_schemes: None,
            links: None,
            callbacks: None,
        });
        convert_schema_to_type_ref(ctx, schema_ref, components_ref)
    } else {
        TypeRef {
            target: StableId::new("string"),
            optional: false,
            nullable: false,
            by_ref: false,
            modifiers: Vec::new(),
        }
    };

    let required = param.required.unwrap_or(false);

    match param._in {
        oas::ParameterIn::Path => {
            path_params.push(PathParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
            });
        }
        oas::ParameterIn::Query => {
            query.push(QueryParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
                required,
                default: None, // TODO: parse default value
            });
        }
        oas::ParameterIn::Header => {
            headers.push(HeaderParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
                required,
                default: None,
            });
        }
        oas::ParameterIn::Cookie => {
            cookies.push(CookieParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
                required,
                default: None,
            });
        }
    }
}

/// Convert request body
fn convert_request_body(
    ctx: &mut BuildContext,
    request_body_ref: &oas::Referenceable<oas::RequestBody>,
    components: Option<&oas::Components>,
) -> (Option<Body>, Vec<String>) {
    let request_body = match request_body_ref {
        oas::Referenceable::Data(rb) => rb,
        oas::Referenceable::Reference(_) => return (None, Vec::new()), // TODO: resolve references
    };

    let mut variants = Vec::new();
    let mut consumes = Vec::new();

    for (content_type, media_type) in &request_body.content {
        consumes.push(content_type.clone());

        if let Some(schema_ref) = &media_type.schema {
            let components_ref = components.unwrap_or(&oas::Components {
                schemas: None,
                responses: None,
                parameters: None,
                examples: None,
                request_bodies: None,
                headers: None,
                security_schemes: None,
                links: None,
                callbacks: None,
            });
            let ty = convert_schema_to_type_ref(ctx, schema_ref, components_ref);

            variants.push(BodyVariant {
                content_type: content_type.clone(),
                ty,
                docs: Docs::default(),
                encoding: Vec::new(), // TODO: handle encoding
            });
        }
    }

    let body = if !variants.is_empty() {
        Some(Body {
            preferred: Some("application/json".to_string()),
            variants,
        })
    } else {
        None
    };

    (body, consumes)
}

/// Convert responses to success payload
fn convert_responses(
    ctx: &mut BuildContext,
    responses: &oas::Responses,
    components: Option<&oas::Components>,
) -> (Option<Payload>, Vec<String>) {
    let mut produces = Vec::new();

    // Look for 2xx success responses
    for (status_code, response_ref) in &responses.data {
        if let Ok(code) = status_code.parse::<u16>() {
            if (200..300).contains(&code) {
                let response = match response_ref {
                    oas::Referenceable::Data(r) => r,
                    oas::Referenceable::Reference(_) => continue, // TODO: resolve references
                };

                // Get first content type
                if let Some(content_map) = &response.content {
                    if let Some((content_type, media_type)) = content_map.iter().next() {
                        produces.push(content_type.clone());

                        if let Some(schema_ref) = &media_type.schema {
                            let components_ref = components.unwrap_or(&oas::Components {
                                schemas: None,
                                responses: None,
                                parameters: None,
                                examples: None,
                                request_bodies: None,
                                headers: None,
                                security_schemes: None,
                                links: None,
                                callbacks: None,
                            });
                            let ty = convert_schema_to_type_ref(ctx, schema_ref, components_ref);

                            let payload = Payload {
                                status: StatusSpec::Code(code),
                                content_type: Some(content_type.clone()),
                                ty: Some(ty),
                                headers: Vec::new(), // TODO: convert response headers
                                docs: Docs {
                                    summary: Some(response.description.clone()),
                                    description: None,
                                    deprecated: false,
                                    since: None,
                                    examples: Vec::new(),
                                    external_urls: Vec::new(),
                                },
                            };

                            return (Some(payload), produces);
                        }
                    }
                }

                // Response with no content
                let payload = Payload {
                    status: StatusSpec::Code(code),
                    content_type: None,
                    ty: None,
                    headers: Vec::new(),
                    docs: Docs {
                        summary: Some(response.description.clone()),
                        description: None,
                        deprecated: false,
                        since: None,
                        examples: Vec::new(),
                        external_urls: Vec::new(),
                    },
                };

                return (Some(payload), produces);
            }
        }
    }

    (None, produces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_openapi() {
        // Create a minimal OpenAPI document
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0",
                "description": "A test API"
            },
            "paths": {}
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // Verify basic conversion
        assert_eq!(gen_ir.api.title, "Test API");
        assert_eq!(gen_ir.api.version, "1.0.0");
        assert_eq!(gen_ir.api.package_name.canonical, "Test API");
        assert_eq!(gen_ir.api.package_name.snake, "test_api");
        assert_eq!(gen_ir.api.package_name.pascal, "TestApi");
    }

    #[test]
    fn test_canonical_name() {
        let name = CanonicalName::from_string("my-test-name");
        assert_eq!(name.snake, "my_test_name");
        assert_eq!(name.pascal, "MyTestName");
        assert_eq!(name.camel, "myTestName");
        assert_eq!(name.kebab, "my-test-name");
        assert_eq!(name.upper, "MY_TEST_NAME");
    }

    #[test]
    fn test_name_disambiguation() {
        // Test that schemas with similar names get unique stable IDs
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "description": "Main user type"
                    },
                    "user": {
                        "type": "object",
                        "description": "Lowercase user type"
                    },
                    "USER": {
                        "type": "object",
                        "description": "Uppercase user type"
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // All three should be present with distinct StableIds
        assert_eq!(gen_ir.types.len(), 3);

        let user_id = StableId::new("User");
        let user_lower_id = StableId::new("user");
        let user_upper_id = StableId::new("USER");

        assert!(gen_ir.types.contains_key(&user_id));
        assert!(gen_ir.types.contains_key(&user_lower_id));
        assert!(gen_ir.types.contains_key(&user_upper_id));

        // Verify descriptions are preserved correctly
        assert_eq!(
            gen_ir.types.get(&user_id).unwrap().docs.description,
            Some("Main user type".to_string())
        );
        assert_eq!(
            gen_ir.types.get(&user_lower_id).unwrap().docs.description,
            Some("Lowercase user type".to_string())
        );
        assert_eq!(
            gen_ir.types.get(&user_upper_id).unwrap().docs.description,
            Some("Uppercase user type".to_string())
        );
    }

    #[test]
    fn test_nested_references() {
        // Test schemas that reference other schemas
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Address": {
                        "type": "object",
                        "description": "Address type"
                    },
                    "Person": {
                        "type": "object",
                        "description": "Person with nested address reference"
                    },
                    "Company": {
                        "type": "object",
                        "description": "Company with nested person reference"
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // All three types should be present
        assert_eq!(gen_ir.types.len(), 3);

        let address_id = StableId::new("Address");
        let person_id = StableId::new("Person");
        let company_id = StableId::new("Company");

        assert!(gen_ir.types.contains_key(&address_id));
        assert!(gen_ir.types.contains_key(&person_id));
        assert!(gen_ir.types.contains_key(&company_id));

        // Verify all are struct types (since we don't have properties, they'll be empty structs)
        for (_, type_decl) in &gen_ir.types {
            match &type_decl.kind {
                TypeKind::Struct {
                    fields,
                    additional,
                    discriminator,
                } => {
                    assert_eq!(fields.len(), 0); // No properties in our test
                    // additionalProperties defaults to Any when not specified
                    assert!(matches!(additional, Additional::Any));
                    assert!(discriminator.is_none());
                }
                _ => panic!("Expected Struct type"),
            }
        }
    }

    #[test]
    fn test_discriminated_union_structure() {
        // Test discriminated unions (oneOf with discriminator)
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Cat": {
                        "type": "object",
                        "description": "A cat"
                    },
                    "Dog": {
                        "type": "object",
                        "description": "A dog"
                    },
                    "Bird": {
                        "type": "object",
                        "description": "A bird"
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // All three animal types should be present
        assert_eq!(gen_ir.types.len(), 3);

        let cat_id = StableId::new("Cat");
        let dog_id = StableId::new("Dog");
        let bird_id = StableId::new("Bird");

        assert!(gen_ir.types.contains_key(&cat_id));
        assert!(gen_ir.types.contains_key(&dog_id));
        assert!(gen_ir.types.contains_key(&bird_id));
    }

    #[test]
    fn test_primitive_type_conversion() {
        // Test various primitive types and formats
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "StringType": {
                        "type": "string"
                    },
                    "DateType": {
                        "type": "string",
                        "format": "date"
                    },
                    "DateTimeType": {
                        "type": "string",
                        "format": "date-time"
                    },
                    "UuidType": {
                        "type": "string",
                        "format": "uuid"
                    },
                    "IntType": {
                        "type": "integer"
                    },
                    "Int64Type": {
                        "type": "integer",
                        "format": "int64"
                    },
                    "FloatType": {
                        "type": "number",
                        "format": "float"
                    },
                    "DoubleType": {
                        "type": "number",
                        "format": "double"
                    },
                    "BoolType": {
                        "type": "boolean"
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        assert_eq!(gen_ir.types.len(), 9);

        // Verify primitive types are converted to Alias with correct Primitive
        let string_type = gen_ir.types.get(&StableId::new("StringType")).unwrap();
        match &string_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::String),
            } => {}
            _ => panic!("Expected String primitive alias"),
        }

        let date_type = gen_ir.types.get(&StableId::new("DateType")).unwrap();
        match &date_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Date),
            } => {}
            _ => panic!("Expected Date primitive alias"),
        }

        let datetime_type = gen_ir.types.get(&StableId::new("DateTimeType")).unwrap();
        match &datetime_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::DateTime),
            } => {}
            _ => panic!("Expected DateTime primitive alias"),
        }

        let uuid_type = gen_ir.types.get(&StableId::new("UuidType")).unwrap();
        match &uuid_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Uuid),
            } => {}
            _ => panic!("Expected Uuid primitive alias"),
        }

        let int_type = gen_ir.types.get(&StableId::new("IntType")).unwrap();
        match &int_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::I32),
            } => {}
            _ => panic!("Expected I32 primitive alias"),
        }

        let int64_type = gen_ir.types.get(&StableId::new("Int64Type")).unwrap();
        match &int64_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::I64),
            } => {}
            _ => panic!("Expected I64 primitive alias"),
        }

        let double_type = gen_ir.types.get(&StableId::new("DoubleType")).unwrap();
        match &double_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::F64),
            } => {}
            _ => panic!("Expected F64 primitive alias"),
        }

        let bool_type = gen_ir.types.get(&StableId::new("BoolType")).unwrap();
        match &bool_type.kind {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Bool),
            } => {}
            _ => panic!("Expected Bool primitive alias"),
        }
    }

    #[test]
    fn test_canonical_name_edge_cases() {
        // Test edge cases in name conversion
        let test_cases = vec![
            ("HTTPClient", "httpclient", "Httpclient", "httpclient"),
            ("API", "api", "Api", "api"),
            ("XMLParser", "xmlparser", "Xmlparser", "xmlparser"),
            ("user_id", "user_id", "UserId", "userId"),
            (
                "kebab-case-name",
                "kebab_case_name",
                "KebabCaseName",
                "kebabCaseName",
            ),
            (
                "camelCaseName",
                "camel_case_name",
                "CamelCaseName",
                "camelCaseName",
            ),
            ("UPPER_SNAKE", "upper_snake", "UpperSnake", "upperSnake"),
            ("123number", "123number", "123number", "123number"),
        ];

        for (input, expected_snake, expected_pascal, expected_camel) in test_cases {
            let name = CanonicalName::from_string(input);
            assert_eq!(
                name.snake, expected_snake,
                "Snake case failed for input: {}",
                input
            );
            assert_eq!(
                name.pascal, expected_pascal,
                "Pascal case failed for input: {}",
                input
            );
            assert_eq!(
                name.camel, expected_camel,
                "Camel case failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_service_grouping_by_tags() {
        // Test that operations are grouped into services by tags
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/users": {
                    "get": {
                        "operationId": "listUsers",
                        "summary": "List all users",
                        "tags": ["Users"],
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                },
                "/users/{id}": {
                    "get": {
                        "operationId": "getUser",
                        "summary": "Get a user",
                        "tags": ["Users"],
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                },
                "/products": {
                    "get": {
                        "operationId": "listProducts",
                        "summary": "List all products",
                        "tags": ["Products"],
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // Should have 2 services: Users and Products
        assert_eq!(gen_ir.services.len(), 2);

        // Find Users service
        let users_service = gen_ir
            .services
            .iter()
            .find(|s| s.name.canonical == "Users")
            .expect("Users service not found");

        assert_eq!(users_service.operations.len(), 2);
        assert!(
            users_service
                .operations
                .iter()
                .any(|op| op.name.canonical == "listUsers")
        );
        assert!(
            users_service
                .operations
                .iter()
                .any(|op| op.name.canonical == "getUser")
        );

        // Find Products service
        let products_service = gen_ir
            .services
            .iter()
            .find(|s| s.name.canonical == "Products")
            .expect("Products service not found");

        assert_eq!(products_service.operations.len(), 1);
        assert_eq!(
            products_service.operations[0].name.canonical,
            "listProducts"
        );
    }

    #[test]
    fn test_http_method_conversion() {
        // Test that all HTTP methods are properly converted
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/resource": {
                    "get": {
                        "operationId": "getResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "post": {
                        "operationId": "createResource",
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    },
                    "put": {
                        "operationId": "updateResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "delete": {
                        "operationId": "deleteResource",
                        "responses": {
                            "204": {
                                "description": "No Content"
                            }
                        }
                    },
                    "patch": {
                        "operationId": "patchResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "head": {
                        "operationId": "headResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "options": {
                        "operationId": "optionsResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        // Should have 1 service with 7 operations
        assert_eq!(gen_ir.services.len(), 1);
        let service = &gen_ir.services[0];
        assert_eq!(service.operations.len(), 7);

        // Verify each operation has correct HTTP method
        let methods = vec![
            ("getResource", HttpMethod::Get),
            ("createResource", HttpMethod::Post),
            ("updateResource", HttpMethod::Put),
            ("deleteResource", HttpMethod::Delete),
            ("patchResource", HttpMethod::Patch),
            ("headResource", HttpMethod::Head),
            ("optionsResource", HttpMethod::Options),
        ];

        for (op_id, expected_method) in methods {
            let op = service
                .operations
                .iter()
                .find(|o| o.name.canonical == op_id)
                .expect(&format!("Operation {} not found", op_id));

            assert_eq!(op.http.method, expected_method);
        }
    }

    #[test]
    fn test_idempotent_operations() {
        // Test that idempotency is correctly inferred from HTTP method
        let json = r#"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/resource": {
                    "get": {
                        "operationId": "getResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "post": {
                        "operationId": "createResource",
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    },
                    "put": {
                        "operationId": "updateResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    },
                    "delete": {
                        "operationId": "deleteResource",
                        "responses": {
                            "204": {
                                "description": "No Content"
                            }
                        }
                    },
                    "patch": {
                        "operationId": "patchResource",
                        "responses": {
                            "200": {
                                "description": "Success"
                            }
                        }
                    }
                }
            }
        }"#;

        let doc: oas::OpenAPIV3 = serde_json::from_str(json).expect("Failed to parse OpenAPI");
        let gen_ir = GenIr::from(doc);

        let service = &gen_ir.services[0];

        // GET, PUT, DELETE should be idempotent
        let get_op = service
            .operations
            .iter()
            .find(|o| o.name.canonical == "getResource")
            .unwrap();
        assert!(get_op.idempotent);

        let put_op = service
            .operations
            .iter()
            .find(|o| o.name.canonical == "updateResource")
            .unwrap();
        assert!(put_op.idempotent);

        let delete_op = service
            .operations
            .iter()
            .find(|o| o.name.canonical == "deleteResource")
            .unwrap();
        assert!(delete_op.idempotent);

        // POST and PATCH should not be idempotent
        let post_op = service
            .operations
            .iter()
            .find(|o| o.name.canonical == "createResource")
            .unwrap();
        assert!(!post_op.idempotent);

        let patch_op = service
            .operations
            .iter()
            .find(|o| o.name.canonical == "patchResource")
            .unwrap();
        assert!(!patch_op.idempotent);
    }
}
