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

#[cfg(test)]
mod tests {
    use super::*;
    use ir::gen_ir::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn emits_error_hook_context_for_network_and_http_failures() {
        let vfs = TypeScript2Generator::new()
            .generate(&error_hook_ir(), &Config::default())
            .unwrap();
        let client = vfs
            .get_file_str(Path::new("src/services/client.ts"))
            .unwrap()
            .unwrap();
        let service = vfs
            .get_file_str(Path::new("src/services/widgets.ts"))
            .unwrap()
            .unwrap();

        assert!(client.contains("export interface SDKErrorContext"));
        assert!(client.contains("request: Omit<SDKRequestInit, 'body'>;"));
        assert!(client.contains("response?: SDKResponseInfo;"));
        assert!(client.contains(
            "onError?: (error: unknown, context: SDKErrorContext) => void | Promise<void>;"
        ));

        let request_info = declaration(service, "const requestInfo:");
        assert!(request_info.contains("method: request.method"));
        assert!(request_info.contains("url: request.url"));
        assert!(request_info.contains("headers: request.headers"));
        assert!(!request_info.contains("body:"));
        assert!(service.contains("body: request.body"));

        let fetch_try_start = service
            .find("    try {\n      response = await fetch(")
            .unwrap();
        let fetch_catch_start = service[fetch_try_start..]
            .find("    } catch (error) {")
            .unwrap()
            + fetch_try_start;
        let fetch_try = &service[fetch_try_start..fetch_catch_start];
        assert!(!fetch_try.contains("onError"));
        assert!(!fetch_try.contains("onResponse"));

        let response_info_start = service.find("    const responseInfo:").unwrap();
        let fetch_catch = &service[fetch_catch_start..response_info_start];
        assert_eq!(fetch_catch.matches("onError").count(), 1);
        assert!(fetch_catch.contains("(error, { request: requestInfo })"));
        assert!(fetch_catch.contains("throw error;"));

        assert!(service.contains("const errorContext: SDKErrorContext"));
        assert!(service.contains("response: responseInfo"));
        assert!(
            service.contains("await this.raise(new CreateWidgetBadRequestError(), errorContext);")
        );
    }

    fn declaration<'a>(output: &'a str, prefix: &str) -> &'a str {
        let start = output.find(prefix).unwrap();
        let end = output[start..].find("};").unwrap() + start + 2;
        &output[start..end]
    }

    fn error_hook_ir() -> GenIr {
        let string_ref = TypeRef {
            target: StableId::primitive(Primitive::String),
            optional: false,
            nullable: false,
            by_ref: false,
            modifiers: Vec::new(),
        };
        let operation = Operation {
            id: StableId::new("CreateWidget"),
            name: CanonicalName::from_string("CreateWidget"),
            docs: Docs::default(),
            deprecated: false,
            http: HttpShape {
                method: HttpMethod::Post,
                path_template: "/widgets".to_string(),
                segments: Vec::new(),
                query: Vec::new(),
                headers: Vec::new(),
                cookies: Vec::new(),
                path_params: Vec::new(),
                body: Some(Body {
                    variants: vec![BodyVariant {
                        content_type: "application/json".to_string(),
                        ty: string_ref,
                        docs: Docs::default(),
                        encoding: Vec::new(),
                    }],
                    preferred: Some("application/json".to_string()),
                }),
                consumes: vec!["application/json".to_string()],
                produces: Vec::new(),
            },
            success: None,
            alt_success: Vec::new(),
            errors: ErrorUse::Inline(Box::new(ErrorDecl {
                id: StableId::new("CreateWidgetErrors"),
                name: CanonicalName::from_string("CreateWidgetErrors"),
                docs: Docs::default(),
                variants: vec![ErrorVariant {
                    name: CanonicalName::from_string("BadRequest"),
                    status: StatusSpec::Code(400),
                    content_type: None,
                    ty: None,
                    docs: Docs::default(),
                }],
            })),
            auth: Vec::new(),
            pagination: None,
            idempotent: false,
            retryable_statuses: BTreeSet::new(),
        };

        GenIr {
            api: ApiMeta {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                package_name: CanonicalName::from_string("test-api"),
                docs: Docs::default(),
            },
            types: BTreeMap::new(),
            services: vec![Service {
                id: StableId::new("Widgets"),
                name: CanonicalName::from_string("Widgets"),
                docs: Docs::default(),
                server_set: None,
                operations: vec![operation],
            }],
            auth_schemes: Vec::new(),
            errors: Vec::new(),
            server_sets: Vec::new(),
        }
    }
}
