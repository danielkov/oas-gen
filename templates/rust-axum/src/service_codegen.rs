//! Service module code generation

use ir::gen_ir::{AuthKind, AuthScheme, HttpMethod, Service};
use std::collections::HashSet;

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
        let mut code = String::new();

        // Module header
        code.push_str(&format!(
            "//! {} service module\n\n",
            self.service.name.pascal
        ));

        // Imports
        code.push_str(&self.generate_imports());

        // RequestContext
        code.push_str(&self.generate_request_context());

        // Auth wrappers
        code.push_str(&self.generate_auth_wrappers());

        // Per-operation result and error types
        code.push_str(&self.generate_operation_types());

        // Service trait
        code.push_str(&self.generate_trait());

        // Query parameter structs
        code.push_str(&self.generate_query_structs());

        // Handlers
        code.push_str(&self.generate_handlers());

        // Router function
        code.push_str(&self.generate_router());

        // Extension trait
        code.push_str(&self.generate_extension_trait());

        code
    }

    fn generate_imports(&self) -> String {
        r#"use axum::{
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::types;

"#
        .to_string()
    }

    fn generate_request_context(&self) -> String {
        r#"/// Request context containing state and request metadata
pub struct RequestContext<S> {
    pub state: S,
    pub method: axum::http::Method,
    pub uri: axum::http::Uri,
    pub headers: axum::http::HeaderMap,
    pub extensions: axum::http::Extensions,
}

impl<S> RequestContext<S> {
    pub fn state(&self) -> &S {
        &self.state
    }
}

impl<S: Clone> RequestContext<S> {
    pub(crate) fn from_parts(state: S, parts: axum::http::request::Parts) -> Self {
        RequestContext {
            state,
            method: parts.method,
            uri: parts.uri,
            headers: parts.headers,
            extensions: parts.extensions,
        }
    }
}

"#
        .to_string()
    }

    fn generate_auth_wrappers(&self) -> String {
        let mut code = String::new();
        let mut generated = HashSet::new();

        for scheme in self.auth_schemes {
            match &scheme.kind {
                AuthKind::Http { scheme: s, .. }
                    if s == "bearer" && !generated.contains("AuthBearer") =>
                {
                    code.push_str(
                        r#"/// Bearer authentication token
#[derive(Clone, Debug)]
pub struct AuthBearer(pub String);

"#,
                    );
                    generated.insert("AuthBearer");
                }
                AuthKind::ApiKey { .. } if !generated.contains("AuthApiKey") => {
                    code.push_str(
                        r#"/// API Key authentication
#[derive(Clone, Debug)]
pub struct AuthApiKey(pub String);

"#,
                    );
                    generated.insert("AuthApiKey");
                }
                _ => {}
            }
        }

        code
    }

    fn generate_operation_types(&self) -> String {
        let mut code = String::new();

        code.push_str("// Per-operation result and error types\n");

        for op in &self.service.operations {
            let op_name = &op.name.pascal;

            // Result type
            if let Some(success) = &op.success {
                if let Some(ty) = &success.ty {
                    code.push_str(&format!(
                        "pub type {}Result = Result<types::{}, {}Error>;\n\n",
                        op_name, ty.target.0, op_name
                    ));
                } else {
                    code.push_str(&format!(
                        "pub type {}Result = Result<(), {}Error>;\n\n",
                        op_name, op_name
                    ));
                }
            } else {
                code.push_str(&format!(
                    "pub type {}Result = Result<(), {}Error>;\n\n",
                    op_name, op_name
                ));
            }

            // Error enum
            code.push_str(&format!("#[derive(Debug)]\npub enum {}Error {{\n", op_name));

            // Add error variants from operation
            if let ir::gen_ir::ErrorUse::Inline(error_decl) = &op.errors {
                for variant in &error_decl.variants {
                    if let Some(ty) = &variant.ty {
                        code.push_str(&format!(
                            "    {}(types::{}),\n",
                            variant.name.pascal, ty.target.0
                        ));
                    } else {
                        code.push_str(&format!("    {},\n", variant.name.pascal));
                    }
                }
            }

            code.push_str("    InternalError(String),\n}\n\n");

            // IntoResponse implementation
            code.push_str(&format!(
                "impl IntoResponse for {}Error {{\n    fn into_response(self) -> Response {{\n        match self {{\n",
                op_name
            ));

            if let ir::gen_ir::ErrorUse::Inline(error_decl) = &op.errors {
                for variant in &error_decl.variants {
                    let status_code = match &variant.status {
                        ir::gen_ir::StatusSpec::Code(c) => *c,
                        _ => 500,
                    };

                    if variant.ty.is_some() {
                        code.push_str(&format!(
                            "            {}Error::{}(err) => {{\n",
                            op_name, variant.name.pascal
                        ));
                        code.push_str(&format!(
                            "                let status = StatusCode::from_u16({}).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);\n",
                            status_code
                        ));
                        code.push_str(
                            "                (status, Json(err)).into_response()\n            }\n",
                        );
                    } else {
                        code.push_str(&format!(
                            "            {}Error::{} => {{\n",
                            op_name, variant.name.pascal
                        ));
                        code.push_str(&format!(
                            "                StatusCode::from_u16({}).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR).into_response()\n            }}\n",
                            status_code
                        ));
                    }
                }
            }

            code.push_str(&format!(
                "            {}Error::InternalError(msg) => {{\n",
                op_name
            ));
            code.push_str("                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()\n            }\n");
            code.push_str("        }\n    }\n}\n\n");
        }

        code
    }

    fn generate_trait(&self) -> String {
        let mut code = String::new();
        let trait_name = &self.service.name.pascal;

        code.push_str(&format!(
            "/// {} service trait\npub trait {}<S>: Send + Sync {{\n",
            trait_name, trait_name
        ));

        for op in &self.service.operations {
            let op_name = &op.name.pascal;
            let op_snake = &op.name.snake;

            code.push_str(&format!(
                "    /// {} {}\n",
                format!("{:?}", op.http.method),
                op.http.path_template
            ));
            code.push_str(&format!("    async fn {}(\n", op_snake));
            code.push_str("        &self,\n");
            code.push_str("        ctx: RequestContext<S>,\n");

            // Path parameters
            for param in &op.http.path_params {
                code.push_str(&format!("        {}: String,\n", param.name.snake));
            }

            // Query parameters
            if !op.http.query.is_empty() {
                code.push_str(&format!("        query: {}Query,\n", op_name));
            }

            // Body
            if let Some(body) = &op.http.body {
                if let Some(variant) = body.variants.first() {
                    code.push_str(&format!("        body: types::{},\n", variant.ty.target.0));
                }
            }

            code.push_str(&format!("    ) -> {}Result;\n\n", op_name));
        }

        code.push_str("}\n\n");

        code
    }

    fn generate_query_structs(&self) -> String {
        let mut code = String::new();

        code.push_str("// Query parameter structs\n");

        for op in &self.service.operations {
            if !op.http.query.is_empty() {
                let op_name = &op.name.pascal;
                code.push_str(&format!(
                    "#[derive(Debug, Deserialize)]\npub struct {}Query {{\n",
                    op_name
                ));

                for param in &op.http.query {
                    let type_str = if param.required {
                        "String".to_string()
                    } else {
                        "Option<String>".to_string()
                    };
                    code.push_str(&format!("    pub {}: {},\n", param.name.snake, type_str));
                }

                code.push_str("}\n\n");
            }
        }

        code
    }

    fn generate_handlers(&self) -> String {
        let mut code = String::new();
        let trait_name = &self.service.name.pascal;

        code.push_str("// Handlers\n");

        for op in &self.service.operations {
            let op_name = &op.name.pascal;
            let op_snake = &op.name.snake;

            code.push_str(&format!("async fn {}_handler<S, H>(\n", op_snake));
            code.push_str("    State(state): State<S>,\n");
            code.push_str("    Extension(service): Extension<H>,\n");

            // Path parameters
            if !op.http.path_params.is_empty() {
                let params_tuple = if op.http.path_params.len() == 1 {
                    "String".to_string()
                } else {
                    format!("({})", vec!["String"; op.http.path_params.len()].join(", "))
                };
                code.push_str(&format!("    Path(path_params): Path<{}>,\n", params_tuple));
            }

            // Query parameters
            if !op.http.query.is_empty() {
                code.push_str(&format!("    Query(query): Query<{}Query>,\n", op_name));
            }

            // Body
            if let Some(body) = &op.http.body {
                if let Some(variant) = body.variants.first() {
                    code.push_str(&format!(
                        "    Json(body): Json<types::{}>,\n",
                        variant.ty.target.0
                    ));
                }
            }

            code.push_str("    req: Request<axum::body::Body>,\n");
            code.push_str(") -> Response\n");
            code.push_str("where\n");
            code.push_str("    S: Clone + Send + Sync + 'static,\n");
            code.push_str(&format!(
                "    H: {}<S> + Clone + Send + Sync + 'static,\n",
                trait_name
            ));
            code.push_str("{\n");

            // Handler body
            code.push_str("    let (parts, _body) = req.into_parts();\n");
            code.push_str("    let ctx = RequestContext::from_parts(state, parts);\n\n");

            // Extract path parameters
            if !op.http.path_params.is_empty() {
                if op.http.path_params.len() == 1 {
                    code.push_str(&format!(
                        "    let {} = path_params;\n",
                        op.http.path_params[0].name.snake
                    ));
                } else {
                    code.push_str("    let (");
                    for (i, param) in op.http.path_params.iter().enumerate() {
                        if i > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&param.name.snake);
                    }
                    code.push_str(") = path_params;\n");
                }
                code.push('\n');
            }

            // Call service method
            code.push_str(&format!("    match service.{}(\n", op_snake));
            code.push_str("        ctx,\n");

            for param in &op.http.path_params {
                code.push_str(&format!("        {},\n", param.name.snake));
            }

            if !op.http.query.is_empty() {
                code.push_str("        query,\n");
            }

            if op.http.body.is_some() {
                code.push_str("        body,\n");
            }

            code.push_str("    ).await {\n");

            // Handle success
            let status_code = if let Some(success) = &op.success {
                match &success.status {
                    ir::gen_ir::StatusSpec::Code(c) => *c,
                    _ => 200,
                }
            } else {
                200
            };

            code.push_str("        Ok(result) => {\n");
            code.push_str(&format!(
                "            let status = StatusCode::from_u16({}).unwrap_or(StatusCode::OK);\n",
                status_code
            ));

            if op.success.is_some() && op.success.as_ref().unwrap().ty.is_some() {
                code.push_str("            (status, Json(result)).into_response()\n");
            } else {
                code.push_str("            status.into_response()\n");
            }

            code.push_str("        }\n");
            code.push_str("        Err(e) => e.into_response(),\n");
            code.push_str("    }\n");
            code.push_str("}\n\n");
        }

        code
    }

    fn generate_router(&self) -> String {
        let mut code = String::new();
        let trait_name = &self.service.name.pascal;

        code.push_str("/// Create a router for this service\n");
        code.push_str("pub fn router<S, H>(service: H) -> Router<S>\n");
        code.push_str("where\n");
        code.push_str("    S: Clone + Send + Sync + 'static,\n");
        code.push_str(&format!(
            "    H: {}<S> + Clone + Send + Sync + 'static,\n",
            trait_name
        ));
        code.push_str("{\n");
        code.push_str("    Router::new()\n");

        for op in &self.service.operations {
            let method_fn = match op.http.method {
                HttpMethod::Get => "get",
                HttpMethod::Post => "post",
                HttpMethod::Put => "put",
                HttpMethod::Delete => "delete",
                HttpMethod::Patch => "patch",
                HttpMethod::Head => "head",
                HttpMethod::Options => "options",
                HttpMethod::Trace => "trace",
            };

            code.push_str(&format!(
                "        .route(\"{}\", {}({}_handler::<S, H>))\n",
                op.http.path_template, method_fn, op.name.snake
            ));
        }

        code.push_str("        .layer(Extension(service))\n");
        code.push_str("}\n\n");

        code
    }

    fn generate_extension_trait(&self) -> String {
        let trait_name = &self.service.name.pascal;

        format!(
            r#"/// Extension trait for ergonomic router creation
pub trait {}RouterExt<S>: {}<S> + Clone + Send + Sync + 'static
where
    S: Clone + Send + Sync + 'static,
{{
    fn router(self) -> Router<S> {{
        router::<S, Self>(self)
    }}
}}

impl<S, T> {}RouterExt<S> for T
where
    S: Clone + Send + Sync + 'static,
    T: {}<S> + Clone + Send + Sync + 'static,
{{
}}
"#,
            trait_name, trait_name, trait_name, trait_name
        )
    }
}
