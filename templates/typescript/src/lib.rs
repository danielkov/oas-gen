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

        for (_id, type_decl) in &ir.types {
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
                        .map(|f| FieldData {
                            name: &f.name.camel,
                            optional: f.ty.optional,
                            type_str: self.render_type_ref(&f.ty, ir),
                            docs: f.docs.summary.as_deref(),
                        })
                        .collect(),
                };
                data.render().map_err(|e| Error::TemplateError(Box::new(e)))
            }
            TypeKind::Enum { base, values } => {
                let data = EnumTemplate {
                    name: &type_decl.name,
                    docs: &type_decl.docs,
                    base_type: self.render_primitive(*base),
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

    fn render_type_ref(&self, type_ref: &ir::gen_ir::TypeRef, ir: &GenIr) -> String {
        // Get the base type name
        let base = if let Some(type_decl) = ir.types.get(&type_ref.target) {
            type_decl.name.pascal.clone()
        } else {
            let target_str = type_ref.target.0.as_str();

            if target_str.starts_with("Primitive_") {
                let primitive_part = &target_str["Primitive_".len()..];
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
                wire: param.wire.clone(),
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
        if let Some(body) = &op.http.body {
            if let Some(variant) = body.variants.first() {
                let type_str = self.render_type_ref(&variant.ty, ir);
                self.collect_type_imports(&variant.ty, ir, type_imports);
                params.push(ParamData {
                    name: "body".to_string(),
                    type_str,
                    optional: false,
                    docs: None,
                });
            }
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
        })
    }

    /// Collect type imports from a type reference.
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
    docs: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "enum.ts.jinja", escape = "none")]
struct EnumTemplate<'a> {
    name: &'a ir::gen_ir::CanonicalName,
    docs: &'a ir::gen_ir::Docs,
    base_type: String,
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
    wire: String,
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
