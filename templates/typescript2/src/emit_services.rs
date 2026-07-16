//! Emit GenIR services as TypeScript AST modules.

use codegen::GenIr;
use ir::gen_ir::{ApiKeyLocation, AuthKind, ErrorUse, HttpMethod, Operation, Service, StatusSpec};

use crate::ast::*;
use crate::emit_types::type_ref_to_ts_type;
use crate::imports::ImportCollector;

/// Emit a single service file as a complete module.
pub fn emit_service(service: &Service, ir: &GenIr) -> Module {
    let mut imports = ImportCollector::new();
    let mut items: Vec<Item> = Vec::new();

    // Collect type imports from all operations
    let mut type_import_names = std::collections::BTreeSet::new();
    for op in &service.operations {
        collect_op_type_imports(op, ir, &mut type_import_names);
    }

    // Register type imports
    for name in &type_import_names {
        imports.add_type(name.clone(), "../types");
    }

    imports.add_value("UnexpectedError", "../types/errors");
    imports.add_type("SDKErrorContext", "./client");
    imports.add_type("SDKHooks", "./client");
    imports.add_type("SDKRequestInit", "./client");
    imports.add_type("SDKResponseInfo", "./client");

    if !ir.auth_schemes.is_empty() {
        imports.add_type("SecurityConfig", "./client");
    }

    // Generate error classes for each operation
    for op in &service.operations {
        let error_classes = emit_operation_errors(op, ir);
        items.extend(error_classes);
    }

    // Generate service class
    let class = emit_service_class(service, ir);
    items.push(Item::Class(class));

    Module {
        imports: imports.into_decls(),
        items,
    }
}

fn collect_op_type_imports(
    op: &Operation,
    ir: &GenIr,
    imports: &mut std::collections::BTreeSet<String>,
) {
    // Path params
    for param in &op.http.path_params {
        collect_type_ref_imports(&param.ty, ir, imports);
    }
    // Query params
    for param in &op.http.query {
        collect_type_ref_imports(&param.ty, ir, imports);
    }
    // Header params
    for param in &op.http.headers {
        collect_type_ref_imports(&param.ty, ir, imports);
    }
    // Body
    if let Some(body) = &op.http.body
        && let Some(variant) = body.variants.first()
    {
        collect_type_ref_imports(&variant.ty, ir, imports);
    }
    // Success response
    if let Some(success) = &op.success
        && let Some(ty) = &success.ty
    {
        collect_type_ref_imports(ty, ir, imports);
    }
    // Error body types
    if let ErrorUse::Inline(error_decl) = &op.errors {
        for v in &error_decl.variants {
            if let Some(ty) = &v.ty {
                collect_type_ref_imports(ty, ir, imports);
            }
        }
    }
}

fn collect_type_ref_imports(
    type_ref: &ir::gen_ir::TypeRef,
    ir: &GenIr,
    imports: &mut std::collections::BTreeSet<String>,
) {
    if let Some(type_decl) = ir.types.get(&type_ref.target) {
        imports.insert(type_decl.name.pascal.clone());
    }
    for modifier in &type_ref.modifiers {
        if let ir::gen_ir::TypeMod::Map(value_type) = modifier {
            collect_type_ref_imports(value_type, ir, imports);
        }
    }
}

fn emit_operation_errors(op: &Operation, ir: &GenIr) -> Vec<Item> {
    let mut items = Vec::new();

    let error_variants = match &op.errors {
        ErrorUse::Inline(error_decl) => &error_decl.variants,
        _ => return items,
    };

    for v in error_variants {
        let status_code = match &v.status {
            StatusSpec::Code(code) => *code,
            _ => continue,
        };

        let class_name = format!("{}{}Error", op.name.pascal, v.name.pascal);
        let has_body = v.ty.is_some();
        let body_type =
            v.ty.as_ref()
                .map(|ty| type_ref_to_ts_type(ty, ir))
                .unwrap_or(TsType::Void);

        let mut members = Vec::new();

        // readonly status = <code>;
        members.push(ClassMember::Field(
            ClassField::new("status")
                .readonly()
                .value(Expr::num(status_code.to_string())),
        ));

        if has_body {
            members.push(ClassMember::Field(
                ClassField::new("body").readonly().typed(body_type.clone()),
            ));

            members.push(ClassMember::BlankLine);

            members.push(ClassMember::Constructor(ConstructorDecl {
                params: vec![Param::new("body", body_type)],
                body: vec![
                    Expr::call(
                        "super",
                        vec![Expr::TemplateLit(vec![TemplPart::Str(class_name.clone())])],
                    )
                    .into_stmt(),
                    Stmt::assign(Expr::this_dot("name"), Expr::str(class_name.clone())),
                    Stmt::assign(Expr::this_dot("body"), Expr::id("body")),
                ],
            }));
        } else {
            members.push(ClassMember::BlankLine);

            members.push(ClassMember::Constructor(ConstructorDecl {
                params: vec![],
                body: vec![
                    Expr::call(
                        "super",
                        vec![Expr::TemplateLit(vec![TemplPart::Str(class_name.clone())])],
                    )
                    .into_stmt(),
                    Stmt::assign(Expr::this_dot("name"), Expr::str(class_name.clone())),
                ],
            }));
        }

        items.push(Item::BlankLine);
        items.push(Item::Class(ClassDecl {
            name: class_name.clone(),
            exported: true,
            docs: Some(JsDoc {
                lines: vec![JsDocLine::Text(format!(
                    "Error thrown when {} returns status {}.",
                    op.name.camel, status_code
                ))],
            }),
            extends: Some("globalThis.Error".into()),
            members,
        }));
    }

    items
}

