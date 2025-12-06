//! TypeScript SDK generator.
//!
//! This crate implements a TypeScript generator that demonstrates
//! the hybrid approach: programmatic file structure with template-based rendering.

use askama::Template;
use codegen::{Config, Error, GenIr, Generator, Result, VirtualFS};
use ir::gen_ir::{TypeDecl, TypeKind};
use std::path::{Path, PathBuf};

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

        // Group types into a single index file for simplicity
        let mut type_declarations = Vec::new();

        for type_decl in ir.types.values() {
            let rendered = self.render_type(type_decl, ir)?;
            type_declarations.push(rendered);
        }

        let types_content = format!(
            "// Generated types from OpenAPI specification\n\n{}",
            type_declarations.join("\n\n")
        );

        vfs.add_file(types_dir.join("index.ts"), types_content);

        Ok(())
    }

    /// Render a single type declaration.
    fn render_type(&self, type_decl: &TypeDecl, ir: &GenIr) -> Result<String> {
        match &type_decl.kind {
            TypeKind::Struct { fields, .. } => {
                let data = InterfaceTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    fields: fields
                        .iter()
                        .map(|f| {
                            // If there's a const value, use it as a literal type
                            let type_str = if let Some(const_val) = &f.const_value {
                                self.render_literal(const_val)
                            } else {
                                self.render_type_ref(&f.ty, ir)
                            };

                            FieldData {
                                name: &f.name.camel,
                                optional: f.ty.optional,
                                type_str,
                                docs: &f.docs,
                            }
                        })
                        .collect(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Enum { values, base: _ } => {
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
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Union { variants, .. } => {
                // For simplicity, render unions as type aliases with union types
                let type_names: Vec<String> = variants
                    .iter()
                    .map(|v| self.render_type_ref(&v.ty, ir))
                    .collect();
                let data = TypeAliasTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    target: type_names.join(" | "),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Alias { aliased } => {
                let target = self.render_alias_target(aliased, ir);
                let data = TypeAliasTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    target,
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
            Primitive::Date | Primitive::DateTime => "string".to_string(),
            Primitive::Uuid => "string".to_string(),
            Primitive::Bytes => "string".to_string(),
            Primitive::Decimal => "string".to_string(),
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
            let target_str = type_ref.target.0.as_str();

            if let Some(primitive_part) = target_str.strip_prefix("Primitive_") {
                match primitive_part {
                    "String" | "Uuid" | "Date" | "DateTime" | "Bytes" | "Decimal" => {
                        "string".to_string()
                    }
                    "Bool" => "boolean".to_string(),
                    "I32" | "I64" | "F32" | "F64" => "number".to_string(),
                    "Any" => "any".to_string(),
                    _ => "any".to_string(),
                }
            } else {
                // Can't find the type - return any
                "any".to_string()
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

        let status = Command::new("npm")
            .arg("install")
            .current_dir(output_dir)
            .status()
            .map_err(|e| Error::Custom(format!("Failed to run npm install: {}", e)))?;

        if !status.success() {
            return Err(Error::Custom("npm install failed".to_string()));
        }

        // Run tsc
        let status = Command::new("npm")
            .arg("run")
            .arg("build")
            .current_dir(output_dir)
            .status()
            .map_err(|e| Error::Custom(format!("Failed to run tsc: {}", e)))?;

        if !status.success() {
            return Err(Error::Custom("npm run build failed".to_string()));
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
}

struct FieldData<'a> {
    name: &'a str,
    optional: bool,
    type_str: String,
    docs: &'a ir::gen_ir::Docs,
}

#[derive(Template)]
#[template(path = "enum.ts.jinja", escape = "none")]
struct EnumTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    values: Vec<EnumValueData>,
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
}

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
        Additional, CanonicalName, Docs, Field, StableId, TypeDecl, TypeKind, TypeRef,
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
                target: StableId("Primitive_String".to_string()),
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
            id: StableId("TestType".to_string()),
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
                target: StableId("Primitive_String".to_string()),
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
            id: StableId("SimpleType".to_string()),
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
                    target: StableId("Primitive_String".to_string()),
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
                    target: StableId("Primitive_String".to_string()),
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
                    target: StableId("Primitive_Bool".to_string()),
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
            id: StableId("Pet".to_string()),
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
