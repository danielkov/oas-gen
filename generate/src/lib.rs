//! Code generation engine for OpenAPI -> SDK/Server generation.
//!
//! This crate provides a flexible plugin-based architecture for generating
//! code from OpenAPI specifications. It uses a hybrid approach:
//! - **Generator Strategy** (Rust code): decides file structure and logic
//! - **Template Fragments** (Handlebars): renders code snippets

use std::collections::BTreeMap;
use std::path::Path;

use codegen::{Config, Error, GenIr, Generator, Result, VirtualFS};

/// Registry for managing multiple generators.
///
/// This allows for built-in generators and dynamic plugin loading.
pub struct GeneratorRegistry {
    generators: BTreeMap<String, Box<dyn Generator>>,
}

impl GeneratorRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            generators: BTreeMap::new(),
        }
    }

    /// Create a registry with default built-in generators.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register built-in generators based on enabled features
        #[cfg(feature = "typescript")]
        registry.register(Box::new(typescript::TypeScriptGenerator::new()));

        registry
    }

    /// Register a generator.
    pub fn register(&mut self, generator: Box<dyn Generator>) {
        self.generators
            .insert(generator.language().to_string(), generator);
    }

    /// Get a generator by language name.
    pub fn get(&self, language: &str) -> Option<&dyn Generator> {
        self.generators.get(language).map(|g| g.as_ref())
    }

    /// List all available generator languages.
    pub fn languages(&self) -> Vec<&str> {
        self.generators.keys().map(|s| s.as_str()).collect()
    }

    /// Generate code using the specified language generator.
    /// This calls the generator's hooks at appropriate times.
    pub fn generate(&self, language: &str, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let generator = self
            .get(language)
            .ok_or_else(|| Error::GeneratorNotFound(language.to_string()))?;

        generator.validate(ir)?;

        // Call before_generate hook
        generator.before_generate(ir, config)?;

        // Generate the code
        let vfs = generator.generate(ir, config)?;

        // Call after_generate hook
        generator.after_generate(&vfs, ir, config)?;

        Ok(vfs)
    }

    /// Call the after_write_to_disk hook for the specified generator.
    /// This should be called after writing files to disk.
    pub fn after_write_to_disk(
        &self,
        language: &str,
        output_dir: &Path,
        vfs: &VirtualFS,
    ) -> Result<()> {
        let generator = self
            .get(language)
            .ok_or_else(|| Error::GeneratorNotFound(language.to_string()))?;

        generator.after_write_to_disk(output_dir, vfs)
    }

    /// Load a dynamic plugin from a shared library.
    /// Requires the "dynamic-plugins" feature.
    #[cfg(feature = "dynamic-plugins")]
    pub fn load_plugin(&mut self, path: &Path) -> Result<()> {
        use libloading::{Library, Symbol};

        let lib = unsafe { Library::new(path).map_err(|e| Error::PluginLoadError(e.to_string()))? };

        // Plugin must export: extern "C" fn create_generator() -> Box<dyn Generator>
        let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn Generator> = unsafe {
            lib.get(b"create_generator")
                .map_err(|e| Error::PluginLoadError(e.to_string()))?
        };

        let generator_ptr = unsafe { constructor() };
        let generator = unsafe { Box::from_raw(generator_ptr) };

        self.register(generator);

        // Keep the library loaded
        std::mem::forget(lib);

        Ok(())
    }
}

impl Default for GeneratorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry() {
        let registry = GeneratorRegistry::with_defaults();

        #[cfg(feature = "typescript")]
        {
            assert!(!registry.languages().is_empty());
            assert!(registry.languages().contains(&"typescript"));
        }
    }
}
