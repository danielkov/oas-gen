//! TypeScript SDK generator (v2) using AST + pretty-printer.
//!
//! Generates pre-formatted TypeScript output without requiring external formatters.

pub mod ast;
pub mod emit_client;
pub mod emit_config;
pub mod emit_services;
pub mod emit_types;
pub mod imports;
pub mod lower;

use codegen::{Config, Error, GenIr, Generator, Result, VirtualFS};
use std::path::{Path, PathBuf};

use crate::lower::render_module;

/// TypeScript SDK generator v2 — AST + pretty-printer pipeline.
pub struct TypeScript2Generator;

impl TypeScript2Generator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TypeScript2Generator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for TypeScript2Generator {
    fn language(&self) -> &str {
        "typescript2"
    }

    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        let types_dir = PathBuf::from("src").join("types");
        let services_dir = PathBuf::from("src").join("services");

        // 1. Generate type files (one per type)
        let mut type_names = Vec::new();
        for type_decl in ir.types.values() {
            let module = emit_types::emit_type(type_decl, ir);
            let content = render_module(&module);
            let file_name = format!("{}.ts", type_decl.name.pascal);
            vfs.add_file(types_dir.join(&file_name), content);
            type_names.push(type_decl.name.pascal.clone());
        }

        // 2. Generate types/index.ts
        type_names.sort();
        let type_index = emit_types::emit_type_index(&type_names);
        let content = render_module(&type_index);
        vfs.add_file(types_dir.join("index.ts"), content);

        // 3. Generate errors.ts
        let errors_module = emit_types::emit_errors();
        let content = render_module(&errors_module);
        vfs.add_file(types_dir.join("errors.ts"), content);

        // 4. Generate service files
        for service in &ir.services {
            let module = emit_services::emit_service(service, ir);
            let content = render_module(&module);
            let file_name = format!("{}.ts", service.name.snake);
            vfs.add_file(services_dir.join(file_name), content);
        }

        // 5. Generate client.ts
        let client_module = emit_client::emit_client(ir);
        let content = render_module(&client_module);
        vfs.add_file(services_dir.join("client.ts"), content);

        // 6. Generate src/index.ts
        let sdk_index = emit_client::emit_sdk_index(ir);
        let content = render_module(&sdk_index);
        vfs.add_file(PathBuf::from("src").join("index.ts"), content);

        // 7. Generate config files (package.json, tsconfig, .gitignore)
        emit_config::emit_config_files(ir, config, &mut vfs)?;

        Ok(vfs)
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
        // output directory. Falls back to npm. Keeps the post-generation install
        // working for bun/pnpm/yarn repos whose `workspace:*` ranges npm rejects.
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

        Ok(())
    }
}