fn emit_service_class(service: &Service, ir: &GenIr) -> ClassDecl {
    let has_auth = !ir.auth_schemes.is_empty();

    let mut members = Vec::new();

    // Constructor
    let mut ctor_params = vec![Param::new("baseUrl", TsType::string()).private()];
    if has_auth {
        ctor_params.push(Param::new("security", TsType::named("SecurityConfig")).private());
    }
    ctor_params.push(Param::new("hooks", TsType::named("SDKHooks")).private());

    members.push(ClassMember::Constructor(ConstructorDecl {
        params: ctor_params,
        body: vec![],
    }));

    members.push(ClassMember::BlankLine);

    // Private raise method
    members.push(ClassMember::Method(MethodDecl {
        is_async: true,
        name: "raise".into(),
        visibility: Some(Visibility::Private),
        params: vec![
            Param::new("error", TsType::Named("globalThis.Error".into())),
            Param::new("context", TsType::named("SDKErrorContext")),
        ],
        return_type: Some(TsType::promise(TsType::Named("never".into()))),
        docs: None,
        body: vec![
            Expr::Raw("this.hooks.onError?.".into())
                .call_with(vec![Expr::id("error"), Expr::id("context")])
                .awaited()
                .into_stmt(),
            Stmt::Throw(Expr::id("error")),
        ],
    }));

    // Operations
    for op in &service.operations {
        members.push(ClassMember::BlankLine);
        members.push(emit_operation_method(op, ir));
    }

    let docs = if service.docs.summary.is_some() || service.docs.description.is_some() {
        let mut lines = Vec::new();
        if let Some(s) = &service.docs.summary {
            lines.push(JsDocLine::Text(s.clone()));
        }
        if service.docs.summary.is_some() && service.docs.description.is_some() {
            lines.push(JsDocLine::Text(String::new()));
        }
        if let Some(d) = &service.docs.description {
            lines.push(JsDocLine::Text(d.clone()));
        }
        Some(JsDoc { lines })
    } else {
        None
    };

    ClassDecl {
        name: format!("{}Service", service.name.pascal),
        exported: true,
        docs,
        extends: None,
        members,
    }
}

