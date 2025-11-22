//! Service module code generation

use askama::Template;
use ir::gen_ir::{AuthKind, AuthScheme, HttpMethod, Operation, Service};

/// Wrapper for operations with preprocessed data for templates
#[derive(Debug, Clone)]
struct OperationTemplate<'a> {
    operation: &'a Operation,
    method_fn: &'static str,
}

impl<'a> OperationTemplate<'a> {
    fn new(operation: &'a Operation) -> Self {
        let method_fn = match operation.http.method {
            HttpMethod::Get => "get",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Delete => "delete",
            HttpMethod::Patch => "patch",
            HttpMethod::Head => "head",
            HttpMethod::Options => "options",
            HttpMethod::Trace => "trace",
        };
        Self {
            operation,
            method_fn,
        }
    }
}

/// Template data for service module generation
#[derive(Template)]
#[template(path = "service_module.rs.jinja", escape = "none")]
struct ServiceModuleTemplate<'a> {
    trait_name: &'a str,
    has_bearer_auth: bool,
    has_api_key_auth: bool,
    operations: Vec<OperationTemplate<'a>>,
}

pub struct ServiceModuleGenerator<'a> {
    service: &'a Service,
    auth_schemes: &'a [AuthScheme],
}

impl<'a> ServiceModuleGenerator<'a> {
    pub fn new(service: &'a Service, auth_schemes: &'a [AuthScheme]) -> Self {
        Self {
            service,
            auth_schemes,
        }
    }

    pub fn generate(&self) -> String {
        // Determine which auth wrappers to generate
        let mut has_bearer_auth = false;
        let mut has_api_key_auth = false;

        for scheme in self.auth_schemes {
            match &scheme.kind {
                AuthKind::Http { scheme: s, .. } if s == "bearer" => {
                    has_bearer_auth = true;
                }
                AuthKind::ApiKey { .. } => {
                    has_api_key_auth = true;
                }
                _ => {}
            }
        }

        // Wrap operations with preprocessed data
        let operations: Vec<OperationTemplate> = self
            .service
            .operations
            .iter()
            .map(OperationTemplate::new)
            .collect();

        let template = ServiceModuleTemplate {
            trait_name: &self.service.name.pascal,
            has_bearer_auth,
            has_api_key_auth,
            operations,
        };

        template
            .render()
            .unwrap_or_else(|e| panic!("Failed to render service module template: {}", e))
    }
}
