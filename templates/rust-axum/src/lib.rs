//! Rust Axum server generator.
//!
//! Generates a production-ready Axum server with:
//! - One module per OpenAPI tag with feature flags
//! - RequestContext for state and request metadata
//! - Auth wrappers from security schemes
//! - Per-operation result and error types with IntoResponse
//! - Generated handlers with proper Axum extractors
//! - Router function and extension trait for ergonomic usage

mod service_codegen;

use askama::Template;
use codegen::{Config, Error, GenIr, Generator, Result, VirtualFS};
use ir::gen_ir::{CanonicalName, Service, TypeDecl};
use service_codegen::ServiceModuleGenerator;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Rust Axum server generator.
pub struct RustAxumGenerator;

impl RustAxumGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate types organized by tags
    fn generate_types(&self, ir: &GenIr, _config: &Config, vfs: &mut VirtualFS) -> Result<()> {
        let types_by_tag = self.group_types_by_tag(ir);

        for (tag, types) in &types_by_tag {
            let module_name = CanonicalName::from_string(tag);
            self.generate_tag_types_module(tag, &module_name, types, ir, vfs)?;
        }

        // Generate types/mod.rs
        self.generate_types_mod(&types_by_tag, vfs)?;

        Ok(())
    }

    /// Group types by their tags
    fn group_types_by_tag<'a>(&self, ir: &'a GenIr) -> BTreeMap<String, Vec<&'a TypeDecl>> {
        let mut types_by_tag: BTreeMap<String, Vec<&'a TypeDecl>> = BTreeMap::new();

        for type_decl in ir.types.values() {
            if type_decl.tags.is_empty() {
                types_by_tag
                    .entry("common".to_string())
                    .or_default()
                    .push(type_decl);
            } else {
                for tag in &type_decl.tags {
                    types_by_tag.entry(tag.clone()).or_default().push(type_decl);
                }
            }
        }

        types_by_tag
    }

    /// Generate types module for a tag
    fn generate_tag_types_module(
        &self,
        tag: &str,
        module_name: &CanonicalName,
        types: &[&TypeDecl],
        ir: &GenIr,
        vfs: &mut VirtualFS,
    ) -> Result<()> {
        let mut type_impls = Vec::new();

        for type_decl in types {
            let rendered = self.render_type(type_decl, ir)?;
            type_impls.push(rendered);
        }

        let content = format!(
            "//! Types for {} API\n\nuse serde::{{Deserialize, Serialize}};\n\n{}",
            tag,
            type_impls.join("\n\n")
        );

        let file_path = PathBuf::from("src")
            .join("types")
            .join(format!("{}.rs", module_name.snake));
        vfs.add_file(file_path, content);

        Ok(())
    }

    /// Generate types/mod.rs
    fn generate_types_mod(
        &self,
        types_by_tag: &BTreeMap<String, Vec<&TypeDecl>>,
        vfs: &mut VirtualFS,
    ) -> Result<()> {
        let mut mod_content = String::from("//! API types organized by tag\n\n");

        for tag in types_by_tag.keys() {
            let module_name = CanonicalName::from_string(tag);
            let feature_name = module_name.snake.clone();

            if tag == "common" {
                mod_content.push_str(&format!("pub mod {};\n", module_name.snake));
            } else {
                mod_content.push_str(&format!(
                    "#[cfg(feature = \"{}\")]\npub mod {};\n",
                    feature_name, module_name.snake
                ));
            }
        }

        vfs.add_file("src/types/mod.rs", mod_content);
        Ok(())
    }

    /// Render a type declaration
    fn render_type(&self, type_decl: &TypeDecl, ir: &GenIr) -> Result<String> {
        use ir::gen_ir::TypeKind;

        match &type_decl.kind {
            TypeKind::Struct { fields, .. } => {
                let fields_str: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        format!(
                            "    pub {}: {},",
                            f.name.snake,
                            self.render_type_ref(&f.ty, ir)
                        )
                    })
                    .collect();

                Ok(format!(
                    "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n{}\n}}",
                    type_decl.name.pascal,
                    fields_str.join("\n")
                ))
            }
            TypeKind::Enum { values, .. } => {
                let variants: Vec<String> = values
                    .iter()
                    .map(|v| format!("    {},", v.name.pascal))
                    .collect();

                Ok(format!(
                    "#[derive(Debug, Clone, Serialize, Deserialize)]\npub enum {} {{\n{}\n}}",
                    type_decl.name.pascal,
                    variants.join("\n")
                ))
            }
            TypeKind::Alias { aliased } => {
                let target = self.render_alias_target(aliased, ir);
                Ok(format!("pub type {} = {};", type_decl.name.pascal, target))
            }
            TypeKind::Union { .. } => Ok(format!(
                "// TODO: Union type {}\npub type {} = serde_json::Value;",
                type_decl.name.pascal, type_decl.name.pascal
            )),
        }
    }

    fn render_type_ref(&self, type_ref: &ir::gen_ir::TypeRef, ir: &GenIr) -> String {
        let base = if let Some(type_decl) = ir.types.get(&type_ref.target) {
            type_decl.name.pascal.clone()
        } else {
            self.render_primitive_from_id(&type_ref.target.0)
        };

        let mut result = base;

        for modifier in &type_ref.modifiers {
            result = match modifier {
                ir::gen_ir::TypeMod::List => format!("Vec<{}>", result),
                ir::gen_ir::TypeMod::Set => format!("std::collections::HashSet<{}>", result),
                ir::gen_ir::TypeMod::Map(value_type) => {
                    format!(
                        "std::collections::HashMap<String, {}>",
                        self.render_type_ref(value_type, ir)
                    )
                }
                _ => result,
            };
        }

        if type_ref.optional {
            result = format!("Option<{}>", result);
        }

        result
    }

    fn render_primitive_from_id(&self, id: &str) -> String {
        if let Some(prim_name) = id.strip_prefix("Primitive_") {
            match prim_name {
                "String" | "Uuid" | "Date" | "DateTime" => "String".to_string(),
                "Bool" => "bool".to_string(),
                "I32" => "i32".to_string(),
                "I64" => "i64".to_string(),
                "F32" => "f32".to_string(),
                "F64" => "f64".to_string(),
                "Bytes" => "Vec<u8>".to_string(),
                "Decimal" => "String".to_string(),
                _ => "serde_json::Value".to_string(),
            }
        } else {
            "serde_json::Value".to_string()
        }
    }

    fn render_alias_target(&self, target: &ir::gen_ir::AliasTarget, ir: &GenIr) -> String {
        use ir::gen_ir::{AliasTarget, Composite, Primitive};
        match target {
            AliasTarget::Primitive(Primitive::String) => "String".to_string(),
            AliasTarget::Primitive(Primitive::Bool) => "bool".to_string(),
            AliasTarget::Primitive(Primitive::I32) => "i32".to_string(),
            AliasTarget::Primitive(Primitive::I64) => "i64".to_string(),
            AliasTarget::Primitive(Primitive::F32) => "f32".to_string(),
            AliasTarget::Primitive(Primitive::F64) => "f64".to_string(),
            AliasTarget::Primitive(_) => "String".to_string(),
            AliasTarget::Composite(Composite::List(inner)) => {
                format!("Vec<{}>", self.render_type_ref(inner, ir))
            }
            AliasTarget::Composite(Composite::Map { value, .. }) => {
                format!(
                    "std::collections::HashMap<String, {}>",
                    self.render_type_ref(value, ir)
                )
            }
            AliasTarget::Composite(Composite::Tuple(types)) => {
                let rendered: Vec<String> =
                    types.iter().map(|t| self.render_type_ref(t, ir)).collect();
                format!("({})", rendered.join(", "))
            }
            AliasTarget::Reference(type_ref) => self.render_type_ref(type_ref, ir),
        }
    }

    /// Generate service modules (one per tag)
    fn generate_services(&self, ir: &GenIr, _config: &Config, vfs: &mut VirtualFS) -> Result<()> {
        for service in &ir.services {
            self.generate_service_module(service, ir, vfs)?;
        }

        self.generate_services_mod(ir, vfs)?;

        Ok(())
    }

    /// Generate a complete service module for a tag
    fn generate_service_module(
        &self,
        service: &Service,
        ir: &GenIr,
        vfs: &mut VirtualFS,
    ) -> Result<()> {
        let module_name = &service.name.snake;

        let generator = ServiceModuleGenerator::new(service, &ir.auth_schemes);
        let content = generator.generate();

        let file_path = PathBuf::from("src")
            .join("services")
            .join(format!("{}.rs", module_name));
        vfs.add_file(file_path, content);

        Ok(())
    }

    /// Generate services/mod.rs
    fn generate_services_mod(&self, ir: &GenIr, vfs: &mut VirtualFS) -> Result<()> {
        let mut mod_content = String::from("//! Service interfaces organized by tag\n\n");

        for service in &ir.services {
            let feature_name = service.name.snake.clone();

            if service.name.canonical == "default" {
                mod_content.push_str(&format!("pub mod {};\n", service.name.snake));
            } else {
                mod_content.push_str(&format!(
                    "#[cfg(feature = \"{}\")]\npub mod {};\n",
                    feature_name, service.name.snake
                ));
            }
        }

        vfs.add_file("src/services/mod.rs", mod_content);
        Ok(())
    }

    /// Generate Cargo.toml with feature flags
    fn generate_cargo_toml(&self, ir: &GenIr, vfs: &mut VirtualFS) -> Result<()> {
        let mut tags: BTreeSet<String> = BTreeSet::new();
        for service in &ir.services {
            if service.name.canonical != "default" {
                tags.insert(service.name.snake.clone());
            }
        }

        let data = CargoTomlData {
            package_name: &ir.api.package_name.snake,
            version: &ir.api.version,
            tags: tags.iter().cloned().collect(),
        };

        let content = data
            .render()
            .map_err(|e| Error::TemplateError(Box::new(e)))?;

        vfs.add_file("Cargo.toml", content);
        Ok(())
    }

    /// Generate lib.rs
    fn generate_lib_rs(&self, _ir: &GenIr, vfs: &mut VirtualFS) -> Result<()> {
        let mut content = String::from("//! Generated Axum API\n\n");
        content.push_str("pub mod types;\n");
        content.push_str("pub mod services;\n");

        vfs.add_file("src/lib.rs", content);
        Ok(())
    }
}

impl Generator for RustAxumGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();

        self.generate_types(ir, config, &mut vfs)?;
        self.generate_services(ir, config, &mut vfs)?;
        self.generate_cargo_toml(ir, &mut vfs)?;
        self.generate_lib_rs(ir, &mut vfs)?;

        Ok(vfs)
    }

    fn language(&self) -> &str {
        "rust-axum"
    }

    fn validate(&self, ir: &GenIr) -> Result<()> {
        if ir.types.is_empty() && ir.services.is_empty() {
            return Err(Error::ValidationError(
                "IR must contain at least one type or service".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for RustAxumGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// Template data structures

#[derive(Template)]
#[template(path = "Cargo.toml.jinja", escape = "none")]
struct CargoTomlData<'a> {
    package_name: &'a str,
    version: &'a str,
    tags: Vec<String>,
}