fn emit_operation_method(op: &Operation, ir: &GenIr) -> ClassMember {
    let mut all_params = Vec::new();
    let mut body_stmts = Vec::new();

    // Collect params
    for param in &op.http.path_params {
        all_params.push((
            param.name.camel.clone(),
            type_ref_to_ts_type(&param.ty, ir),
            false,
            param.docs.summary.clone(),
        ));
    }
    for param in &op.http.query {
        all_params.push((
            param.name.camel.clone(),
            type_ref_to_ts_type(&param.ty, ir),
            !param.required,
            param.docs.summary.clone(),
        ));
    }
    for param in &op.http.headers {
        all_params.push((
            param.name.camel.clone(),
            type_ref_to_ts_type(&param.ty, ir),
            !param.required,
            param.docs.summary.clone(),
        ));
    }
    if let Some(body) = &op.http.body
        && let Some(variant) = body.variants.first()
    {
        all_params.push((
            "body".into(),
            type_ref_to_ts_type(&variant.ty, ir),
            false,
            None,
        ));
    }

    let has_params = !all_params.is_empty();
    let has_body = op.http.body.is_some();

    // Return type
    let return_type = if let Some(success) = &op.success {
        if let Some(ty) = &success.ty {
            type_ref_to_ts_type(ty, ir)
        } else {
            TsType::Void
        }
    } else {
        TsType::Void
    };
    let has_return = !matches!(return_type, TsType::Void);

    let http_method = match op.http.method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Head => "HEAD",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Trace => "TRACE",
    };

    // Path construction
    if !op.http.path_params.is_empty() {
        body_stmts.push(Stmt::Raw(format!(
            "const path = `{}`{};",
            op.http.path_template,
            op.http
                .path_params
                .iter()
                .map(|p| format!(
                    ".replace('{{{}}}', String(params.{}))",
                    p.wire, p.name.camel
                ))
                .collect::<Vec<_>>()
                .join("")
        )));
    } else {
        body_stmts.push(Stmt::const_decl(
            "path",
            Expr::str(op.http.path_template.clone()),
        ));
    }

    // Query params
    if !op.http.query.is_empty() {
        body_stmts.push(Stmt::const_decl(
            "queryParams",
            Expr::new_expr("URLSearchParams", vec![]),
        ));

        for param in &op.http.query {
            body_stmts.push(Stmt::If {
                cond: Expr::id("params")
                    .dot(param.name.camel.clone())
                    .op("!==", Expr::id("undefined")),
                then: vec![
                    Expr::id("queryParams")
                        .method(
                            "append",
                            vec![
                                Expr::str(param.wire.clone()),
                                Expr::call(
                                    "String",
                                    vec![Expr::id("params").dot(param.name.camel.clone())],
                                ),
                            ],
                        )
                        .into_stmt(),
                ],
                else_: None,
            });
        }

        body_stmts.push(Stmt::const_decl(
            "queryString",
            Expr::id("queryParams").method("toString", vec![]),
        ));
        body_stmts.push(Stmt::Raw(
            "const url = queryString ? `${this.baseUrl}${path}?${queryString}` : `${this.baseUrl}${path}`;".into()
        ));
    } else {
        body_stmts.push(Stmt::Raw("const url = `${this.baseUrl}${path}`;".into()));
    }

    // Headers
    body_stmts.push(Stmt::BlankLine);
    body_stmts.push(Stmt::const_typed(
        "headers",
        TsType::record(TsType::string(), TsType::string()),
        Expr::Object(vec![]),
    ));

    if has_body {
        body_stmts.push(Stmt::Raw(
            "headers['Content-Type'] = 'application/json';".into(),
        ));
    }

    for param in &op.http.headers {
        body_stmts.push(Stmt::Raw(format!(
            "headers['{}'] = String(params.{});",
            param.wire, param.name.camel
        )));
    }

    // Auth
    for auth_use in &op.auth {
        if let Some(scheme) = ir.auth_schemes.iter().find(|s| s.id == auth_use.scheme) {
            match &scheme.kind {
                AuthKind::Http {
                    scheme: http_scheme,
                    ..
                } if http_scheme == "bearer" => {
                    body_stmts.push(Stmt::If {
                        cond: Expr::id("this.security").dot(scheme.name.camel.clone()),
                        then: vec![Stmt::Raw(format!(
                            "headers['Authorization'] = `Bearer ${{this.security.{}}}`;",
                            scheme.name.camel
                        ))],
                        else_: None,
                    });
                }
                AuthKind::ApiKey {
                    location: ApiKeyLocation::Header,
                    param_name,
                } => {
                    body_stmts.push(Stmt::If {
                        cond: Expr::id("this.security").dot(scheme.name.camel.clone()),
                        then: vec![Stmt::Raw(format!(
                            "headers['{}'] = this.security.{};",
                            param_name, scheme.name.camel
                        ))],
                        else_: None,
                    });
                }
                _ => {}
            }
        }
    }

    // Build request
    body_stmts.push(Stmt::BlankLine);

    let mut request_entries = vec![
        ObjEntry::kv("method", Expr::str(http_method)),
        ObjEntry::short("url"),
        ObjEntry::short("headers"),
    ];
    if has_body {
        request_entries.push(ObjEntry::kv(
            "body",
            Expr::id("JSON")
                .dot("stringify")
                .call_with(vec![Expr::id("params").dot("body")]),
        ));
    }
    body_stmts.push(Stmt::const_typed(
        "request",
        TsType::named("SDKRequestInit"),
        Expr::Object(request_entries),
    ));

    body_stmts.push(
        Expr::Raw("this.hooks.onRequest?.".into())
            .call_with(vec![Expr::id("request")])
            .awaited()
            .into_stmt(),
    );

    // Safe request context and fetch call
    body_stmts.push(Stmt::BlankLine);
    body_stmts.push(Stmt::const_typed(
        "requestInfo",
        TsType::Generic(
            "Omit".into(),
            vec![
                TsType::named("SDKRequestInit"),
                TsType::Literal("'body'".into()),
            ],
        ),
        request_info_expr(),
    ));

    let mut fetch_opts = vec![
        ObjEntry::kv("method", Expr::id("request").dot("method")),
        ObjEntry::kv("headers", Expr::id("request").dot("headers")),
    ];
    if has_body {
        fetch_opts.push(ObjEntry::kv("body", Expr::id("request").dot("body")));
    }
    body_stmts.push(Stmt::VarDecl {
        kind: VarKind::Let,
        name: "response".into(),
        ty: Some(TsType::named("Response")),
        init: None,
    });
    body_stmts.push(Stmt::TryCatch {
        try_body: vec![Stmt::assign(
            Expr::id("response"),
            Expr::call(
                "fetch",
                vec![Expr::id("request").dot("url"), Expr::Object(fetch_opts)],
            )
            .awaited(),
        )],
        catch_param: "error".into(),
        catch_body: vec![
            Expr::Raw("this.hooks.onError?.".into())
                .call_with(vec![
                    Expr::id("error"),
                    Expr::Object(vec![ObjEntry::kv("request", Expr::id("requestInfo"))]),
                ])
                .awaited()
                .into_stmt(),
            Stmt::Throw(Expr::id("error")),
        ],
    });

    // onResponse hook
    body_stmts.push(Stmt::BlankLine);
    body_stmts.push(Stmt::const_typed(
        "responseInfo",
        TsType::named("SDKResponseInfo"),
        response_info_expr(),
    ));
    body_stmts.push(
        Expr::Raw("this.hooks.onResponse?.".into())
            .call_with(vec![Expr::id("responseInfo")])
            .awaited()
            .into_stmt(),
    );

    body_stmts.push(Stmt::const_typed(
        "errorContext",
        TsType::named("SDKErrorContext"),
        Expr::Object(vec![
            ObjEntry::kv("request", Expr::id("requestInfo")),
            ObjEntry::kv("response", Expr::id("responseInfo")),
        ]),
    ));

    // Error handling
    body_stmts.push(Stmt::BlankLine);

    let error_variants: Vec<_> = match &op.errors {
        ErrorUse::Inline(error_decl) => error_decl
            .variants
            .iter()
            .filter_map(|v| {
                let code = match &v.status {
                    StatusSpec::Code(c) => *c,
                    _ => return None,
                };
                Some((
                    format!("{}{}Error", op.name.pascal, v.name.pascal),
                    code,
                    v.ty.is_some(),
                    v.ty.as_ref()
                        .map(|ty| crate::lower::render_ts_type(&type_ref_to_ts_type(ty, ir)))
                        .unwrap_or_else(|| "void".into()),
                ))
            })
            .collect(),
        _ => vec![],
    };

    let unexpected_error = Expr::id("this")
        .method(
            "raise",
            vec![
                Expr::new_expr(
                    "UnexpectedError",
                    vec![
                        Expr::id("response").dot("status"),
                        Expr::id("response").method("text", vec![]).awaited(),
                    ],
                ),
                Expr::id("errorContext"),
            ],
        )
        .awaited()
        .into_stmt();

    if !error_variants.is_empty() {
        let cases: Vec<SwitchCase> = error_variants
            .iter()
            .map(|(class_name, code, has_body, body_type)| {
                let case_body = if *has_body {
                    vec![Stmt::TryCatch {
                        try_body: vec![
                            Stmt::const_decl(
                                "body",
                                Expr::id("response")
                                    .method("json", vec![])
                                    .awaited()
                                    .as_type(TsType::Named(body_type.clone())),
                            ),
                            Expr::id("this")
                                .method(
                                    "raise",
                                    vec![
                                        Expr::new_expr(class_name.clone(), vec![Expr::id("body")]),
                                        Expr::id("errorContext"),
                                    ],
                                )
                                .awaited()
                                .into_stmt(),
                        ],
                        catch_param: "e".into(),
                        catch_body: vec![
                            Stmt::If {
                                cond: Expr::Instanceof(Box::new(Expr::id("e")), class_name.clone()),
                                then: vec![Stmt::Throw(Expr::id("e"))],
                                else_: None,
                            },
                            Expr::id("this")
                                .method(
                                    "raise",
                                    vec![
                                        Expr::new_expr(
                                            "UnexpectedError",
                                            vec![
                                                Expr::id("response").dot("status"),
                                                Expr::id("response")
                                                    .method("text", vec![])
                                                    .awaited(),
                                            ],
                                        ),
                                        Expr::id("errorContext"),
                                    ],
                                )
                                .awaited()
                                .into_stmt(),
                        ],
                    }]
                } else {
                    vec![
                        Expr::id("this")
                            .method(
                                "raise",
                                vec![
                                    Expr::new_expr(class_name.clone(), vec![]),
                                    Expr::id("errorContext"),
                                ],
                            )
                            .awaited()
                            .into_stmt(),
                    ]
                };

                SwitchCase {
                    value: Expr::num(code.to_string()),
                    body: case_body,
                }
            })
            .collect();

        body_stmts.push(Stmt::If {
            cond: Expr::Raw("!response.ok".into()),
            then: vec![Stmt::Switch {
                expr: Expr::id("response").dot("status"),
                cases,
                default: Some(vec![unexpected_error.clone()]),
            }],
            else_: None,
        });
    } else {
        body_stmts.push(Stmt::If {
            cond: Expr::Raw("!response.ok".into()),
            then: vec![unexpected_error],
            else_: None,
        });
    }

    // Return
    body_stmts.push(Stmt::BlankLine);
    if has_return {
        body_stmts.push(Stmt::Return(Some(
            Expr::id("response").method("json", vec![]),
        )));
    } else {
        body_stmts.push(Stmt::Return(None));
    }

    // Build JSDoc
    let mut jsdoc_lines = Vec::new();
    if let Some(summary) = &op.docs.summary {
        jsdoc_lines.push(JsDocLine::Text(summary.clone()));
    }
    if op.docs.summary.is_some() && op.docs.description.is_some() {
        jsdoc_lines.push(JsDocLine::Text(String::new()));
    }
    if let Some(desc) = &op.docs.description {
        jsdoc_lines.push(JsDocLine::Text(desc.clone()));
    }
    if !all_params.is_empty() && !jsdoc_lines.is_empty() {
        jsdoc_lines.push(JsDocLine::Text(String::new()));
    }
    for (name, _, _, docs) in &all_params {
        if let Some(doc) = docs {
            jsdoc_lines.push(JsDocLine::Param {
                name: name.clone(),
                desc: doc.clone(),
            });
        }
    }
    if !error_variants.is_empty() {
        let throws_str = error_variants
            .iter()
            .map(|(name, _, _, _)| name.clone())
            .chain(std::iter::once("UnexpectedError".into()))
            .collect::<Vec<_>>()
            .join(" | ");
        jsdoc_lines.push(JsDocLine::Throws(throws_str));
    }

    let docs = if jsdoc_lines.is_empty() {
        None
    } else {
        Some(JsDoc { lines: jsdoc_lines })
    };

    // Build params type
    let method_params = if has_params {
        let param_fields: Vec<InterfaceField> = all_params
            .iter()
            .map(|(name, ty, optional, _)| {
                let field = InterfaceField::new(name.clone(), ty.clone());
                if *optional { field.optional() } else { field }
            })
            .collect();

        vec![Param::new("params", TsType::Object(param_fields))]
    } else {
        vec![]
    };

    let return_ts = if has_return {
        TsType::promise(return_type)
    } else {
        TsType::promise(TsType::Void)
    };

    ClassMember::Method(MethodDecl {
        is_async: true,
        name: op.name.camel.clone(),
        visibility: None,
        params: method_params,
        return_type: Some(return_ts),
        docs,
        body: body_stmts,
    })
}

fn request_info_expr() -> Expr {
    Expr::Object(vec![
        ObjEntry::kv("method", Expr::id("request").dot("method")),
        ObjEntry::kv("url", Expr::id("request").dot("url")),
        ObjEntry::kv("headers", Expr::id("request").dot("headers")),
    ])
}

fn response_info_expr() -> Expr {
    Expr::Object(vec![
        ObjEntry::kv("method", Expr::id("request").dot("method")),
        ObjEntry::kv("url", Expr::id("request").dot("url")),
        ObjEntry::kv("status", Expr::id("response").dot("status")),
        ObjEntry::kv("headers", Expr::id("response").dot("headers")),
    ])
}
