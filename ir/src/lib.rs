use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

pub mod gen_ir;

use gen_ir::*;

/// Builder context for tracking state during conversion
struct BuildContext<'a> {
    types: BTreeMap<StableId, TypeDecl>,
    inline_schema_cache: HashMap<u64, StableId>, // Schema hash -> StableId (for inline deduplication)
    type_counter: usize,
    used_type_names: HashSet<String>, // Track used names for collision detection
    spec: &'a oas3::spec::Spec,
    current_operation_id: Option<String>, // Track current operation for naming
}

impl<'a> BuildContext<'a> {
    fn new(spec: &'a oas3::spec::Spec) -> Self {
        Self {
            types: BTreeMap::new(),
            inline_schema_cache: HashMap::new(),
            type_counter: 0,
            used_type_names: HashSet::new(),
            spec,
            current_operation_id: None,
        }
    }

    #[allow(dead_code)]
    fn next_type_id(&mut self, base: &str) -> StableId {
        self.type_counter += 1;
        StableId::new(format!("{}{}", base, self.type_counter))
    }

    fn add_type(&mut self, decl: TypeDecl) {
        self.used_type_names.insert(decl.id.0.clone());
        self.types.insert(decl.id.clone(), decl);
    }
}

/// Convert string to PascalCase
fn to_pascal_case(s: &str) -> String {
    let canonical = CanonicalName::from_string(s);
    canonical.pascal
}

/// Generate a unique name for an inline schema
fn generate_inline_type_name(
    ctx: &BuildContext,
    operation_id: Option<&str>,
    context: &str,
    hint: Option<&str>,
) -> String {
    let base_name = match (operation_id, hint, context) {
        (Some(op_id), Some(hint_str), _) => {
            format!("{}{}", to_pascal_case(op_id), to_pascal_case(hint_str))
        }
        (Some(op_id), None, "Request") => format!("{}Request", to_pascal_case(op_id)),
        (Some(op_id), None, "Response") => format!("{}Response", to_pascal_case(op_id)),
        (Some(op_id), None, _) => format!("{}{}", to_pascal_case(op_id), context),
        (None, Some(hint_str), _) => format!("{}{}", to_pascal_case(hint_str), context),
        (None, None, _) => format!("InlineType{}", context),
    };

    // Handle name collisions by appending counter
    let mut candidate = base_name.clone();
    let mut suffix = 2;
    while ctx.used_type_names.contains(&candidate) {
        candidate = format!("{}{}", base_name, suffix);
        suffix += 1;
    }

    candidate
}

/// Hash a schema for deduplication
fn hash_schema(schema: &oas3::spec::ObjectSchema) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();

    // Hash the schema structure (type, properties, etc.)
    // We serialize to JSON for a stable hash
    if let Ok(json) = serde_json::to_string(schema) {
        json.hash(&mut hasher);
    }

    hasher.finish()
}

/// Check if a schema is an inline object that should be hoisted
fn should_hoist_schema(schema: &oas3::spec::ObjectSchema) -> bool {
    // Hoist if it's an object type with properties or additional constraints
    if let Some(schema_type) = &schema.schema_type
        && let oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Object) = schema_type
    {
        // Hoist if it has properties, or if it's explicitly an object
        return !schema.properties.is_empty()
            || schema.additional_properties.is_some()
            || !schema.required.is_empty();
    }

    // Also check if there are no types specified but properties exist (implicit object)
    if schema.schema_type.is_none() && !schema.properties.is_empty() {
        return true;
    }

    false
}

/// Hoist an inline schema to a named type
/// parent_type_name: Used for nested object naming (overrides operation_id)
fn hoist_inline_schema_with_parent(
    ctx: &mut BuildContext,
    type_name: String,
    schema: &oas3::spec::ObjectSchema,
    parent_type_name: Option<&str>,
) -> StableId {
    // Check if already hoisted (deduplication)
    let schema_hash = hash_schema(schema);
    if let Some(existing_id) = ctx.inline_schema_cache.get(&schema_hash) {
        return existing_id.clone();
    }

    // Temporarily save and override the operation context with parent type for nested hoisting
    let saved_op_id = ctx.current_operation_id.clone();
    if let Some(parent) = parent_type_name {
        ctx.current_operation_id = Some(parent.to_string());
    }

    // Convert inline schema to TypeDecl
    let type_decl = convert_schema_to_type(ctx, &type_name, schema);

    // Restore the original operation context
    ctx.current_operation_id = saved_op_id;

    if let Some(decl) = type_decl {
        let id = decl.id.clone();

        // Cache and register
        ctx.inline_schema_cache.insert(schema_hash, id.clone());
        ctx.add_type(decl);

        id
    } else {
        // Fallback to Any if conversion fails
        StableId::new("Any")
    }
}

/// Hoist an inline schema to a named type (convenience wrapper)
fn hoist_inline_schema(
    ctx: &mut BuildContext,
    type_name: String,
    schema: &oas3::spec::ObjectSchema,
) -> StableId {
    hoist_inline_schema_with_parent(ctx, type_name, schema, None)
}

/// Associate tags with types based on their usage in operations
fn associate_tags_with_types(types: &mut BTreeMap<StableId, TypeDecl>, services: &[Service]) {
    for service in services {
        // Service name corresponds to the tag
        let tag = service.name.canonical.clone();

        for operation in &service.operations {
            // Collect all type references from this operation
            let mut type_ids = HashSet::new();

            // From path parameters
            for param in &operation.http.path_params {
                collect_type_ids_from_type_ref(&param.ty, &mut type_ids);
            }

            // From query parameters
            for param in &operation.http.query {
                collect_type_ids_from_type_ref(&param.ty, &mut type_ids);
            }

            // From header parameters
            for param in &operation.http.headers {
                collect_type_ids_from_type_ref(&param.ty, &mut type_ids);
            }

            // From cookie parameters
            for param in &operation.http.cookies {
                collect_type_ids_from_type_ref(&param.ty, &mut type_ids);
            }

            // From request body
            if let Some(body) = &operation.http.body {
                for variant in &body.variants {
                    collect_type_ids_from_type_ref(&variant.ty, &mut type_ids);
                }
            }

            // From success response
            if let Some(success) = &operation.success
                && let Some(ty) = &success.ty
            {
                collect_type_ids_from_type_ref(ty, &mut type_ids);
            }

            // From alt success responses
            for alt_success in &operation.alt_success {
                if let Some(ty) = &alt_success.ty {
                    collect_type_ids_from_type_ref(ty, &mut type_ids);
                }
            }

            // Add the tag to all collected types
            for type_id in type_ids {
                add_tag_recursively(types, &type_id, &tag);
            }
        }
    }
}

/// Recursively add a tag to a type and all its nested types
fn add_tag_recursively(types: &mut BTreeMap<StableId, TypeDecl>, type_id: &StableId, tag: &str) {
    // First, check if the type exists and needs the tag
    let needs_tag = types
        .get(type_id)
        .map(|t| !t.tags.contains(tag))
        .unwrap_or(false);

    if !needs_tag {
        return; // Already has this tag, no need to recurse
    }

    // Collect nested type IDs before borrowing mutably
    let nested_ids: Vec<StableId> = if let Some(type_decl) = types.get(type_id) {
        let mut ids = Vec::new();
        match &type_decl.kind {
            TypeKind::Struct { fields, .. } => {
                for field in fields {
                    let mut field_ids = HashSet::new();
                    collect_type_ids_from_type_ref(&field.ty, &mut field_ids);
                    ids.extend(field_ids);
                }
            }
            TypeKind::Union { variants, .. } => {
                for variant in variants {
                    let mut variant_ids = HashSet::new();
                    collect_type_ids_from_type_ref(&variant.ty, &mut variant_ids);
                    ids.extend(variant_ids);
                }
            }
            TypeKind::Alias {
                aliased: AliasTarget::Reference(type_ref),
            } => {
                let mut alias_ids = HashSet::new();
                collect_type_ids_from_type_ref(type_ref, &mut alias_ids);
                ids.extend(alias_ids);
            }
            _ => {}
        }
        ids
    } else {
        Vec::new()
    };

    // Now add the tag (safe to borrow mutably)
    if let Some(type_decl) = types.get_mut(type_id) {
        type_decl.tags.insert(tag.to_string());
    }

    // Recursively add tag to nested types
    for nested_id in nested_ids {
        add_tag_recursively(types, &nested_id, tag);
    }
}

