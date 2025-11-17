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
            .map(|v| std::str::from_utf8(v).map_err(|e| Error::Utf8Error(e)))
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
