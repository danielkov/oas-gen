//! TypeScript SDK generator.
//!
//! This crate implements a TypeScript generator that demonstrates
//! the hybrid approach: programmatic file structure with template-based rendering.

use askama::Template;
use codegen::{Config, Error, GenIr, Generator, Result, VirtualFS};
use ir::gen_ir::{TypeDecl, TypeKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Registry for tracking and rendering imports.
///
/// Collects imports via `register()` and renders them sorted:
/// - Third-party imports first (alphabetically by source)
/// - Empty line
/// - Local imports (alphabetically by source)
#[derive(Default)]
struct ImportRegistry {
    /// source -> (type_only_names, value_names)
    imports: BTreeMap<String, (Vec<String>, Vec<String>)>,
}

impl ImportRegistry {
    fn new() -> Self {
        Self::default()
    }

    /// Register an import.
    ///
    /// For local imports (starting with `./`), this registers both the type
    /// and its schema. For third-party imports, just the name is registered.
    fn register(&mut self, name: &str, source: &str) {
        let entry = self.imports.entry(source.to_string()).or_default();

        if source.starts_with("./") || source.starts_with("../") {
            // Local import - add type and schema
            if !entry.0.contains(&name.to_string()) {
                entry.0.push(name.to_string());
            }
            let schema_name = format!("{}Schema", name);
            if !entry.1.contains(&schema_name) {
                entry.1.push(schema_name);
            }
        } else {
            // Third-party - just value import
            if !entry.1.contains(&name.to_string()) {
                entry.1.push(name.to_string());
            }
        }
    }

    /// Render all imports as a string.
    ///
    /// Output is sorted: third-party first (alphabetically), empty line,
    /// then local imports (alphabetically).
    fn render(&self) -> String {
        let mut lines = Vec::new();

        // Separate third-party from local
        let mut third_party: Vec<_> = self
            .imports
            .iter()
            .filter(|(source, _)| !source.starts_with("./") && !source.starts_with("../"))
            .collect();
        let mut local: Vec<_> = self
            .imports
            .iter()
            .filter(|(source, _)| source.starts_with("./") || source.starts_with("../"))
            .collect();

        // Sort by source
        third_party.sort_by_key(|(source, _)| *source);
        local.sort_by_key(|(source, _)| *source);

        // Third-party imports
        for (source, (type_names, value_names)) in &third_party {
            if !value_names.is_empty() {
                let mut sorted_names = value_names.clone();
                sorted_names.sort();
                lines.push(format!(
                    "import {{ {} }} from '{}';",
                    sorted_names.join(", "),
                    source
                ));
            }
            if !type_names.is_empty() {
                let mut sorted_names = type_names.clone();
                sorted_names.sort();
                lines.push(format!(
                    "import type {{ {} }} from '{}';",
                    sorted_names.join(", "),
                    source
                ));
            }
        }

        // Empty line between sections
        if !third_party.is_empty() && !local.is_empty() {
            lines.push(String::new());
        }

        // Local imports
        for (source, (type_names, value_names)) in &local {
            if !type_names.is_empty() {
                let mut sorted_names = type_names.clone();
                sorted_names.sort();
                lines.push(format!(
                    "import type {{ {} }} from '{}';",
                    sorted_names.join(", "),
                    source
                ));
            }
            if !value_names.is_empty() {
                let mut sorted_names = value_names.clone();
                sorted_names.sort();
                lines.push(format!(
                    "import {{ {} }} from '{}';",
                    sorted_names.join(", "),
                    source
                ));
            }
        }

        lines.join("\n")
    }
}

/// TypeScript SDK generator.
pub struct TypeScriptGenerator;

impl TypeScriptGenerator {
    /// Create a new TypeScript generator.
    pub fn new() -> Self {
        Self
    }

    /// Generate types into the virtual file system.
    fn generate_types(&self, ir: &GenIr, _config: &Config, vfs: &mut VirtualFS) -> Result<()> {
        let types_dir = PathBuf::from("src").join("types");

        // Generate individual type files
        let mut type_names = Vec::new();

        for type_decl in ir.types.values() {
            let rendered = self.render_type(type_decl, ir)?;
            let file_name = format!("{}.ts", type_decl.name.pascal);
            vfs.add_file(types_dir.join(&file_name), rendered);
            type_names.push(type_decl.name.pascal.clone());
        }

        // Sort for deterministic output
        type_names.sort();

        // Generate index.ts that re-exports all types
        let exports: Vec<String> = type_names
            .iter()
            .map(|name| {
                format!(
                    "export * from './{}';\nexport {{ {}Schema }} from './{}';",
                    name, name, name
                )
            })
            .collect();

        let index_content = format!(
            "// Generated types from OpenAPI specification\n\nexport * from './errors';\n{}\n",
            exports.join("\n")
        );

        vfs.add_file(types_dir.join("index.ts"), index_content);

        // Generate error classes
        self.generate_errors(ir, vfs)?;

        Ok(())
    }

    /// Generate base error classes for API error handling.
    fn generate_errors(&self, _ir: &GenIr, vfs: &mut VirtualFS) -> Result<()> {
        let types_dir = PathBuf::from("src").join("types");

        let data = ErrorsTemplate;
        let content = data
            .render()
            .map_err(|e| Error::TemplateError(Box::new(e)))?;
        vfs.add_file(types_dir.join("errors.ts"), content);

        Ok(())
    }

    /// Render a single type declaration.
    fn render_type(&self, type_decl: &TypeDecl, ir: &GenIr) -> Result<String> {
        match &type_decl.kind {
            TypeKind::Struct { fields, .. } => {
                let mut imports = ImportRegistry::new();

                // Register base imports needed for all structs
                imports.register("object", "@speakeasy-api/tonic");
                imports.register("typed", "@speakeasy-api/tonic");
                imports.register("TypedSchema", "@speakeasy-api/tonic");

                // Track if we need optional
                let has_optional_fields = fields.iter().any(|f| f.ty.optional);
                if has_optional_fields {
                    imports.register("optional", "@speakeasy-api/tonic");
                }

                let field_data: Vec<FieldData> = fields
                    .iter()
                    .map(|f| {
                        // If there's a const value, use it as a literal type
                        let type_str = if let Some(const_val) = &f.const_value {
                            self.render_literal(const_val)
                        } else {
                            self.render_type_ref(&f.ty, ir)
                        };

                        // Generate schema expression
                        let schema_expr = if let Some(const_val) = &f.const_value {
                            self.render_literal_schema(const_val, &mut imports)
                        } else {
                            self.render_schema_for_type_ref(&f.ty, ir, &mut imports)
                        };

                        // Use getter for fields that reference other types AND are optional
                        // This is needed because tonic's .optional() doesn't work on imported schema refs
                        let references_other_type = self.field_references_custom_type(&f.ty, ir);
                        let use_getter = references_other_type && f.ty.optional;

                        FieldData {
                            name: &f.name.camel,
                            optional: f.ty.optional,
                            type_str,
                            schema_expr,
                            use_getter,
                            docs: &f.docs,
                        }
                    })
                    .collect();

                let data = InterfaceTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    fields: field_data,
                    imports: imports.render(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Enum { values, base: _ } => {
                let mut imports = ImportRegistry::new();
                imports.register("typed", "@speakeasy-api/tonic");
                imports.register("TypedSchema", "@speakeasy-api/tonic");
                imports.register("literal", "@speakeasy-api/tonic");
                imports.register("union", "@speakeasy-api/tonic");

                let data = EnumTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    values: values
                        .iter()
                        .map(|v| EnumValueData {
                            name: v.name.upper.clone(),
                            value: self.render_literal(&v.wire),
                        })
                        .collect(),
                    imports: imports.render(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Union { variants, .. } => {
                let mut imports = ImportRegistry::new();

                // Always use typed<T> pattern
                imports.register("typed", "@speakeasy-api/tonic");
                imports.register("TypedSchema", "@speakeasy-api/tonic");
                imports.register("union", "@speakeasy-api/tonic");

                // Render union as type alias with union types
                let type_names: Vec<String> = variants
                    .iter()
                    .map(|v| self.render_type_ref(&v.ty, ir))
                    .collect();

                // Generate schema expressions for each variant
                let schema_variants: Vec<String> = variants
                    .iter()
                    .map(|v| self.render_schema_for_type_ref(&v.ty, ir, &mut imports))
                    .collect();

                let schema_target = format!("union({})", schema_variants.join(", "));

                let data = TypeAliasTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    target: type_names.join(" | "),
                    schema_target,
                    imports: imports.render(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Alias { aliased } => {
                let mut imports = ImportRegistry::new();

                // Always use typed<T> pattern
                imports.register("typed", "@speakeasy-api/tonic");
                imports.register("TypedSchema", "@speakeasy-api/tonic");

                let target = self.render_alias_target(aliased, ir);
                let schema_target = self.render_alias_schema(aliased, ir, &mut imports);

                let data = TypeAliasTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    target,
                    schema_target,
                    imports: imports.render(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
        }
    }

    /// Render a primitive type as TypeScript type.
    fn render_primitive(&self, primitive: ir::gen_ir::Primitive) -> String {
        use ir::gen_ir::Primitive;
        match primitive {
            Primitive::Any => "any".to_string(),
            Primitive::Bool => "boolean".to_string(),
            Primitive::I32 | Primitive::I64 | Primitive::F32 | Primitive::F64 => {
                "number".to_string()
            }
            Primitive::String => "string".to_string(),
            Primitive::Date | Primitive::DateTime => "Date".to_string(),
            Primitive::Uuid => "string".to_string(),
            Primitive::Bytes => "Uint8Array".to_string(),
            Primitive::Decimal => "number".to_string(),
        }
    }

    /// Render a literal as a TypeScript literal.
    fn render_literal(&self, literal: &ir::gen_ir::Literal) -> String {
        use ir::gen_ir::Literal;
        match literal {
            Literal::Null => "null".to_string(),
            Literal::Bool(b) => b.to_string(),
            Literal::I64(i) => i.to_string(),
            Literal::F64(f) => f.to_string(),
            Literal::String(s) => format!("\"{}\"", s.replace('\"', "\\\"")),
            Literal::Array(_) | Literal::Object(_) => {
                // For complex literals, just use string representation
                "null".to_string()
            }
        }
    }

    /// Render an alias target.
    fn render_alias_target(&self, target: &ir::gen_ir::AliasTarget, ir: &GenIr) -> String {
        use ir::gen_ir::AliasTarget;
        match target {
            AliasTarget::Primitive(p) => self.render_primitive(*p),
            AliasTarget::Composite(c) => self.render_composite(c, ir),
            AliasTarget::Reference(type_ref) => self.render_type_ref(type_ref, ir),
        }
    }

    /// Render an alias target as a tonic schema expression.
    fn render_alias_schema(
        &self,
        target: &ir::gen_ir::AliasTarget,
        ir: &GenIr,
        imports: &mut ImportRegistry,
    ) -> String {
        use ir::gen_ir::AliasTarget;
        match target {
            AliasTarget::Primitive(p) => {
                let (schema, import) = self.render_primitive_schema(*p);
                if let Some(import) = import {
                    imports.register(&import, "@speakeasy-api/tonic");
                }
                schema
            }
            AliasTarget::Composite(c) => self.render_composite_schema(c, ir, imports),
            AliasTarget::Reference(type_ref) => {
                self.render_schema_for_type_ref(type_ref, ir, imports)
            }
        }
    }

    /// Render a composite type as a tonic schema expression.
    fn render_composite_schema(
        &self,
        composite: &ir::gen_ir::Composite,
        ir: &GenIr,
        imports: &mut ImportRegistry,
    ) -> String {
        use ir::gen_ir::Composite;
        match composite {
            Composite::List(inner) => {
                imports.register("array", "@speakeasy-api/tonic");
                format!(
                    "array({})",
                    self.render_schema_for_type_ref(inner, ir, imports)
                )
            }
            Composite::Map { value, .. } => {
                imports.register("record", "@speakeasy-api/tonic");
                format!(
                    "record({})",
                    self.render_schema_for_type_ref(value, ir, imports)
                )
            }
            Composite::Tuple(types) => {
                imports.register("tuple", "@speakeasy-api/tonic");
                let rendered: Vec<String> = types
                    .iter()
                    .map(|t| self.render_schema_for_type_ref(t, ir, imports))
                    .collect();
                format!("tuple([{}])", rendered.join(", "))
            }
        }
    }

    /// Render a composite type.
    fn render_composite(&self, composite: &ir::gen_ir::Composite, ir: &GenIr) -> String {
        use ir::gen_ir::Composite;
        match composite {
            Composite::List(inner) => format!("Array<{}>", self.render_type_ref(inner, ir)),
            Composite::Map { value, .. } => {
                format!("Record<string, {}>", self.render_type_ref(value, ir))
            }
            Composite::Tuple(types) => {
                let rendered: Vec<String> =
                    types.iter().map(|t| self.render_type_ref(t, ir)).collect();
                format!("[{}]", rendered.join(", "))
            }
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn render_type_ref(&self, type_ref: &ir::gen_ir::TypeRef, ir: &GenIr) -> String {
        // Get the base type name
        let base = if let Some(type_decl) = ir.types.get(&type_ref.target) {
            type_decl.name.pascal.clone()
        } else {
            match &type_ref.target {
                ir::gen_ir::StableId::Primitive(p) => {
                    use ir::gen_ir::Primitive;
                    match p {
                        Primitive::String
                        | Primitive::Uuid
                        | Primitive::Date
                        | Primitive::DateTime
                        | Primitive::Bytes
                        | Primitive::Decimal => "string".to_string(),
                        Primitive::Bool => "boolean".to_string(),
                        Primitive::I32 | Primitive::I64 | Primitive::F32 | Primitive::F64 => {
                            "number".to_string()
                        }
                        Primitive::Any => "any".to_string(),
                    }
                }
                ir::gen_ir::StableId::Named(_) => {
                    // Can't find the type - return any
                    "any".to_string()
                }
            }
        };

        let mut result = base;

        // Apply type modifiers (collections, maps, etc.)
        for modifier in &type_ref.modifiers {
            result = match modifier {
                ir::gen_ir::TypeMod::List => format!("Array<{}>", result),
                ir::gen_ir::TypeMod::Set => format!("Set<{}>", result),
                ir::gen_ir::TypeMod::Map(value_type) => {
                    // Recursively render the value type for Map<K, V>
                    // TypeScript uses Record<string, V> for string-keyed maps
                    format!("Record<string, {}>", self.render_type_ref(value_type, ir))
                }
                _ => result,
            };
        }

        // Add null union if nullable
        if type_ref.nullable {
            result = format!("{} | null", result);
        }

        // Note: optional is not added to the type string itself
        // It's handled at the field/parameter level with the `?` syntax
        // e.g., `field?: string` instead of `field: string | undefined`

        result
    }

    /// Render a primitive as a tonic schema expression.
    /// Returns (schema_expr, Option<tonic_import>)
    fn render_primitive_schema(
        &self,
        primitive: ir::gen_ir::Primitive,
    ) -> (String, Option<String>) {
        use ir::gen_ir::Primitive;
        match primitive {
            // unknown() is provided by tonic
            Primitive::Any => ("unknown()".to_string(), Some("unknown".to_string())),
            Primitive::Bool => ("boolean()".to_string(), Some("boolean".to_string())),
            Primitive::I32
            | Primitive::I64
            | Primitive::F32
            | Primitive::F64
            | Primitive::Decimal => ("number()".to_string(), Some("number".to_string())),
            Primitive::String | Primitive::Uuid | Primitive::Date | Primitive::DateTime => {
                ("string()".to_string(), Some("string".to_string()))
            }
            Primitive::Bytes => ("string()".to_string(), Some("string".to_string())),
        }
    }

    /// Render a schema expression for a type reference.
    fn render_schema_for_type_ref(
        &self,
        type_ref: &ir::gen_ir::TypeRef,
        ir: &GenIr,
        imports: &mut ImportRegistry,
    ) -> String {
        // Get the base schema
        let base = if let Some(type_decl) = ir.types.get(&type_ref.target) {
            // Reference to another type - use its schema
            let type_name = type_decl.name.pascal.clone();
            // Add to imports
            imports.register(&type_name, &format!("./{}", type_name));
            format!("{}Schema", type_name)
        } else {
            match &type_ref.target {
                ir::gen_ir::StableId::Primitive(p) => {
                    let (schema, import) = self.render_primitive_schema(*p);
                    if let Some(import) = import {
                        imports.register(&import, "@speakeasy-api/tonic");
                    }
                    schema
                }
                ir::gen_ir::StableId::Named(_) => {
                    // unknown() is provided by tonic
                    imports.register("unknown", "@speakeasy-api/tonic");
                    "unknown()".to_string()
                }
            }
        };

        let mut result = base;

        // Apply type modifiers
        for modifier in &type_ref.modifiers {
            result = match modifier {
                ir::gen_ir::TypeMod::List => {
                    imports.register("array", "@speakeasy-api/tonic");
                    format!("array({})", result)
                }
                ir::gen_ir::TypeMod::Set => {
                    imports.register("array", "@speakeasy-api/tonic");
                    format!("array({})", result)
                }
                ir::gen_ir::TypeMod::Map(value_type) => {
                    imports.register("record", "@speakeasy-api/tonic");
                    let value_schema = self.render_schema_for_type_ref(value_type, ir, imports);
                    format!("record({})", value_schema)
                }
                _ => result,
            };
        }

        // Add nullable wrapper
        if type_ref.nullable {
            imports.register("nullable", "@speakeasy-api/tonic");
            result = format!("nullable({})", result);
        }

        // Note: We don't add .optional() here anymore.
        // The calling code handles optional by:
        // - For getters (cyclic types): schema.optional() works
        // - For non-getters: use optional(schema) wrapper function

        result
    }

    /// Check if a field references any custom type (not a primitive).
    #[allow(clippy::only_used_in_recursion)]
    fn field_references_custom_type(&self, type_ref: &ir::gen_ir::TypeRef, ir: &GenIr) -> bool {
        // Check if the target is a custom type (not a primitive)
        if ir.types.contains_key(&type_ref.target) {
            return true;
        }

        // Check modifiers for nested custom type references
        for modifier in &type_ref.modifiers {
            if let ir::gen_ir::TypeMod::Map(value_type) = modifier
                && self.field_references_custom_type(value_type, ir)
            {
                return true;
            }
        }

        false
    }

    /// Render a literal as a tonic schema expression.
    fn render_literal_schema(
        &self,
        literal: &ir::gen_ir::Literal,
        imports: &mut ImportRegistry,
    ) -> String {
        use ir::gen_ir::Literal;
        match literal {
            Literal::Null => {
                imports.register("nullType", "@speakeasy-api/tonic");
                "nullType()".to_string()
            }
            Literal::Bool(b) => {
                imports.register("literal", "@speakeasy-api/tonic");
                format!("literal({})", b)
            }
            Literal::I64(i) => {
                imports.register("literal", "@speakeasy-api/tonic");
                format!("literal({})", i)
            }
            Literal::F64(f) => {
                imports.register("literal", "@speakeasy-api/tonic");
                format!("literal({})", f)
            }
            Literal::String(s) => {
                imports.register("literal", "@speakeasy-api/tonic");
                format!("literal(\"{}\")", s.replace('\"', "\\\""))
            }
            Literal::Array(_) | Literal::Object(_) => {
                imports.register("any", "@speakeasy-api/tonic");
                "any()".to_string()
            }
        }
    }

    /// Generate service files.
    fn generate_services(&self, ir: &GenIr, _config: &Config, vfs: &mut VirtualFS) -> Result<()> {
        let services_dir = PathBuf::from("src").join("services");

        // Generate individual service files
        for service in &ir.services {
            let file_name = format!("{}.ts", service.name.snake);
            let content = self.render_service(service, ir)?;
            vfs.add_file(services_dir.join(file_name), content);
        }

        // Get default base URL from server sets
        let default_base_url = if let Some(server_set) = ir.server_sets.first() {
            if let Some(server_url) = server_set.urls.first() {
                server_url.resolved_preview.clone()
            } else {
                "https://api.example.com".to_string()
            }
        } else {
            "https://api.example.com".to_string()
        };

        // Generate main SDK/client class
        let service_imports: Vec<ServiceImportData> = ir
            .services
            .iter()
            .map(|s| ServiceImportData {
                name: s.name.pascal.clone(),
                file: s.name.snake.clone(),
            })
            .collect();

        let data = ClientTemplate {
            api: &ir.api,
            services: &ir.services,
            auth_schemes: &ir.auth_schemes,
            default_base_url,
            service_imports,
        };
        let content = data
            .render()
            .map_err(|e| Error::TemplateError(Box::new(e)))?;
        vfs.add_file(services_dir.join("client.ts"), content);

        Ok(())
    }

    /// Render a single service file with all its operations.
    /// This generates a TypeScript service class with methods for each operation.
    /// All custom types used in method signatures are collected and will be imported from '../types'.
    fn render_service(&self, service: &ir::gen_ir::Service, ir: &GenIr) -> Result<String> {
        // Collect all custom types referenced by this service's operations
        // These will be imported at the top of the service file
        let mut type_imports = std::collections::HashSet::new();

        let operations: Vec<OperationData> = service
            .operations
            .iter()
            .map(|op| self.render_operation(op, ir, &mut type_imports))
            .collect::<Result<Vec<_>>>()?;

        // Sort type imports for deterministic output
        let mut type_imports_vec: Vec<String> = type_imports.into_iter().collect();
        type_imports_vec.sort();

        let data = ServiceTemplate {
            name: &service.name,
            docs: &service.docs,
            type_imports: type_imports_vec,
            operations,
            auth_schemes: &ir.auth_schemes,
        };
        data.render().map_err(|e| Error::TemplateError(Box::new(e)))
    }

    /// Render operation data for template rendering.
    /// This collects all type imports needed for this operation and renders
    /// parameters and return types using the proper TypeScript type names.
    /// Custom types will be added to type_imports and should be imported from '../types'.
    fn render_operation(
        &self,
        op: &ir::gen_ir::Operation,
        ir: &GenIr,
        type_imports: &mut std::collections::HashSet<String>,
    ) -> Result<OperationData> {
        let mut params = Vec::new();
        let mut path_params = Vec::new();
        let mut query_params = Vec::new();
        let mut header_params = Vec::new();
        let has_body = op.http.body.is_some();

        // Path parameters
        for param in &op.http.path_params {
            let type_str = self.render_type_ref(&param.ty, ir);
            self.collect_type_imports(&param.ty, ir, type_imports);
            params.push(ParamData {
                name: param.name.camel.clone(),
                type_str: type_str.clone(),
                optional: false,
                docs: param.docs.summary.clone(),
            });
            path_params.push(PathParamData {
                name: param.name.camel.clone(),
                placeholder: format!("{{{}}}", param.wire),
            });
        }

        // Query parameters
        for param in &op.http.query {
            let type_str = self.render_type_ref(&param.ty, ir);
            self.collect_type_imports(&param.ty, ir, type_imports);
            params.push(ParamData {
                name: param.name.camel.clone(),
                type_str,
                optional: !param.required,
                docs: param.docs.summary.clone(),
            });
            query_params.push(QueryParamData {
                name: param.name.camel.clone(),
                wire: param.wire.clone(),
            });
        }

        // Header parameters
        for param in &op.http.headers {
            let type_str = self.render_type_ref(&param.ty, ir);
            self.collect_type_imports(&param.ty, ir, type_imports);
            params.push(ParamData {
                name: param.name.camel.clone(),
                type_str,
                optional: !param.required,
                docs: param.docs.summary.clone(),
            });
            header_params.push(HeaderParamData {
                name: param.name.camel.clone(),
                wire: param.wire.clone(),
            });
        }

        // Request body
        if let Some(body) = &op.http.body
            && let Some(variant) = body.variants.first()
        {
            let type_str = self.render_type_ref(&variant.ty, ir);
            self.collect_type_imports(&variant.ty, ir, type_imports);
            params.push(ParamData {
                name: "body".to_string(),
                type_str,
                optional: false,
                docs: None,
            });
        }

        // Response type
        let return_type = if let Some(success) = &op.success {
            if let Some(ty) = &success.ty {
                self.collect_type_imports(ty, ir, type_imports);
                self.render_type_ref(ty, ir)
            } else {
                "void".to_string()
            }
        } else {
            "void".to_string()
        };

        let http_method = match op.http.method {
            ir::gen_ir::HttpMethod::Get => "GET",
            ir::gen_ir::HttpMethod::Post => "POST",
            ir::gen_ir::HttpMethod::Put => "PUT",
            ir::gen_ir::HttpMethod::Delete => "DELETE",
            ir::gen_ir::HttpMethod::Patch => "PATCH",
            ir::gen_ir::HttpMethod::Head => "HEAD",
            ir::gen_ir::HttpMethod::Options => "OPTIONS",
            ir::gen_ir::HttpMethod::Trace => "TRACE",
        };

        let has_params = !params.is_empty();

        // Collect auth schemes used by this operation
        let auth_schemes: Vec<AuthSchemeUse> = op
            .auth
            .iter()
            .filter_map(|auth_use| {
                // Find the auth scheme in the IR
                ir.auth_schemes
                    .iter()
                    .find(|scheme| scheme.id == auth_use.scheme)
                    .map(|scheme| {
                        let (kind, param_name) = match &scheme.kind {
                            ir::gen_ir::AuthKind::Http {
                                scheme: http_scheme,
                                ..
                            } => (format!("bearer_{}", http_scheme), None),
                            ir::gen_ir::AuthKind::ApiKey {
                                location,
                                param_name,
                            } => {
                                let kind = match location {
                                    ir::gen_ir::ApiKeyLocation::Header => "apikey_header",
                                    ir::gen_ir::ApiKeyLocation::Query => "apikey_query",
                                    ir::gen_ir::ApiKeyLocation::Cookie => "apikey_cookie",
                                };
                                (kind.to_string(), Some(param_name.clone()))
                            }
                            ir::gen_ir::AuthKind::OAuth2 { .. } => ("oauth2".to_string(), None),
                            ir::gen_ir::AuthKind::OpenIdConnect { .. } => {
                                ("openid".to_string(), None)
                            }
                        };
                        AuthSchemeUse {
                            name_camel: scheme.name.camel.clone(),
                            kind,
                            param_name,
                        }
                    })
            })
            .collect();

        // Extract error variants for this specific operation
        let error_variants: Vec<ErrorVariantData> = match &op.errors {
            ir::gen_ir::ErrorUse::Inline(error_decl) => {
                error_decl
                    .variants
                    .iter()
                    .filter_map(|v| {
                        // Only handle concrete status codes, not ranges
                        let status_code = match &v.status {
                            ir::gen_ir::StatusSpec::Code(code) => *code,
                            _ => return None,
                        };

                        let (has_body, body_type) = if let Some(ty) = &v.ty {
                            self.collect_type_imports(ty, ir, type_imports);
                            (true, self.render_type_ref(ty, ir))
                        } else {
                            (false, "void".to_string())
                        };

                        Some(ErrorVariantData {
                            class_name: format!("{}{}Error", op.name.pascal, v.name.pascal),
                            status_code,
                            has_body,
                            body_type,
                        })
                    })
                    .collect()
            }
            ir::gen_ir::ErrorUse::Shared(_) | ir::gen_ir::ErrorUse::None => Vec::new(),
        };
        let has_errors = !error_variants.is_empty();

        Ok(OperationData {
            method_name: op.name.camel.clone(),
            docs: op.docs.clone(),
            params,
            path_params,
            query_params,
            header_params,
            has_params,
            has_body,
            has_return: return_type != "void",
            return_type,
            http_method: http_method.to_string(),
            path_template: op.http.path_template.clone(),
            auth_schemes,
            error_variants,
            has_errors,
        })
    }

    /// Collect type imports from a type reference.
    #[allow(clippy::only_used_in_recursion)]
    fn collect_type_imports(
        &self,
        type_ref: &ir::gen_ir::TypeRef,
        ir: &GenIr,
        imports: &mut std::collections::HashSet<String>,
    ) {
        // Only add to imports if this is a non-primitive custom type
        if let Some(type_decl) = ir.types.get(&type_ref.target) {
            // Only import if it's actually a defined type, not a primitive
            imports.insert(type_decl.name.pascal.clone());
        }
        // Also collect from modifiers that contain nested type references
        for modifier in &type_ref.modifiers {
            if let ir::gen_ir::TypeMod::Map(value_type) = modifier {
                self.collect_type_imports(value_type, ir, imports);
            }
        }
    }

    /// Generate package.json.
    fn generate_package_json(&self, ir: &GenIr, vfs: &mut VirtualFS) -> Result<()> {
        let package_json = serde_json::json!({
            "name": ir.api.package_name.kebab.to_lowercase(),
            "version": ir.api.version,
            "description": ir.api.docs.summary.as_ref().unwrap_or(&"Generated SDK".to_string()),
            "main": "dist/index.js",
            "types": "dist/index.d.ts",
            "scripts": {
                "build": "tsc",
                "test": "jest"
            },
            "dependencies": {
                "@speakeasy-api/tonic": "0.1.0"
            },
            "devDependencies": {
                "typescript": "^5.0.0",
                "@types/web": "^0.0.294"
            }
        });

        vfs.add_file(
            "package.json",
            serde_json::to_string_pretty(&package_json).unwrap(),
        );

        Ok(())
    }

    fn generate_gitignore(&self, vfs: &mut VirtualFS) -> Result<()> {
        let gitignore = GitignoreTemplate;
        let gitignore_content = gitignore
            .render()
            .map_err(|e| Error::TemplateError(Box::new(e)))?;
        vfs.add_file(PathBuf::from(".gitignore"), gitignore_content);

        Ok(())
    }

    /// Generate TypeScript configuration.
    fn generate_tsconfig(&self, vfs: &mut VirtualFS) -> Result<()> {
        let tsconfig = serde_json::json!({
            "compilerOptions": {
                "target": "ES2022",
                "module": "commonjs",
                "lib": ["ESNext"],
                "declaration": true,
                "outDir": "./dist",
                "rootDir": "./src",
                "strict": true,
                "esModuleInterop": true,
                "skipLibCheck": true,
                "forceConsistentCasingInFileNames": true
            },
            "include": ["src/**/*"],
            "exclude": ["node_modules", "dist"],
            "types": ["@types/web"]
        });

        vfs.add_file(
            "tsconfig.json",
            serde_json::to_string_pretty(&tsconfig).unwrap(),
        );

        Ok(())
    }

    /// Generate index.ts that exports everything.
    fn generate_index(&self, ir: &GenIr, _config: &Config, vfs: &mut VirtualFS) -> Result<()> {
        let services: Vec<ServiceExportData> = ir
            .services
            .iter()
            .map(|s| ServiceExportData {
                name: s.name.pascal.clone(),
                file: s.name.snake.clone(),
            })
            .collect();

        let data = SdkExportTemplate { services };
        let index_content = data
            .render()
            .map_err(|e| Error::TemplateError(Box::new(e)))?;

        vfs.add_file(PathBuf::from("src").join("index.ts"), index_content);

        Ok(())
    }
}

impl Generator for TypeScriptGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();

        // Generate types
        self.generate_types(ir, config, &mut vfs)?;

        // Generate services
        self.generate_services(ir, config, &mut vfs)?;

        // Generate configuration files
        self.generate_package_json(ir, &mut vfs)?;
        self.generate_tsconfig(&mut vfs)?;
        self.generate_gitignore(&mut vfs)?;
        // Generate index
        self.generate_index(ir, config, &mut vfs)?;

        Ok(vfs)
    }

    fn language(&self) -> &str {
        "typescript"
    }

    fn validate(&self, ir: &GenIr) -> Result<()> {
        if ir.types.is_empty() && ir.services.is_empty() {
            return Err(Error::ValidationError(
                "IR must contain at least one type or service".to_string(),
            ));
        }
        Ok(())
    }

    fn after_write_to_disk(&self, output_dir: &Path, _vfs: &VirtualFS) -> Result<()> {
        use std::process::Command;

        // Pick the package manager from the nearest lockfile, walking up from the
        // output directory (the generated package may be a workspace member whose
        // lockfile lives at the workspace root). Falls back to npm. This keeps the
        // post-generation install/build working for repos that use bun/pnpm/yarn,
        // whose `workspace:*` ranges npm cannot resolve.
        let pm = {
            let mut found = "npm";
            let mut dir = Some(output_dir);
            while let Some(d) = dir {
                if d.join("bun.lock").exists() || d.join("bun.lockb").exists() {
                    found = "bun";
                    break;
                } else if d.join("pnpm-lock.yaml").exists() {
                    found = "pnpm";
                    break;
                } else if d.join("yarn.lock").exists() {
                    found = "yarn";
                    break;
                } else if d.join("package-lock.json").exists() {
                    found = "npm";
                    break;
                }
                dir = d.parent();
            }
            found
        };

        let status = Command::new(pm)
            .arg("install")
            .current_dir(output_dir)
            .status()
            .map_err(|e| Error::Custom(format!("Failed to run {pm} install: {e}")))?;

        if !status.success() {
            return Err(Error::Custom(format!("{pm} install failed")));
        }

        // Run the build script (tsc) via the same package manager.
        let status = Command::new(pm)
            .arg("run")
            .arg("build")
            .current_dir(output_dir)
            .status()
            .map_err(|e| Error::Custom(format!("Failed to run build: {e}")))?;

        if !status.success() {
            return Err(Error::Custom(format!("{pm} run build failed")));
        }

        Ok(())
    }
}

impl Default for TypeScriptGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// Template data structures

#[derive(Template)]
#[template(path = "interface.ts.jinja", escape = "none")]
struct InterfaceTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    fields: Vec<FieldData<'a>>,
    imports: String,
}

struct FieldData<'a> {
    name: &'a str,
    optional: bool,
    type_str: String,
    schema_expr: String,
    use_getter: bool,
    docs: &'a ir::gen_ir::Docs,
}

#[derive(Template)]
#[template(path = "enum.ts.jinja", escape = "none")]
struct EnumTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    values: Vec<EnumValueData>,
    imports: String,
}

struct EnumValueData {
    name: String,
    value: String,
}

#[derive(Template)]
#[template(path = "type_alias.ts.jinja", escape = "none")]
struct TypeAliasTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    target: String,
    schema_target: String,
    imports: String,
}

#[derive(Template)]
#[template(path = "errors.ts.jinja", escape = "none")]
struct ErrorsTemplate;

#[derive(Template)]
#[template(path = ".gitignore.jinja", escape = "none")]
struct GitignoreTemplate;

#[derive(Template)]
#[template(path = "service.ts.jinja", escape = "none")]
struct ServiceTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    type_imports: Vec<String>,
    operations: Vec<OperationData>,
    auth_schemes: &'a [ir::gen_ir::AuthScheme],
}

struct OperationData {
    method_name: String,
    docs: ir::gen_ir::Docs,
    params: Vec<ParamData>,
    path_params: Vec<PathParamData>,
    query_params: Vec<QueryParamData>,
    header_params: Vec<HeaderParamData>,
    has_params: bool,
    has_body: bool,
    has_return: bool,
    return_type: String,
    http_method: String,
    path_template: String,
    auth_schemes: Vec<AuthSchemeUse>,
    error_variants: Vec<ErrorVariantData>,
    has_errors: bool,
}

struct ErrorVariantData {
    class_name: String, // e.g., "ListPetsNotFoundError"
    status_code: u16,   // e.g., 404
    has_body: bool,     // whether error has a typed body
    body_type: String,  // e.g., "Error" or "void"
}

struct AuthSchemeUse {
    name_camel: String,
    kind: String, // "bearer", "apikey_header", "apikey_query"
    param_name: Option<String>,
}

struct ParamData {
    name: String,
    type_str: String,
    optional: bool,
    docs: Option<String>,
}

struct PathParamData {
    name: String,
    placeholder: String,
}

struct QueryParamData {
    name: String,
    wire: String,
}

struct HeaderParamData {
    name: String,
    wire: String,
}

#[derive(Template)]
#[template(path = "client.ts.jinja", escape = "none")]
struct ClientTemplate<'a> {
    api: &'a ir::gen_ir::ApiMeta,
    services: &'a [ir::gen_ir::Service],
    auth_schemes: &'a [ir::gen_ir::AuthScheme],
    default_base_url: String,
    service_imports: Vec<ServiceImportData>,
}

struct ServiceImportData {
    name: String,
    file: String,
}

#[derive(Template)]
#[template(path = "sdk_export.ts.jinja", escape = "none")]
struct SdkExportTemplate {
    services: Vec<ServiceExportData>,
}

struct ServiceExportData {
    name: String,
    file: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::gen_ir::{
        Additional, CanonicalName, Docs, Field, Primitive, StableId, TypeDecl, TypeKind, TypeRef,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn test_interface_with_field_descriptions() {
        let generator = TypeScriptGenerator::new();

        // Create a type with full documentation
        let type_name = CanonicalName::from_string("TestType");
        let type_docs = Docs {
            summary: Some("Test type summary".to_string()),
            description: Some("Test type description with more details".to_string()),
            deprecated: false,
            since: None,
            examples: Vec::new(),
            external_urls: Vec::new(),
        };

        // Create a field with documentation
        let field = Field {
            name: CanonicalName::from_string("testField"),
            docs: Docs {
                summary: Some("Field summary".to_string()),
                description: Some("Field description with more details".to_string()),
                deprecated: false,
                since: None,
                examples: Vec::new(),
                external_urls: Vec::new(),
            },
            ty: TypeRef {
                target: StableId::primitive(Primitive::String),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            },
            default: None,
            deprecated: false,
            const_value: None,
            wire_name: "testField".to_string(),
        };

        let type_decl = TypeDecl {
            id: StableId::new("TestType"),
            name: type_name,
            docs: type_docs,
            kind: TypeKind::Struct {
                fields: vec![field],
                additional: Additional::Forbidden,
                discriminator: None,
            },
            origin: None,
            tags: BTreeSet::new(),
        };

        let ir = GenIr {
            api: ir::gen_ir::ApiMeta {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                package_name: CanonicalName::from_string("test-api"),
                docs: Docs::default(),
            },
            types: BTreeMap::new(),
            services: Vec::new(),
            auth_schemes: Vec::new(),
            errors: Vec::new(),
            server_sets: Vec::new(),
        };

        let result = generator.render_type(&type_decl, &ir).unwrap();

        // Verify the output contains JSDoc comments for the type
        assert!(result.contains("/**"), "Should contain JSDoc opening");
        assert!(
            result.contains("Test type summary"),
            "Should contain type summary"
        );
        assert!(
            result.contains("Test type description with more details"),
            "Should contain type description"
        );

        // Verify the output contains JSDoc comments for the field
        assert!(
            result.contains("Field summary"),
            "Should contain field summary"
        );
        assert!(
            result.contains("Field description with more details"),
            "Should contain field description"
        );

        // Verify the interface structure
        assert!(
            result.contains("export interface TestType"),
            "Should contain interface declaration"
        );
        assert!(
            result.contains("testField: string;"),
            "Should contain field declaration"
        );
    }

    #[test]
    fn test_interface_without_descriptions() {
        let generator = TypeScriptGenerator::new();

        let type_name = CanonicalName::from_string("SimpleType");

        let field = Field {
            name: CanonicalName::from_string("simpleField"),
            docs: Docs::default(), // No summary or description
            ty: TypeRef {
                target: StableId::primitive(Primitive::String),
                optional: false,
                nullable: false,
                by_ref: false,
                modifiers: Vec::new(),
            },
            default: None,
            deprecated: false,
            const_value: None,
            wire_name: "simpleField".to_string(),
        };

        let type_decl = TypeDecl {
            id: StableId::new("SimpleType"),
            name: type_name,
            docs: Docs::default(), // No summary or description
            kind: TypeKind::Struct {
                fields: vec![field],
                additional: Additional::Forbidden,
                discriminator: None,
            },
            origin: None,
            tags: BTreeSet::new(),
        };

        let ir = GenIr {
            api: ir::gen_ir::ApiMeta {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                package_name: CanonicalName::from_string("test-api"),
                docs: Docs::default(),
            },
            types: BTreeMap::new(),
            services: Vec::new(),
            auth_schemes: Vec::new(),
            errors: Vec::new(),
            server_sets: Vec::new(),
        };

        let result = generator.render_type(&type_decl, &ir).unwrap();

        // Should not have any JSDoc comments since there are no descriptions
        assert!(
            !result.contains("/**"),
            "Should not contain JSDoc when no descriptions exist"
        );

        // But should still have the interface
        assert!(
            result.contains("export interface SimpleType"),
            "Should contain interface declaration"
        );
        assert!(
            result.contains("simpleField: string;"),
            "Should contain field declaration"
        );
    }

    #[test]
    fn test_interface_with_const_fields() {
        let generator = TypeScriptGenerator::new();

        // Create a type with const fields
        let type_name = CanonicalName::from_string("Pet");

        let fields = vec![
            Field {
                name: CanonicalName::from_string("type"),
                docs: Docs {
                    summary: Some("Type discriminator".to_string()),
                    description: Some("Always 'pet' for this type".to_string()),
                    deprecated: false,
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
                ty: TypeRef {
                    target: StableId::primitive(Primitive::String),
                    optional: false,
                    nullable: false,
                    by_ref: false,
                    modifiers: Vec::new(),
                },
                default: None,
                deprecated: false,
                const_value: Some(ir::gen_ir::Literal::String("pet".to_string())),
                wire_name: "type".to_string(),
            },
            Field {
                name: CanonicalName::from_string("name"),
                docs: Docs {
                    summary: Some("Pet name".to_string()),
                    description: None,
                    deprecated: false,
                    since: None,
                    examples: Vec::new(),
                    external_urls: Vec::new(),
                },
                ty: TypeRef {
                    target: StableId::primitive(Primitive::String),
                    optional: false,
                    nullable: false,
                    by_ref: false,
                    modifiers: Vec::new(),
                },
                default: None,
                deprecated: false,
                const_value: None,
                wire_name: "name".to_string(),
            },
            Field {
                name: CanonicalName::from_string("active"),
                docs: Docs::default(),
                ty: TypeRef {
                    target: StableId::primitive(Primitive::Bool),
                    optional: false,
                    nullable: false,
                    by_ref: false,
                    modifiers: Vec::new(),
                },
                default: None,
                deprecated: false,
                const_value: Some(ir::gen_ir::Literal::Bool(true)),
                wire_name: "active".to_string(),
            },
        ];

        let type_decl = TypeDecl {
            id: StableId::new("Pet"),
            name: type_name,
            docs: Docs::default(),
            kind: TypeKind::Struct {
                fields,
                additional: Additional::Forbidden,
                discriminator: None,
            },
            origin: None,
            tags: BTreeSet::new(),
        };

        let ir = GenIr {
            api: ir::gen_ir::ApiMeta {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                package_name: CanonicalName::from_string("test-api"),
                docs: Docs::default(),
            },
            types: BTreeMap::new(),
            services: Vec::new(),
            auth_schemes: Vec::new(),
            errors: Vec::new(),
            server_sets: Vec::new(),
        };

        let result = generator.render_type(&type_decl, &ir).unwrap();

        // Verify the interface structure
        assert!(
            result.contains("export interface Pet"),
            "Should contain interface declaration"
        );

        // Verify const fields are rendered as literal types
        assert!(
            result.contains("type: \"pet\"") || result.contains("type: 'pet'"),
            "Should render const string field as literal type, got: {}",
            result
        );
        assert!(
            result.contains("active: true"),
            "Should render const boolean field as literal type, got: {}",
            result
        );

        // Verify regular field is still rendered normally
        assert!(
            result.contains("name: string"),
            "Should render regular field with normal type, got: {}",
            result
        );
    }
}