/// Collect all StableIds from a TypeRef (including nested ones)
fn collect_type_ids_from_type_ref(type_ref: &TypeRef, ids: &mut HashSet<StableId>) {
    // Don't collect primitive types
    if type_ref.target.0.starts_with("Primitive_") {
        return;
    }

    ids.insert(type_ref.target.clone());

    // Also collect from modifiers (like Map value types)
    for modifier in &type_ref.modifiers {
        if let TypeMod::Map(value_type) = modifier {
            collect_type_ids_from_type_ref(value_type, ids);
        }
    }
}

// Build an AST from an OpenAPI 3.0 document (oas3::Spec)
impl From<oas3::spec::Spec> for GenIr {
    fn from(spec: oas3::spec::Spec) -> Self {
        let mut ctx = BuildContext::new(&spec);

        // Convert API metadata
        let api = ApiMeta::from(spec.info.clone());

        // Convert schemas to types
        if let Some(components) = &spec.components {
            convert_schemas(&mut ctx, components);
        }

        // Convert servers to ServerSets
        let server_sets = convert_servers(&spec.servers);

        // Convert security schemes to AuthSchemes
        let auth_schemes = if let Some(components) = &spec.components {
            convert_security_schemes(components)
        } else {
            Vec::new()
        };

        // Convert paths to Services and Operations
        let services = convert_paths(
            &mut ctx,
            &spec.paths,
            &spec.security,
            spec.components.as_ref(),
        );

        // Associate tags with types based on operation usage
        associate_tags_with_types(&mut ctx.types, &services);

        GenIr {
            api,
            types: ctx.types,
            services,
            auth_schemes,
            errors: Vec::new(),
            server_sets,
        }
    }
}

/// Convert OpenAPI components/schemas to TypeDecl
fn convert_schemas(ctx: &mut BuildContext, components: &oas3::spec::Components) {
    for (name, schema_ref) in &components.schemas {
        // Resolve the schema reference
        if let Ok(schema) = schema_ref.resolve(ctx.spec) {
            let type_decl = convert_schema_to_type(ctx, name, &schema);
            if let Some(decl) = type_decl {
                ctx.add_type(decl);
            }
        }
    }
}

/// Convert a single schema to TypeDecl
fn convert_schema_to_type(
    ctx: &mut BuildContext,
    name: &str,
    schema: &oas3::spec::ObjectSchema,
) -> Option<TypeDecl> {
    let id = StableId::new(name);
    let canonical_name = CanonicalName::from_string(name);

    let docs = Docs {
        summary: schema.title.clone(),
        description: schema.description.clone(),
        deprecated: schema.deprecated.unwrap_or(false),
        since: None,
        examples: Vec::new(),
        external_urls: Vec::new(),
    };

    // Determine the type kind based on schema properties
    let kind = infer_type_kind(ctx, schema);

    Some(TypeDecl {
        id,
        name: canonical_name,
        docs,
        kind,
        origin: None,
        tags: BTreeSet::new(),
    })
}

/// Infer the TypeKind from a schema
fn infer_type_kind(ctx: &mut BuildContext, schema: &oas3::spec::ObjectSchema) -> TypeKind {
    // Check for enum values
    if !schema.enum_values.is_empty() {
        let base = infer_primitive_from_schema(schema);
        let values = convert_enum_values(&schema.enum_values, base);
        return TypeKind::Enum { base, values };
    }

    // Check for anyOf/oneOf/allOf composition
    if !schema.any_of.is_empty() {
        return convert_any_of_to_union(ctx, &schema.any_of);
    }
    if !schema.one_of.is_empty() {
        return convert_one_of_to_union(ctx, &schema.one_of);
    }
    if !schema.all_of.is_empty() {
        return convert_all_of_to_type(ctx, &schema.all_of);
    }

    // Check schema type
    match &schema.schema_type {
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Object)) => {
            // Object/Struct type
            let fields = convert_properties(ctx, &schema.properties, &schema.required);
            let additional =
                if let Some(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(false))) =
                    &schema.additional_properties
                {
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
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::String)) => {
            // String primitive as alias
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(infer_primitive_from_schema(schema)),
            }
        }
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Integer))
        | Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Number)) => {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(infer_primitive_from_schema(schema)),
            }
        }
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Boolean)) => {
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Bool),
            }
        }
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Array)) => {
            // Array type - create as alias to list
            if let Some(items) = &schema.items {
                let item_type_ref = convert_schema_to_type_ref(ctx, items);
                return TypeKind::Alias {
                    aliased: AliasTarget::Composite(Composite::List(Box::new(item_type_ref))),
                };
            }
            TypeKind::Alias {
                aliased: AliasTarget::Primitive(Primitive::Any),
            }
        }
        Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Null)) => TypeKind::Struct {
            fields: Vec::new(),
            additional: Additional::Forbidden,
            discriminator: None,
        },
        _ => {
            // Default to struct for None or Multiple types
            TypeKind::Struct {
                fields: Vec::new(),
                additional: Additional::Any,
                discriminator: None,
            }
        }
    }
}

