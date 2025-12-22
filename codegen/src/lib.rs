//! Core abstractions for code generation.
//!
//! This crate provides the foundational types and traits needed by both
//! the generation engine and individual language generators (templates).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use ir::gen_ir::GenIr;
use serde::Serialize;

mod error;
pub use error::{Error, Result};

/// Virtual file system representing the generated output.
/// Maps file paths to their content before writing to disk.
#[derive(Debug, Clone, Default)]
pub struct VirtualFS {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl VirtualFS {
    /// Create a new empty virtual file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a text file to the virtual file system.
    pub fn add_file(&mut self, path: impl Into<PathBuf>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into().into_bytes());
    }

    /// Add a binary file to the virtual file system.
    pub fn add_binary(&mut self, path: impl Into<PathBuf>, content: Vec<u8>) {
        self.files.insert(path.into(), content);
    }

    /// Get the content of a file as a string (if it's valid UTF-8).
    pub fn get_file(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    /// Get the content of a file as a string (if it's valid UTF-8).
    pub fn get_file_str(&self, path: &Path) -> Option<Result<&str>> {
        self.files
            .get(path)
            .map(|v| std::str::from_utf8(v).map_err(Error::Utf8Error))
    }

    /// Iterate over all files in the virtual file system.
    pub fn files(&self) -> impl Iterator<Item = (&Path, &[u8])> {
        self.files.iter().map(|(p, c)| (p.as_path(), c.as_slice()))
    }

    /// Get the number of files.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if the virtual file system is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Check if a file exists.
    pub fn contains(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    /// Write all files to the actual file system.
    pub fn write_to_disk(&self, base_path: &Path) -> std::io::Result<()> {
        for (path, content) in &self.files {
            let full_path = base_path.join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(full_path, content)?;
        }
        Ok(())
    }
}

/// Configuration for code generation.
#[derive(Debug, Clone, Serialize)]
pub struct Config {
    /// How to organize services.
    pub service_style: ServiceStyle,
    /// Whether to generate documentation comments.
    pub include_docs: bool,
    /// Language-specific options.
    #[serde(skip_serializing)]
    pub lang_options: BTreeMap<String, serde_json::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            service_style: ServiceStyle::PerService,
            include_docs: true,
            lang_options: BTreeMap::new(),
        }
    }
}

/// How to organize service/operation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStyle {
    /// One file per service.
    PerService,
    /// Single client file with all services.
    SingleClient,
    /// Grouped by tag/category.
    ByTag,
}

/// Main trait for language-specific code generators.
///
/// Implement this trait to create a custom generator for a language.
/// The generator has full programmatic control over file structure and
/// can use templates for rendering code snippets.
pub trait Generator: Send + Sync {
    /// Generate code from the IR into a virtual file system.
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS>;

    /// The language identifier (e.g., "typescript", "python", "rust").
    fn language(&self) -> &str;

    /// Optional: validate the IR before generation.
    fn validate(&self, _ir: &GenIr) -> Result<()> {
        Ok(())
    }

    // === Hooks ===
    // These methods allow generators to perform actions before/after key steps.
    // All have default implementations that do nothing, making them optional.

    /// Called before code generation starts.
    /// Can be used for setup or pre-generation checks.
    fn before_generate(&self, _ir: &GenIr, _config: &Config) -> Result<()> {
        Ok(())
    }

    /// Called after code is generated but before writing to disk.
    /// Can be used to modify or validate the generated files.
    fn after_generate(&self, _vfs: &VirtualFS, _ir: &GenIr, _config: &Config) -> Result<()> {
        Ok(())
    }

    /// Called after files have been written to disk.
    /// Can be used to run post-generation commands like `npm install`,
    /// formatting, or build steps.
    fn after_write_to_disk(&self, _output_dir: &Path, _vfs: &VirtualFS) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::gen_ir::{
        Additional, ApiMeta, CanonicalName, Docs, Field, Literal, StableId, TypeDecl, TypeKind,
        TypeRef,
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// Mock generator for testing
    struct MockGenerator;

    impl Generator for MockGenerator {
        fn generate(&self, ir: &GenIr, _config: &Config) -> Result<VirtualFS> {
            let mut vfs = VirtualFS::new();

            // Generate a simple file that lists all types and their const fields
            let mut output = String::new();
            output.push_str("# Generated Types\n\n");

            for (id, type_decl) in &ir.types {
                output.push_str(&format!("## Type: {} ({})\n", type_decl.name.pascal, id.to_string()));

                if let TypeKind::Struct { fields, .. } = &type_decl.kind {
                    for field in fields {
                        output.push_str(&format!("- {}: {}", field.name.camel, field.ty.target.to_string()));

                        if let Some(const_val) = &field.const_value {
                            output.push_str(&format!(" = const {:?}", const_val));
                        }

                        output.push('\n');
                    }
                }

                output.push('\n');
            }

            vfs.add_file("types.txt", output);
            Ok(vfs)
        }

        fn language(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn test_generator_with_const_fields() {
        // Create a simple IR with const fields
        let mut types = BTreeMap::new();

        let pet_type = TypeDecl {
            id: StableId::new("Pet"),
            name: CanonicalName::from_string("Pet"),
            docs: Docs::default(),
            kind: TypeKind::Struct {
                fields: vec![
                    Field {
                        name: CanonicalName::from_string("type"),
                        docs: Docs::default(),
                        ty: TypeRef {
                            target: StableId::new("String"),
                            optional: false,
                            nullable: false,
                            by_ref: false,
                            modifiers: vec![],
                        },
                        default: None,
                        deprecated: false,
                        const_value: Some(Literal::String("pet".to_string())),
                        wire_name: "type".to_string(),
                    },
                    Field {
                        name: CanonicalName::from_string("name"),
                        docs: Docs::default(),
                        ty: TypeRef {
                            target: StableId::new("String"),
                            optional: false,
                            nullable: false,
                            by_ref: false,
                            modifiers: vec![],
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
                            target: StableId::new("Boolean"),
                            optional: false,
                            nullable: false,
                            by_ref: false,
                            modifiers: vec![],
                        },
                        default: None,
                        deprecated: false,
                        const_value: Some(Literal::Bool(true)),
                        wire_name: "active".to_string(),
                    },
                ],
                additional: Additional::Forbidden,
                discriminator: None,
            },
            origin: None,
            tags: BTreeSet::new(),
        };

        types.insert(StableId::new("Pet"), pet_type);

        let gen_ir = GenIr {
            api: ApiMeta {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                package_name: CanonicalName::from_string("test-api"),
                docs: Docs::default(),
            },
            types,
            services: vec![],
            auth_schemes: vec![],
            errors: vec![],
            server_sets: vec![],
        };

        let generator = MockGenerator;
        let config = Config::default();

        let vfs = generator.generate(&gen_ir, &config).unwrap();

        // Verify the generated content
        let content = vfs.get_file_str(Path::new("types.txt")).unwrap().unwrap();

        assert!(content.contains("Type: Pet"));
        assert!(content.contains("type: String = const String(\"pet\")"));
        assert!(content.contains("name: String\n")); // No const value
        assert!(content.contains("active: Boolean = const Bool(true)"));
    }
}