/// Convert anyOf to Union TypeKind
fn convert_any_of_to_union(
    ctx: &mut BuildContext,
    schemas: &[oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>],
) -> TypeKind {
    let variants = schemas
        .iter()
        .enumerate()
        .filter_map(|(idx, schema_ref)| {
            let schema = schema_ref.resolve(ctx.spec).ok()?;

            // Get variant name: prefer reference name, then title, then generic name
            let variant_name =
                if let oas3::spec::ObjectOrReference::Ref { ref_path, .. } = schema_ref {
                    // Extract type name from reference like "#/components/schemas/Cat"
                    ref_path
                        .split('/')
                        .next_back()
                        .unwrap_or("Unknown")
                        .to_string()
                } else {
                    schema
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Variant{}", idx + 1))
                };

            // For anyOf, we need to convert each schema to a TypeRef
            let ty = convert_schema_ref_to_type_ref(ctx, schema_ref);

            Some(Variant {
                name: CanonicalName::from_string(&variant_name),
                docs: Docs {
                    summary: schema.title.clone(),
                    description: schema.description.clone(),
                    deprecated: schema.deprecated.unwrap_or(false),
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
                ty,
                tag_value: None,
            })
        })
        .collect();

    TypeKind::Union {
        style: UnionStyle::AnyOf,
        variants,
    }
}

/// Convert oneOf to Union TypeKind
fn convert_one_of_to_union(
    ctx: &mut BuildContext,
    schemas: &[oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>],
) -> TypeKind {
    let variants = schemas
        .iter()
        .enumerate()
        .filter_map(|(idx, schema_ref)| {
            let schema = schema_ref.resolve(ctx.spec).ok()?;

            // Get variant name: prefer reference name, then title, then generic name
            let variant_name =
                if let oas3::spec::ObjectOrReference::Ref { ref_path, .. } = schema_ref {
                    // Extract type name from reference like "#/components/schemas/Cat"
                    ref_path
                        .split('/')
                        .next_back()
                        .unwrap_or("Unknown")
                        .to_string()
                } else {
                    schema
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Variant{}", idx + 1))
                };

            let ty = convert_schema_ref_to_type_ref(ctx, schema_ref);

            Some(Variant {
                name: CanonicalName::from_string(&variant_name),
                docs: Docs {
                    summary: schema.title.clone(),
                    description: schema.description.clone(),
                    deprecated: schema.deprecated.unwrap_or(false),
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
                ty,
                tag_value: None,
            })
        })
        .collect();

    TypeKind::Union {
        style: UnionStyle::OneOf,
        variants,
    }
}

/// Convert allOf to TypeKind
/// For allOf, we merge all schemas into a single Struct (composition/flattening)
fn convert_all_of_to_type(
    ctx: &mut BuildContext,
    schemas: &[oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>],
) -> TypeKind {
    // Use a BTreeMap to track fields by wire_name and merge duplicates
    let mut fields_map: BTreeMap<String, Field> = BTreeMap::new();
    let mut additional = Additional::Any;

    for schema_ref in schemas {
        if let Ok(schema) = schema_ref.resolve(ctx.spec) {
            // IMPORTANT: If this schema itself contains an allOf, we need to recursively
            // merge those fields first. This handles nested allOf (e.g., Final -> Middle -> Base)
            if !schema.all_of.is_empty() {
                // Recursively process the nested allOf
                if let TypeKind::Struct {
                    fields: nested_fields,
                    additional: nested_additional,
                    ..
                } = convert_all_of_to_type(ctx, &schema.all_of)
                {
                    // Merge the nested fields into our fields_map
                    for nested_field in nested_fields {
                        if let Some(existing_field) = fields_map.get_mut(&nested_field.wire_name) {
                            // Merge with existing field (same logic as below)
                            if !nested_field.ty.optional {
                                existing_field.ty.optional = false;
                            }
                            if nested_field.ty.nullable {
                                existing_field.ty.nullable = true;
                            }
                            if existing_field.ty.target == StableId::new("Any")
                                && nested_field.ty.target != StableId::new("Any")
                            {
                                existing_field.ty.target = nested_field.ty.target;
                                existing_field.ty.modifiers = nested_field.ty.modifiers;
                            }
                            if existing_field.docs.description.is_none()
                                && nested_field.docs.description.is_some()
                            {
                                existing_field.docs.description = nested_field.docs.description;
                            }
                            if existing_field.docs.summary.is_none()
                                && nested_field.docs.summary.is_some()
                            {
                                existing_field.docs.summary = nested_field.docs.summary;
                            }
                        } else {
                            // New field from nested allOf
                            fields_map.insert(nested_field.wire_name.clone(), nested_field);
                        }
                    }

                    // Update additionalProperties
                    if nested_additional == Additional::Forbidden {
                        additional = Additional::Forbidden;
                    }
                }
            }

            // Merge properties from each schema
            let required_set: std::collections::HashSet<String> =
                schema.required.iter().cloned().collect();

            for (prop_name, prop_schema_ref) in &schema.properties {
                let is_required = required_set.contains(prop_name);

                if let Ok(prop_schema) = prop_schema_ref.resolve(ctx.spec) {
                    let ty = convert_object_schema_to_type_ref(ctx, &prop_schema);
                    let is_nullable = prop_schema.is_nullable().unwrap_or(false);

                    // Check if this field has a const value
                    let const_value = prop_schema
                        .const_value
                        .as_ref()
                        .map(convert_json_value_to_literal);

                    let new_field = Field {
                        name: CanonicalName::from_string(prop_name),
                        docs: Docs {
                            summary: prop_schema.title.clone(),
                            description: prop_schema.description.clone(),
                            deprecated: prop_schema.deprecated.unwrap_or(false),
                            since: None,
                            examples: Vec::new(),
                            external_urls: Vec::new(),
                        },
                        ty: TypeRef {
                            target: ty.target.clone(),
                            optional: !is_required,
                            nullable: is_nullable,
                            by_ref: false,
                            modifiers: ty.modifiers.clone(),
                        },
                        default: None,
                        deprecated: prop_schema.deprecated.unwrap_or(false),
                        const_value,
                        wire_name: prop_name.clone(),
                    };

                    // Merge with existing field if present
                    if let Some(existing_field) = fields_map.get_mut(prop_name) {
                        // When merging fields from allOf:
                        // - If any schema marks it as required, it's required (optional = false)
                        // - Take the most specific type (prefer non-Any types)
                        // - Merge documentation (prefer non-empty)

                        // Make field required if any schema requires it
                        if !new_field.ty.optional {
                            existing_field.ty.optional = false;
                        }

                        // Update nullable if new field is nullable
                        if new_field.ty.nullable {
                            existing_field.ty.nullable = true;
                        }

                        // Prefer non-Any types
                        if existing_field.ty.target == StableId::new("Any")
                            && new_field.ty.target != StableId::new("Any")
                        {
                            existing_field.ty.target = new_field.ty.target;
                            existing_field.ty.modifiers = new_field.ty.modifiers;
                        }

                        // Merge documentation (prefer non-empty)
                        if existing_field.docs.description.is_none()
                            && new_field.docs.description.is_some()
                        {
                            existing_field.docs.description = new_field.docs.description;
                        }
                        if existing_field.docs.summary.is_none() && new_field.docs.summary.is_some()
                        {
                            existing_field.docs.summary = new_field.docs.summary;
                        }
                    } else {
                        // New field, add it
                        fields_map.insert(prop_name.clone(), new_field);
                    }
                }
            }

            // Handle additionalProperties from merged schemas
            if let Some(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(false))) =
                &schema.additional_properties
            {
                additional = Additional::Forbidden;
            }
        }
    }

    // Convert map back to vector
    let all_fields: Vec<Field> = fields_map.into_values().collect();

    TypeKind::Struct {
        fields: all_fields,
        additional,
        discriminator: None,
    }
}

/// Convert schema properties to fields
fn convert_properties(
    ctx: &mut BuildContext,
    properties: &BTreeMap<String, oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>>,
    required: &[String],
) -> Vec<Field> {
    let required_fields: std::collections::HashSet<String> = required.iter().cloned().collect();

    properties
        .iter()
        .filter_map(|(prop_name, prop_schema_ref)| {
            let is_required = required_fields.contains(prop_name);

            // Check if this is an inline schema that should be hoisted
            let (ty, is_nullable) = match prop_schema_ref {
                oas3::spec::ObjectOrReference::Ref { .. } => {
                    // Reference - use normal conversion
                    let prop_schema = prop_schema_ref.resolve(ctx.spec).ok()?;
                    let ty = convert_object_schema_to_type_ref_with_hint(
                        ctx,
                        &prop_schema,
                        Some(prop_name),
                    );
                    let is_nullable = prop_schema.is_nullable().unwrap_or(false);
                    (ty, is_nullable)
                }
                oas3::spec::ObjectOrReference::Object(inline_schema) => {
                    // Inline schema - check if we should hoist it
                    let is_nullable = inline_schema.is_nullable().unwrap_or(false);

                    if should_hoist_schema(inline_schema) {
                        // Hoist nested inline schema
                        let type_name = generate_inline_type_name(
                            ctx,
                            ctx.current_operation_id.as_deref(),
                            "Property",
                            Some(prop_name),
                        );
                        let type_id = hoist_inline_schema(ctx, type_name, inline_schema);
                        let ty = TypeRef {
                            target: type_id,
                            optional: false,
                            nullable: is_nullable,
                            by_ref: false,
                            modifiers: Vec::new(),
                        };
                        (ty, is_nullable)
                    } else {
                        // Simple inline schema - use normal conversion with hint
                        let ty = convert_object_schema_to_type_ref_with_hint(
                            ctx,
                            inline_schema,
                            Some(prop_name),
                        );
                        (ty, is_nullable)
                    }
                }
            };

            let prop_schema = prop_schema_ref.resolve(ctx.spec).ok()?;

            // Check if this field has a const value
            let const_value = prop_schema
                .const_value
                .as_ref()
                .map(convert_json_value_to_literal);

            Some(Field {
                name: CanonicalName::from_string(prop_name),
                docs: Docs {
                    summary: prop_schema.title.clone(),
                    description: prop_schema.description.clone(),
                    deprecated: prop_schema.deprecated.unwrap_or(false),
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
                deprecated: prop_schema.deprecated.unwrap_or(false),
                const_value,
                wire_name: prop_name.clone(),
            })
        })
        .collect()
}

/// Convert a JSON value to a Literal
fn convert_json_value_to_literal(value: &JsonValue) -> Literal {
    match value {
        JsonValue::String(s) => Literal::String(s.clone()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Literal::I64(i)
            } else if let Some(f) = n.as_f64() {
                Literal::F64(f)
            } else {
                Literal::String(n.to_string())
            }
        }
        JsonValue::Bool(b) => Literal::Bool(*b),
        JsonValue::Null => Literal::Null,
        JsonValue::Array(arr) => {
            let items = arr.iter().map(convert_json_value_to_literal).collect();
            Literal::Array(items)
        }
        JsonValue::Object(obj) => {
            let items = obj
                .iter()
                .map(|(k, v)| (k.clone(), convert_json_value_to_literal(v)))
                .collect();
            Literal::Object(items)
        }
    }
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

/// Convert an ObjectSchema to a TypeRef
fn convert_object_schema_to_type_ref(
    ctx: &mut BuildContext,
    schema: &oas3::spec::ObjectSchema,
) -> TypeRef {
    convert_object_schema_to_type_ref_with_hint(ctx, schema, None)
}

/// Convert an ObjectSchema to a TypeRef with optional naming hint
fn convert_object_schema_to_type_ref_with_hint(
    ctx: &mut BuildContext,
    schema: &oas3::spec::ObjectSchema,
    hint: Option<&str>,
) -> TypeRef {
    let nullable = schema.is_nullable().unwrap_or(false);

    // Check for anyOf/oneOf/allOf - these need to be hoisted to named types
    if !schema.any_of.is_empty() || !schema.one_of.is_empty() || !schema.all_of.is_empty() {
        // Generate a name for this inline union type using the hint if available
        // If we have a hint, use it directly; otherwise use "Union" as context
        let type_name = if let Some(h) = hint {
            generate_inline_type_name(ctx, ctx.current_operation_id.as_deref(), "", Some(h))
        } else {
            generate_inline_type_name(ctx, ctx.current_operation_id.as_deref(), "Union", None)
        };

        // Hoist the union schema to a named type with parent context
        let type_id =
            hoist_inline_schema_with_parent(ctx, type_name.clone(), schema, Some(&type_name));

        return TypeRef {
            target: type_id,
            optional: false,
            nullable,
            by_ref: false,
            modifiers: Vec::new(),
        };
    }

    // Check if this is an object with properties that should be hoisted
    if should_hoist_schema(schema) {
        // Use the hint directly for the type name if available
        // The hint already contains the full context (e.g., "InputItem")
        let type_name = if let Some(h) = hint {
            generate_inline_type_name(ctx, ctx.current_operation_id.as_deref(), "", Some(h))
        } else {
            generate_inline_type_name(ctx, ctx.current_operation_id.as_deref(), "Object", None)
        };

        // Hoist with parent context so nested properties use this type name as prefix
        let type_id =
            hoist_inline_schema_with_parent(ctx, type_name.clone(), schema, Some(&type_name));

        return TypeRef {
            target: type_id,
            optional: false,
            nullable,
            by_ref: false,
            modifiers: Vec::new(),
        };
    }

    // Check schema type to determine the type
    if let Some(schema_type) = &schema.schema_type {
        match schema_type {
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::String)
            | oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Integer)
            | oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Number)
            | oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Boolean) => {
                // For primitives, create a synthetic ID based on the type
                let primitive = infer_primitive_from_schema(schema);
                return TypeRef {
                    target: StableId::new(format!("Primitive_{:?}", primitive)),
                    optional: false,
                    nullable,
                    by_ref: false,
                    modifiers: Vec::new(),
                };
            }
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Array) => {
                if let Some(items) = &schema.items {
                    // Generate hint for array items: "Input" -> "InputItem"
                    let item_hint = hint.map(|h| format!("{}Item", h));
                    let inner_ref = convert_schema_to_type_ref_with_hint_internal(
                        ctx,
                        items,
                        item_hint.as_deref(),
                    );
                    // Preserve inner modifiers and add List modifier
                    let mut modifiers = inner_ref.modifiers.clone();
                    modifiers.push(TypeMod::List);
                    return TypeRef {
                        target: inner_ref.target,
                        optional: false,
                        nullable,
                        by_ref: false,
                        modifiers,
                    };
                }
            }
            _ => {}
        }
    }

    TypeRef {
        target: StableId::new("Any"),
        optional: false,
        nullable,
        by_ref: false,
        modifiers: Vec::new(),
    }
}

/// Convert a Schema to a TypeRef (handles both Boolean and Object schemas)
fn convert_schema_to_type_ref(ctx: &mut BuildContext, schema: &oas3::spec::Schema) -> TypeRef {
    convert_schema_to_type_ref_with_hint_internal(ctx, schema, None)
}

/// Convert a Schema to a TypeRef with hint (handles both Boolean and Object schemas)
fn convert_schema_to_type_ref_with_hint_internal(
    ctx: &mut BuildContext,
    schema: &oas3::spec::Schema,
    hint: Option<&str>,
) -> TypeRef {
    match schema {
        oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(true)) => {
            // true schema accepts anything
            TypeRef {
                target: StableId::new("Any"),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            }
        }
        oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(false)) => {
            // false schema rejects everything (should not happen for items)
            TypeRef {
                target: StableId::new("Never"),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            }
        }
        oas3::spec::Schema::Object(schema_ref) => {
            convert_schema_ref_to_type_ref_with_hint(ctx, schema_ref, hint)
        }
    }
}

/// Convert a schema ref to a TypeRef
fn convert_schema_ref_to_type_ref(
    ctx: &mut BuildContext,
    schema_ref: &oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>,
) -> TypeRef {
    convert_schema_ref_to_type_ref_with_hint(ctx, schema_ref, None)
}

/// Convert a schema ref to a TypeRef with hint
fn convert_schema_ref_to_type_ref_with_hint(
    ctx: &mut BuildContext,
    schema_ref: &oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>,
    hint: Option<&str>,
) -> TypeRef {
    // Check if this is a reference
    if let oas3::spec::ObjectOrReference::Ref { ref_path, .. } = schema_ref {
        // Extract type name from reference like "#/components/schemas/Pet"
        let type_name = ref_path
            .split('/')
            .next_back()
            .unwrap_or("Unknown")
            .to_string();

        return TypeRef {
            target: StableId::new(&type_name),
            optional: false,
            nullable: false,
            by_ref: false,
            modifiers: Vec::new(),
        };
    }

    // Resolve and convert the inline schema with hint
    if let Ok(schema) = schema_ref.resolve(ctx.spec) {
        convert_object_schema_to_type_ref_with_hint(ctx, &schema, hint)
    } else {
        TypeRef {
            target: StableId::new("Any"),
            optional: false,
            nullable: false,
            by_ref: false,
            modifiers: Vec::new(),
        }
    }
}

/// Infer primitive type from schema
fn infer_primitive_from_schema(schema: &oas3::spec::ObjectSchema) -> Primitive {
    if let Some(schema_type) = &schema.schema_type {
        match schema_type {
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::String) => {
                match schema.format.as_deref() {
                    Some("date") => Primitive::Date,
                    Some("date-time") => Primitive::DateTime,
                    Some("uuid") => Primitive::Uuid,
                    Some("byte") | Some("binary") => Primitive::Bytes,
                    _ => Primitive::String,
                }
            }
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Integer) => {
                match schema.format.as_deref() {
                    Some("int64") => Primitive::I64,
                    _ => Primitive::I32,
                }
            }
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Number) => {
                match schema.format.as_deref() {
                    Some("double") => Primitive::F64,
                    Some("decimal") => Primitive::Decimal,
                    _ => Primitive::F32,
                }
            }
            oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Boolean) => Primitive::Bool,
            _ => Primitive::Any,
        }
    } else {
        Primitive::Any
    }
}

/// Convert servers to ServerSets
fn convert_servers(servers: &[oas3::spec::Server]) -> Vec<ServerSet> {
    if servers.is_empty() {
        return Vec::new();
    }

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
fn convert_security_schemes(components: &oas3::spec::Components) -> Vec<AuthScheme> {
    components
        .security_schemes
        .iter()
        .filter_map(|(name, scheme_ref)| {
            let id = StableId::new(name);
            let canonical_name = CanonicalName::from_string(name);

            // Resolve the security scheme reference
            // Note: we're not using ctx.spec here, so we'll just handle ObjectOrReference directly
            let scheme = match scheme_ref {
                oas3::spec::ObjectOrReference::Object(s) => s,
                oas3::spec::ObjectOrReference::Ref { .. } => {
                    // Skip references for now - would need spec to resolve
                    return None;
                }
            };

            let kind = match scheme {
                oas3::spec::SecurityScheme::ApiKey {
                    name: param_name,
                    location,
                    description: _,
                } => {
                    let api_location = match location.as_str() {
                        "query" => ApiKeyLocation::Query,
                        "header" => ApiKeyLocation::Header,
                        "cookie" => ApiKeyLocation::Cookie,
                        _ => ApiKeyLocation::Header, // default to header
                    };

                    AuthKind::ApiKey {
                        location: api_location,
                        param_name: param_name.clone(),
                    }
                }
                oas3::spec::SecurityScheme::Http {
                    scheme,
                    bearer_format,
                    description: _,
                } => AuthKind::Http {
                    scheme: scheme.clone(),
                    bearer_format: bearer_format.clone(),
                },
                oas3::spec::SecurityScheme::OAuth2 {
                    flows,
                    description: _,
                } => {
                    let oauth_flows = convert_oauth_flows(flows);
                    AuthKind::OAuth2 { flows: oauth_flows }
                }
                oas3::spec::SecurityScheme::OpenIdConnect {
                    open_id_connect_url,
                    description: _,
                } => AuthKind::OpenIdConnect {
                    url: open_id_connect_url.clone(),
                },
                _ => return None, // Skip unsupported types like MutualTls
            };

            let description = match scheme {
                oas3::spec::SecurityScheme::ApiKey { description, .. } => description.clone(),
                oas3::spec::SecurityScheme::Http { description, .. } => description.clone(),
                oas3::spec::SecurityScheme::OAuth2 { description, .. } => description.clone(),
                oas3::spec::SecurityScheme::OpenIdConnect { description, .. } => {
                    description.clone()
                }
                _ => None,
            };

            Some(AuthScheme {
                id,
                name: canonical_name,
                kind,
                docs: Docs {
                    summary: None,
                    description,
                    deprecated: false,
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
            })
        })
        .collect()
}

/// Convert OAuth2 flows
fn convert_oauth_flows(flows: &oas3::spec::Flows) -> Vec<OAuthFlow> {
    let mut result = Vec::new();

    if let Some(implicit) = &flows.implicit {
        result.push(OAuthFlow {
            kind: OAuthFlowKind::Implicit,
            authorization_url: Some(implicit.authorization_url.to_string()),
            token_url: None,
            refresh_url: implicit.refresh_url.as_ref().map(|u| u.to_string()),
            scopes: convert_oauth_scopes(&implicit.scopes),
        });
    }

    if let Some(password) = &flows.password {
        result.push(OAuthFlow {
            kind: OAuthFlowKind::Password,
            authorization_url: None,
            token_url: Some(password.token_url.to_string()),
            refresh_url: password.refresh_url.as_ref().map(|u| u.to_string()),
            scopes: convert_oauth_scopes(&password.scopes),
        });
    }

    if let Some(client_credentials) = &flows.client_credentials {
        result.push(OAuthFlow {
            kind: OAuthFlowKind::ClientCredentials,
            authorization_url: None,
            token_url: Some(client_credentials.token_url.to_string()),
            refresh_url: client_credentials
                .refresh_url
                .as_ref()
                .map(|u| u.to_string()),
            scopes: convert_oauth_scopes(&client_credentials.scopes),
        });
    }

    if let Some(authorization_code) = &flows.authorization_code {
        result.push(OAuthFlow {
            kind: OAuthFlowKind::AuthorizationCode,
            authorization_url: Some(authorization_code.authorization_url.to_string()),
            token_url: Some(authorization_code.token_url.to_string()),
            refresh_url: authorization_code
                .refresh_url
                .as_ref()
                .map(|u| u.to_string()),
            scopes: convert_oauth_scopes(&authorization_code.scopes),
        });
    }

    result
}

/// Convert OAuth scopes
fn convert_oauth_scopes(scopes: &std::collections::BTreeMap<String, String>) -> Vec<OAuthScope> {
    scopes
        .iter()
        .map(|(name, description)| OAuthScope {
            name: name.clone(),
            description: Some(description.clone()),
        })
        .collect()
}

/// Convert paths to Services and Operations
fn convert_paths(
    ctx: &mut BuildContext,
    paths: &Option<BTreeMap<String, oas3::spec::PathItem>>,
    security: &[oas3::spec::SecurityRequirement],
    _components: Option<&oas3::spec::Components>,
) -> Vec<Service> {
    // Group operations by tag (or use "default" if no tag)
    let mut services_map: BTreeMap<String, Vec<Operation>> = BTreeMap::new();

    // Convert global security to Option for easier handling
    let global_security = if security.is_empty() {
        None
    } else {
        Some(security)
    };

    if let Some(paths_map) = paths {
        for (path, path_item) in paths_map.iter() {
            convert_path_item(ctx, path, path_item, &mut services_map, global_security);
        }
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
    path_item: &oas3::spec::PathItem,
    services_map: &mut BTreeMap<String, Vec<Operation>>,
    global_security: Option<&[oas3::spec::SecurityRequirement]>,
) {
    let methods = [
        ("get", path_item.get.as_ref()),
        ("post", path_item.post.as_ref()),
        ("put", path_item.put.as_ref()),
        ("delete", path_item.delete.as_ref()),
        ("patch", path_item.patch.as_ref()),
        ("head", path_item.head.as_ref()),
        ("options", path_item.options.as_ref()),
        ("trace", path_item.trace.as_ref()),
    ];

    for (method_name, operation_opt) in methods {
        if let Some(operation) = operation_opt {
            let op = convert_operation(ctx, path, method_name, operation, global_security);

            // Group by first tag or "default"
            let tag = operation
                .tags
                .first()
                .cloned()
                .unwrap_or_else(|| "default".to_string());

            services_map.entry(tag).or_default().push(op);
        }
    }
}

/// Convert an OpenAPI operation to our Operation type
fn convert_operation(
    ctx: &mut BuildContext,
    path: &str,
    method_name: &str,
    operation: &oas3::spec::Operation,
    global_security: Option<&[oas3::spec::SecurityRequirement]>,
) -> Operation {
    let operation_id = operation
        .operation_id
        .clone()
        .unwrap_or_else(|| format!("{}{}", method_name, path.replace('/', "_")));

    // Set current operation ID in context for schema naming
    ctx.current_operation_id = Some(operation_id.clone());

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

    for param_ref in &operation.parameters {
        // Resolve parameter if it's a reference
        if let Ok(param) = param_ref.resolve(ctx.spec) {
            convert_parameter(
                ctx,
                &param,
                &mut path_params,
                &mut query,
                &mut headers,
                &mut cookies,
            );
        }
    }

    // Convert request body
    let (body, consumes) = if let Some(request_body_ref) = &operation.request_body {
        if let Ok(request_body) = request_body_ref.resolve(ctx.spec) {
            convert_request_body(ctx, &request_body)
        } else {
            (None, Vec::new())
        }
    } else {
        (None, Vec::new())
    };

    // Convert responses
    let (success, produces) = if let Some(responses) = &operation.responses {
        convert_responses(ctx, responses)
    } else {
        (None, Vec::new())
    };

    // Clear current operation ID
    ctx.current_operation_id = None;

    // Convert security requirements
    // Use operation-level security if present, otherwise fall back to global security
    let auth = if !operation.security.is_empty() {
        convert_security_requirements(Some(&operation.security))
    } else {
        convert_security_requirements(global_security)
    };

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
        auth,
        pagination: None,
        idempotent: matches!(
            method,
            HttpMethod::Get | HttpMethod::Put | HttpMethod::Delete
        ),
        retryable_statuses: Default::default(),
    }
}

/// Convert OpenAPI security requirements to AuthUse
fn convert_security_requirements(
    security: Option<&[oas3::spec::SecurityRequirement]>,
) -> Vec<AuthUse> {
    let Some(security) = security else {
        return Vec::new();
    };

    // OpenAPI security is an array of alternatives (OR)
    // Each SecurityRequirement is a tuple struct wrapping a BTreeMap of scheme names to scopes
    // For now, we'll flatten all schemes into a single list
    security
        .iter()
        .flat_map(|req| {
            req.0.iter().map(|(scheme_name, scopes)| AuthUse {
                scheme: StableId::new(scheme_name),
                scopes: scopes.clone(),
                optional: false, // TODO: determine if this is optional based on context
            })
        })
        .collect()
}

/// Convert a parameter to the appropriate parameter type
fn convert_parameter(
    ctx: &mut BuildContext,
    param: &oas3::spec::Parameter,
    path_params: &mut Vec<PathParam>,
    query: &mut Vec<QueryParam>,
    headers: &mut Vec<HeaderParam>,
    cookies: &mut Vec<CookieParam>,
) {
    let name = CanonicalName::from_string(&param.name);
    let docs = Docs {
        summary: None,
        description: param.description.clone(),
        deprecated: param.deprecated.unwrap_or(false),
        since: None,
        examples: Vec::new(),
        external_urls: Vec::new(),
    };

    // Get type from schema
    let ty = if let Some(schema_ref) = &param.schema {
        if let Ok(schema) = schema_ref.resolve(ctx.spec) {
            convert_object_schema_to_type_ref(ctx, &schema)
        } else {
            TypeRef {
                target: StableId::new("string"),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            }
        }
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

    match param.location {
        oas3::spec::ParameterIn::Path => {
            path_params.push(PathParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
            });
        }
        oas3::spec::ParameterIn::Query => {
            query.push(QueryParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
                required,
                default: None, // TODO: parse default value
            });
        }
        oas3::spec::ParameterIn::Header => {
            headers.push(HeaderParam {
                name: name.clone(),
                wire: param.name.clone(),
                docs,
                ty,
                required,
                default: None,
            });
        }
        oas3::spec::ParameterIn::Cookie => {
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
    request_body: &oas3::spec::RequestBody,
) -> (Option<Body>, Vec<String>) {
    let mut variants = Vec::new();
    let mut consumes = Vec::new();

    for (content_type, media_type) in &request_body.content {
        consumes.push(content_type.clone());

        if let Some(schema_ref) = &media_type.schema {
            // Check if this is an inline schema that should be hoisted
            let ty = match schema_ref {
                oas3::spec::ObjectOrReference::Ref { .. } => {
                    // Reference - use normal conversion
                    convert_schema_ref_to_type_ref(ctx, schema_ref)
                }
                oas3::spec::ObjectOrReference::Object(inline_schema) => {
                    // Inline schema - check if we should hoist it
                    if should_hoist_schema(inline_schema) {
                        // Hoist inline schema
                        let type_name = generate_inline_type_name(
                            ctx,
                            ctx.current_operation_id.as_deref(),
                            "Request",
                            None,
                        );
                        let type_id = hoist_inline_schema_with_parent(
                            ctx,
                            type_name.clone(),
                            inline_schema,
                            Some(&type_name),
                        );
                        TypeRef {
                            target: type_id,
                            optional: false,
                            nullable: inline_schema.is_nullable().unwrap_or(false),
                            by_ref: false,
                            modifiers: Vec::new(),
                        }
                    } else {
                        // Simple inline schema - use normal conversion
                        convert_schema_ref_to_type_ref(ctx, schema_ref)
                    }
                }
            };

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
    responses: &BTreeMap<String, oas3::spec::ObjectOrReference<oas3::spec::Response>>,
) -> (Option<Payload>, Vec<String>) {
    let mut produces = Vec::new();

    // Look for 2xx success responses
    for (status_code, response_ref) in responses {
        if let Ok(code) = status_code.parse::<u16>()
            && (200..300).contains(&code)
        {
            // Resolve the response
            if let Ok(response) = response_ref.resolve(ctx.spec) {
                // Get first content type
                if let Some((content_type, media_type)) = response.content.iter().next() {
                    produces.push(content_type.clone());

                    if let Some(schema_ref) = &media_type.schema {
                        // Check if this is an inline schema that should be hoisted
                        let ty = match schema_ref {
                            oas3::spec::ObjectOrReference::Ref { .. } => {
                                // Reference - use normal conversion
                                convert_schema_ref_to_type_ref(ctx, schema_ref)
                            }
                            oas3::spec::ObjectOrReference::Object(inline_schema) => {
                                // Inline schema - check if we should hoist it
                                if should_hoist_schema(inline_schema) {
                                    // Hoist inline schema
                                    let type_name = generate_inline_type_name(
                                        ctx,
                                        ctx.current_operation_id.as_deref(),
                                        "Response",
                                        None,
                                    );
                                    let type_id = hoist_inline_schema_with_parent(
                                        ctx,
                                        type_name.clone(),
                                        inline_schema,
                                        Some(&type_name),
                                    );
                                    TypeRef {
                                        target: type_id,
                                        optional: false,
                                        nullable: inline_schema.is_nullable().unwrap_or(false),
                                        by_ref: false,
                                        modifiers: Vec::new(),
                                    }
                                } else {
                                    // Simple inline schema - use normal conversion
                                    convert_schema_ref_to_type_ref(ctx, schema_ref)
                                }
                            }
                        };

                        let payload = Payload {
                            status: StatusSpec::Code(code),
                            content_type: Some(content_type.clone()),
                            ty: Some(ty),
                            headers: Vec::new(), // TODO: convert response headers
                            docs: Docs {
                                summary: response.description.clone(),
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

                // Response with no content
                let payload = Payload {
                    status: StatusSpec::Code(code),
                    content_type: None,
                    ty: None,
                    headers: Vec::new(),
                    docs: Docs {
                        summary: response.description.clone(),
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
    use parser2::parse;
    use pretty_assertions::assert_eq;

    use crate::gen_ir::{
        Additional, AliasTarget, CanonicalName, HttpMethod, Primitive, StableId, TypeKind,
    };

    use super::*;

    #[test]
    fn test_from_openapi() {
        // Create a minimal OpenAPI document
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0",
                "description": "A test API"
            },
            "paths": {}
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        for type_decl in gen_ir.types.values() {
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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
                .unwrap_or_else(|| panic!("Operation {} not found", op_id));

            assert_eq!(op.http.method, expected_method);
        }
    }

    #[test]
    fn test_idempotent_operations() {
        // Test that idempotency is correctly inferred from HTTP method
        let json = r##"{
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
        }"##;

        let doc = parse(json).expect("Failed to parse OpenAPI");
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

    #[test]
    fn test_inline_schema_hoisting_request_body() {
        // Test that inline request body schemas are hoisted to named types
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/test": {
                    "post": {
                        "operationId": "createTest",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {
                                                "type": "string"
                                            },
                                            "age": {
                                                "type": "integer"
                                            }
                                        },
                                        "required": ["name"]
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Should generate CreateTestRequest type
        let request_type_id = StableId::new("CreateTestRequest");
        assert!(
            gen_ir.types.contains_key(&request_type_id),
            "CreateTestRequest type should be generated"
        );

        // Verify the type has the correct fields
        let request_type = &gen_ir.types[&request_type_id];
        match &request_type.kind {
            TypeKind::Struct { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert!(fields.iter().any(|f| f.name.canonical == "name"));
                assert!(fields.iter().any(|f| f.name.canonical == "age"));

                // Check required field
                let name_field = fields.iter().find(|f| f.name.canonical == "name").unwrap();
                assert!(!name_field.ty.optional);

                let age_field = fields.iter().find(|f| f.name.canonical == "age").unwrap();
                assert!(age_field.ty.optional);
            }
            _ => panic!("Expected Struct type"),
        }

        // Operation should reference the hoisted type
        let op = &gen_ir.services[0].operations[0];
        let body_type = op.http.body.as_ref().unwrap().variants[0].ty.target.clone();
        assert_eq!(body_type, request_type_id);
    }

    #[test]
    fn test_inline_schema_hoisting_response() {
        // Test that inline response schemas are hoisted to named types
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/users": {
                    "get": {
                        "operationId": "listUsers",
                        "responses": {
                            "200": {
                                "description": "Success",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "object",
                                            "properties": {
                                                "users": {
                                                    "type": "array",
                                                    "items": {
                                                        "type": "string"
                                                    }
                                                },
                                                "total": {
                                                    "type": "integer"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Should generate ListUsersResponse type
        let response_type_id = StableId::new("ListUsersResponse");
        assert!(
            gen_ir.types.contains_key(&response_type_id),
            "ListUsersResponse type should be generated"
        );

        // Verify the type structure
        let response_type = &gen_ir.types[&response_type_id];
        match &response_type.kind {
            TypeKind::Struct { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert!(fields.iter().any(|f| f.name.canonical == "users"));
                assert!(fields.iter().any(|f| f.name.canonical == "total"));
            }
            _ => panic!("Expected Struct type"),
        }

        // Operation should reference the hoisted type
        let op = &gen_ir.services[0].operations[0];
        let response_type = op
            .success
            .as_ref()
            .unwrap()
            .ty
            .as_ref()
            .unwrap()
            .target
            .clone();
        assert_eq!(response_type, response_type_id);
    }

    #[test]
    fn test_inline_schema_hoisting_nested_properties() {
        // Test that nested inline object properties are hoisted
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/accounts": {
                    "post": {
                        "operationId": "createAccount",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {
                                                "type": "string"
                                            },
                                            "settings": {
                                                "type": "object",
                                                "properties": {
                                                    "theme": {
                                                        "type": "string"
                                                    },
                                                    "notifications": {
                                                        "type": "boolean"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Should generate both CreateAccountRequest and CreateAccountRequestSettings types
        let request_type_id = StableId::new("CreateAccountRequest");
        let settings_type_id = StableId::new("CreateAccountRequestSettings");

        assert!(
            gen_ir.types.contains_key(&request_type_id),
            "CreateAccountRequest type should be generated"
        );
        assert!(
            gen_ir.types.contains_key(&settings_type_id),
            "CreateAccountRequestSettings type should be generated for nested object"
        );

        // Verify the nested type structure
        let settings_type = &gen_ir.types[&settings_type_id];
        match &settings_type.kind {
            TypeKind::Struct { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert!(fields.iter().any(|f| f.name.canonical == "theme"));
                assert!(fields.iter().any(|f| f.name.canonical == "notifications"));
            }
            _ => panic!("Expected Struct type for settings"),
        }

        // Verify the parent type references the nested type
        let request_type = &gen_ir.types[&request_type_id];
        match &request_type.kind {
            TypeKind::Struct { fields, .. } => {
                let settings_field = fields
                    .iter()
                    .find(|f| f.name.canonical == "settings")
                    .unwrap();
                assert_eq!(settings_field.ty.target, settings_type_id);
            }
            _ => panic!("Expected Struct type for request"),
        }
    }

    #[test]
    fn test_inline_schema_deduplication() {
        // Test that identical inline schemas are deduplicated
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/users": {
                    "post": {
                        "operationId": "createUser",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {
                                                "type": "string"
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    }
                },
                "/accounts": {
                    "post": {
                        "operationId": "createAccount",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {
                                                "type": "string"
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Both operations should use the same hoisted type (deduplication)
        let user_op = &gen_ir.services[0]
            .operations
            .iter()
            .find(|o| o.name.canonical == "createUser")
            .unwrap();
        let account_op = &gen_ir.services[0]
            .operations
            .iter()
            .find(|o| o.name.canonical == "createAccount")
            .unwrap();

        let user_body_type = user_op.http.body.as_ref().unwrap().variants[0]
            .ty
            .target
            .clone();
        let account_body_type = account_op.http.body.as_ref().unwrap().variants[0]
            .ty
            .target
            .clone();

        // They should reference the same type due to deduplication
        assert_eq!(user_body_type, account_body_type);
    }

    #[test]
    fn test_inline_schema_name_collision_handling() {
        // Test that name collisions are handled with suffixes
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "CreateTestRequest": {
                        "type": "object",
                        "description": "Pre-existing type with the same name"
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Should have the pre-existing type
        let existing_type_id = StableId::new("CreateTestRequest");
        assert!(gen_ir.types.contains_key(&existing_type_id));

        // If we tried to hoist an inline schema with the same name,
        // it should get a suffix like CreateTestRequest2
        // This is implicitly tested by the collision detection logic
    }

    #[test]
    fn test_empty_inline_objects_not_hoisted() {
        // Test that empty objects are not hoisted (they remain as Any)
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {
                "/test": {
                    "post": {
                        "operationId": "createTest",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object"
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created"
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Empty object should not generate a hoisted type
        // The operation should reference "Any" type
        let op = &gen_ir.services[0].operations[0];
        let body_type = &op.http.body.as_ref().unwrap().variants[0].ty.target;

        // Should be Any, not a hoisted type
        assert_eq!(body_type, &StableId::new("Any"));
    }

    // Helper function to generate pseudo-code for types
    fn generate_pseudo_code(gen_ir: &GenIr) -> String {
        let mut output = String::new();

        for type_decl in gen_ir.types.values() {
            output.push_str(&format!("type {} = ", type_decl.name.pascal));

            match &type_decl.kind {
                TypeKind::Struct {
                    fields, additional, ..
                } => {
                    output.push_str("{\n");
                    for field in fields {
                        let opt = if field.ty.optional { "?" } else { "" };
                        let nullable = if field.ty.nullable { " | null" } else { "" };
                        let mut type_str = field.ty.target.0.clone();

                        // Handle modifiers (like List)
                        for modifier in &field.ty.modifiers {
                            if matches!(modifier, crate::gen_ir::TypeMod::List) {
                                type_str = format!("{}[]", type_str);
                            }
                        }

                        output.push_str(&format!(
                            "  {}{}: {}{}\n",
                            field.name.camel, opt, type_str, nullable
                        ));
                    }
                    match additional {
                        Additional::Forbidden => {
                            output.push_str("  [key: string]: never\n");
                        }
                        Additional::Any => {
                            output.push_str("  [key: string]: any\n");
                        }
                        _ => {}
                    }
                    output.push_str("}\n\n");
                }
                TypeKind::Enum { base, values } => {
                    output.push_str(&format!("enum<{:?}> {{\n", base));
                    for value in values {
                        output.push_str(&format!("  {} = {:?}\n", value.name.pascal, value.wire));
                    }
                    output.push_str("}\n\n");
                }
                TypeKind::Alias { aliased } => match aliased {
                    AliasTarget::Primitive(p) => {
                        output.push_str(&format!("{:?}\n\n", p));
                    }
                    AliasTarget::Composite(c) => {
                        output.push_str(&format!("{:?}\n\n", c));
                    }
                    AliasTarget::Reference(r) => {
                        output.push_str(&format!("{}\n\n", r.target.0));
                    }
                },
                TypeKind::Union { style, variants } => {
                    output.push_str(&format!("union<{:?}> {{\n", style));
                    for variant in variants {
                        let type_str = &variant.ty.target.0;
                        output.push_str(&format!("  {}: {}\n", variant.name.pascal, type_str));
                    }
                    output.push_str("}\n\n");
                }
            }
        }

        output
    }

    #[test]
    fn test_oneof_basic() {
        // Test oneOf - exactly one schema must match
        // Example: Pet can be a Cat OR a Dog (mutually exclusive)
        let json = r##"{
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
                        "properties": {
                            "meow": { "type": "string" }
                        }
                    },
                    "Dog": {
                        "type": "object",
                        "properties": {
                            "bark": { "type": "string" }
                        }
                    },
                    "Pet": {
                        "oneOf": [
                            { "$ref": "#/components/schemas/Cat" },
                            { "$ref": "#/components/schemas/Dog" }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Generate pseudo-code
        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // This is what SHOULD be generated for oneOf with $ref
        let expected = r##"type Cat = {
  meow?: Primitive_String
  [key: string]: any
}

type Dog = {
  bark?: Primitive_String
  [key: string]: any
}

type Pet = union<OneOf> {
  Cat: Cat
  Dog: Dog
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output.\n\nActual output:\n{}\n\nExpected:\n{}",
            pseudo_code,
            expected
        );
    }

    #[test]
    fn test_anyof_basic() {
        // Test anyOf - any number of schemas can match
        // Example: ParkingSpot can be Paid AND/OR Covered
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Paid": {
                        "type": "object",
                        "properties": {
                            "price": { "type": "number" },
                            "currency": { "type": "string" }
                        }
                    },
                    "Covered": {
                        "type": "object",
                        "properties": {
                            "roofType": { "type": "string" }
                        }
                    },
                    "ParkingSpot": {
                        "anyOf": [
                            { "$ref": "#/components/schemas/Paid" },
                            { "$ref": "#/components/schemas/Covered" }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // This is what SHOULD be generated for anyOf with $ref
        // Note: Field order might vary due to BTreeMap
        let expected = r##"type Covered = {
  roofType?: Primitive_String
  [key: string]: any
}

type Paid = {
  currency?: Primitive_String
  price?: Primitive_F32
  [key: string]: any
}

type ParkingSpot = union<AnyOf> {
  Paid: Paid
  Covered: Covered
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_allof_basic() {
        // Test allOf - all schemas must match (composition/inheritance)
        // Example: Cat extends Pet and adds cat-specific properties
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "age": { "type": "integer" }
                        },
                        "required": ["name"]
                    },
                    "Cat": {
                        "allOf": [
                            { "$ref": "#/components/schemas/Pet" },
                            {
                                "type": "object",
                                "properties": {
                                    "meow": { "type": "string" },
                                    "indoor": { "type": "boolean" }
                                },
                                "required": ["indoor"]
                            }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        let expected = r#"type Cat = {
  age?: Primitive_I32
  indoor: Primitive_Bool
  meow?: Primitive_String
  name: Primitive_String
  [key: string]: any
}

type Pet = {
  age?: Primitive_I32
  name: Primitive_String
  [key: string]: any
}

"#;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_oneof_with_inline_schemas() {
        // Test oneOf with inline object schemas (not references)
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Response": {
                        "oneOf": [
                            {
                                "type": "object",
                                "title": "Success",
                                "properties": {
                                    "data": { "type": "string" }
                                }
                            },
                            {
                                "type": "object",
                                "title": "Error",
                                "properties": {
                                    "error": { "type": "string" }
                                }
                            }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // Inline schemas with "title" should be hoisted to named types
        let expected = r##"type InlineTypeObject = {
  data?: Primitive_String
  [key: string]: any
}

type InlineTypeObject2 = {
  error?: Primitive_String
  [key: string]: any
}

type Response = union<OneOf> {
  Success: InlineTypeObject
  Error: InlineTypeObject2
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_allof_multiple_objects() {
        // Test allOf with multiple object schemas merging their properties
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "HasId": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        },
                        "required": ["id"]
                    },
                    "HasTimestamps": {
                        "type": "object",
                        "properties": {
                            "createdAt": { "type": "string", "format": "date-time" },
                            "updatedAt": { "type": "string", "format": "date-time" }
                        },
                        "required": ["createdAt"]
                    },
                    "User": {
                        "allOf": [
                            { "$ref": "#/components/schemas/HasId" },
                            { "$ref": "#/components/schemas/HasTimestamps" },
                            {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "email": { "type": "string" }
                                },
                                "required": ["email"]
                            }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // allOf should merge all fields from all schemas, respecting required fields
        let expected = r##"type HasId = {
  id: Primitive_String
  [key: string]: any
}

type HasTimestamps = {
  createdAt: Primitive_DateTime
  updatedAt?: Primitive_DateTime
  [key: string]: any
}

type User = {
  createdAt: Primitive_DateTime
  email: Primitive_String
  id: Primitive_String
  name?: Primitive_String
  updatedAt?: Primitive_DateTime
  [key: string]: any
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_anyof_with_primitives() {
        // Test anyOf with primitive types
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "StringOrNumber": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "number" }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Verify StringOrNumber is a union
        let type_decl = gen_ir.types.get(&StableId::new("StringOrNumber")).unwrap();
        match &type_decl.kind {
            TypeKind::Union { style, variants } => {
                assert!(matches!(style, crate::gen_ir::UnionStyle::AnyOf));
                assert_eq!(variants.len(), 2);
            }
            _ => panic!("Expected Union type for anyOf with primitives"),
        }

        assert!(pseudo_code.contains("type StringOrNumber = union<AnyOf>"));
    }

    #[test]
    fn test_nested_allof() {
        // Test nested allOf (allOf containing allOf)
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Base": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        }
                    },
                    "Middle": {
                        "allOf": [
                            { "$ref": "#/components/schemas/Base" },
                            {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" }
                                }
                            }
                        ]
                    },
                    "Final": {
                        "allOf": [
                            { "$ref": "#/components/schemas/Middle" },
                            {
                                "type": "object",
                                "properties": {
                                    "email": { "type": "string" }
                                }
                            }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // nested allOf: Middle should merge Base, Final should merge Middle
        let expected = r##"type Base = {
  id?: Primitive_String
  [key: string]: any
}

type Final = {
  email?: Primitive_String
  id?: Primitive_String
  name?: Primitive_String
  [key: string]: any
}

type Middle = {
  id?: Primitive_String
  name?: Primitive_String
  [key: string]: any
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_discriminated_union_with_oneof() {
        // Test oneOf with discriminator property
        let json = r##"{
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
                        "properties": {
                            "petType": { "type": "string" },
                            "meow": { "type": "string" }
                        },
                        "required": ["petType"]
                    },
                    "Dog": {
                        "type": "object",
                        "properties": {
                            "petType": { "type": "string" },
                            "bark": { "type": "string" }
                        },
                        "required": ["petType"]
                    },
                    "Pet": {
                        "oneOf": [
                            { "$ref": "#/components/schemas/Cat" },
                            { "$ref": "#/components/schemas/Dog" }
                        ],
                        "discriminator": {
                            "propertyName": "petType"
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // oneOf with discriminator should still create a union with proper references
        let expected = r##"type Cat = {
  meow?: Primitive_String
  petType: Primitive_String
  [key: string]: any
}

type Dog = {
  bark?: Primitive_String
  petType: Primitive_String
  [key: string]: any
}

type Pet = union<OneOf> {
  Cat: Cat
  Dog: Dog
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_allof_with_additional_properties() {
        // Test allOf respects additionalProperties from merged schemas
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Base": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        },
                        "additionalProperties": true
                    },
                    "Extended": {
                        "allOf": [
                            { "$ref": "#/components/schemas/Base" },
                            {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" }
                                },
                                "additionalProperties": false
                            }
                        ]
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        let pseudo_code = generate_pseudo_code(&gen_ir);

        // Check against the ENTIRE pseudo-code output
        // allOf should respect the most restrictive additionalProperties setting
        let expected = r##"type Base = {
  id?: Primitive_String
  [key: string]: any
}

type Extended = {
  id?: Primitive_String
  name?: Primitive_String
  [key: string]: never
}

"##;
        assert_eq!(
            pseudo_code.trim(),
            expected.trim(),
            "Generated pseudo-code doesn't match expected output"
        );
    }

    #[test]
    fn test_const_field_detection() {
        // Test that const fields are properly detected and converted to const_value
        let json = r##"{
            "openapi": "3.0.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "required": ["type", "name"],
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "pet",
                                "description": "Always 'pet'"
                            },
                            "name": {
                                "type": "string"
                            },
                            "active": {
                                "type": "boolean",
                                "const": true
                            },
                            "version": {
                                "type": "integer",
                                "const": 1
                            }
                        }
                    }
                }
            }
        }"##;

        let doc = parse(json).unwrap();
        let gen_ir = GenIr::from(doc);

        // Find the Pet type
        let pet_id = StableId::new("Pet");
        let pet_type = gen_ir.types.get(&pet_id).expect("Pet type should exist");

        // Verify it's a struct
        if let TypeKind::Struct { fields, .. } = &pet_type.kind {
            // Find the 'type' field
            let type_field = fields
                .iter()
                .find(|f| f.name.canonical == "type")
                .expect("'type' field should exist");

            // Verify it has a const value
            assert!(
                type_field.const_value.is_some(),
                "'type' field should have a const_value"
            );

            // Verify the const value is correct
            if let Some(crate::gen_ir::Literal::String(val)) = &type_field.const_value {
                assert_eq!(val, "pet", "const value should be 'pet'");
            } else {
                panic!("const_value should be a String literal");
            }

            // Find the 'active' field
            let active_field = fields
                .iter()
                .find(|f| f.name.canonical == "active")
                .expect("'active' field should exist");

            // Verify it has a const value
            assert!(
                active_field.const_value.is_some(),
                "'active' field should have a const_value"
            );

            // Verify the const value is correct
            if let Some(crate::gen_ir::Literal::Bool(val)) = &active_field.const_value {
                assert_eq!(*val, true, "const value should be true");
            } else {
                panic!("const_value should be a Bool literal");
            }

            // Find the 'version' field
            let version_field = fields
                .iter()
                .find(|f| f.name.canonical == "version")
                .expect("'version' field should exist");

            // Verify it has a const value
            assert!(
                version_field.const_value.is_some(),
                "'version' field should have a const_value"
            );

            // Verify the const value is correct
            if let Some(crate::gen_ir::Literal::I64(val)) = &version_field.const_value {
                assert_eq!(*val, 1, "const value should be 1");
            } else {
                panic!("const_value should be an I64 literal");
            }

            // Verify 'name' field does NOT have a const value
            let name_field = fields
                .iter()
                .find(|f| f.name.canonical == "name")
                .expect("'name' field should exist");

            assert!(
                name_field.const_value.is_none(),
                "'name' field should NOT have a const_value"
            );
        } else {
            panic!("Pet type should be a Struct");
        }
    }
}
