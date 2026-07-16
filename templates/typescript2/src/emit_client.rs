//! Emit SDK client and index modules.

use codegen::GenIr;

use crate::ast::*;
use crate::imports::ImportCollector;

/// Emit the services/client.ts module.
#[allow(clippy::vec_init_then_push)]
pub fn emit_client(ir: &GenIr) -> Module {
    let mut imports = ImportCollector::new();
    let has_auth = !ir.auth_schemes.is_empty();

    for service in &ir.services {
        imports.add_value(
            format!("{}Service", service.name.pascal),
            format!("./{}", service.name.snake),
        );
    }

    let mut items = Vec::new();

    // SDKRequestInit interface
    items.push(Item::Interface(InterfaceDecl {
        name: "SDKRequestInit".into(),
        exported: true,
        docs: Some(JsDoc {
            lines: vec![
                JsDocLine::Text(
                    "Describes the outgoing request. Mutate any field in `onRequest` to".into(),
                ),
                JsDocLine::Text(
                    "alter the request before it is sent (e.g. add headers, rewrite URL).".into(),
                ),
            ],
        }),
        fields: vec![
            InterfaceField::new("method", TsType::string()),
            InterfaceField::new("url", TsType::string()),
            InterfaceField::new(
                "headers",
                TsType::record(TsType::string(), TsType::string()),
            ),
            InterfaceField::new("body", TsType::string()).optional(),
        ],
    }));

    items.push(Item::BlankLine);

    // SDKResponseInfo interface
    items.push(Item::Interface(InterfaceDecl {
        name: "SDKResponseInfo".into(),
        exported: true,
        docs: Some(JsDoc {
            lines: vec![JsDocLine::Text(
                "Read-only snapshot of the response returned by `fetch`.".into(),
            )],
        }),
        fields: vec![
            InterfaceField::new("method", TsType::string()),
            InterfaceField::new("url", TsType::string()),
            InterfaceField::new("status", TsType::number()),
            InterfaceField::new("headers", TsType::named("Headers")),
        ],
    }));

    items.push(Item::BlankLine);

    // SDKErrorContext interface
    items.push(Item::Interface(InterfaceDecl {
        name: "SDKErrorContext".into(),
        exported: true,
        docs: Some(JsDoc::from_text(
            "Safe request and optional response details supplied to `onError`.",
        )),
        fields: vec![
            InterfaceField::new(
                "request",
                TsType::Generic(
                    "Omit".into(),
                    vec![
                        TsType::named("SDKRequestInit"),
                        TsType::Literal("'body'".into()),
                    ],
                ),
            ),
            InterfaceField::new("response", TsType::named("SDKResponseInfo")).optional(),
        ],
    }));

    items.push(Item::BlankLine);

    // SDKHooks interface
    items.push(Item::Interface(InterfaceDecl {
        name: "SDKHooks".into(),
        exported: true,
        docs: Some(JsDoc {
            lines: vec![
                JsDocLine::Text("Hooks for intercepting SDK lifecycle events.".into()),
                JsDocLine::Text("All hooks are optional.".into()),
            ],
        }),
        fields: vec![
            InterfaceField::new("onRequest", TsType::Function {
                params: vec![("request".into(), TsType::named("SDKRequestInit"))],
                ret: Box::new(TsType::union(vec![TsType::Void, TsType::promise(TsType::Void)])),
            })
            .optional()
            .docs(JsDoc::from_text(
                "Called before each request is sent. Mutate `request` to add headers, rewrite the URL, etc.",
            )),
            InterfaceField::new("onResponse", TsType::Function {
                params: vec![("response".into(), TsType::named("SDKResponseInfo"))],
                ret: Box::new(TsType::union(vec![TsType::Void, TsType::promise(TsType::Void)])),
            })
            .optional()
            .docs(JsDoc::from_text(
                "Called after each response is received, before the body is parsed.",
            )),
            InterfaceField::new("onError", TsType::Function {
                params: vec![
                    ("error".into(), TsType::Named("unknown".into())),
                    ("context".into(), TsType::named("SDKErrorContext")),
                ],
                ret: Box::new(TsType::union(vec![TsType::Void, TsType::promise(TsType::Void)])),
            })
            .optional()
            .docs(JsDoc::from_text(
                "Called when any API call results in an error, before the error is thrown.",
            )),
        ],
    }));

    items.push(Item::BlankLine);

    // SecurityConfig interface (if there are auth schemes)
    if has_auth {
        let auth_fields: Vec<InterfaceField> = ir
            .auth_schemes
            .iter()
            .map(|scheme| {
                let field =
                    InterfaceField::new(scheme.name.camel.clone(), TsType::string()).optional();
                if let Some(d) = &scheme.docs.description {
                    field.docs(JsDoc::from_text(d.clone()))
                } else {
                    field
                }
            })
            .collect();

        items.push(Item::Interface(InterfaceDecl {
            name: "SecurityConfig".into(),
            exported: true,
            docs: Some(JsDoc::from_text("Security configuration for the SDK")),
            fields: auth_fields,
        }));

        items.push(Item::BlankLine);
    }

    // SDK Options interface
    let sdk_name = format!("{}SDK", ir.api.package_name.pascal);
    let opts_name = format!("{}Options", sdk_name);

    let default_base_url = ir
        .server_sets
        .first()
        .and_then(|ss| ss.urls.first())
        .map(|u| u.resolved_preview.clone())
        .unwrap_or_else(|| "https://api.example.com".into());

    let mut opts_fields = vec![
        InterfaceField::new("baseUrl", TsType::string())
            .optional()
            .docs(JsDoc::from_text(format!(
                "Base URL for API requests. Defaults to '{}'.",
                default_base_url
            ))),
    ];

    if has_auth {
        opts_fields.push(
            InterfaceField::new("security", TsType::named("SecurityConfig"))
                .optional()
                .docs(JsDoc::from_text("Security credentials.")),
        );
    }

    opts_fields.push(
        InterfaceField::new("hooks", TsType::named("SDKHooks"))
            .optional()
            .docs(JsDoc::from_text("Lifecycle hooks.")),
    );

    items.push(Item::Interface(InterfaceDecl {
        name: opts_name.clone(),
        exported: true,
        docs: Some(JsDoc::from_text("Options for constructing the SDK client.")),
        fields: opts_fields,
    }));

    items.push(Item::BlankLine);

    // SDK class
    let mut class_members = Vec::new();

    // Private service fields
    for service in &ir.services {
        class_members.push(ClassMember::Field(
            ClassField::new(format!("_{}", service.name.camel))
                .private()
                .typed(TsType::union(vec![
                    TsType::named(format!("{}Service", service.name.pascal)),
                    TsType::Named("undefined".into()),
                ])),
        ));
    }

    class_members.push(ClassMember::BlankLine);

    // Private fields
    class_members.push(ClassMember::Field(
        ClassField::new("baseUrl")
            .readonly()
            .private()
            .typed(TsType::string()),
    ));

    if has_auth {
        class_members.push(ClassMember::Field(
            ClassField::new("security")
                .readonly()
                .private()
                .typed(TsType::named("SecurityConfig")),
        ));
    }

    class_members.push(ClassMember::Field(
        ClassField::new("hooks")
            .readonly()
            .private()
            .typed(TsType::named("SDKHooks")),
    ));

    class_members.push(ClassMember::BlankLine);

    // Constructor
    let mut ctor_body = vec![Stmt::Raw(format!(
        "this.baseUrl = options.baseUrl ?? '{}';",
        default_base_url
    ))];
    if has_auth {
        ctor_body.push(Stmt::Raw("this.security = options.security ?? {};".into()));
    }
    ctor_body.push(Stmt::Raw("this.hooks = options.hooks ?? {};".into()));

    class_members.push(ClassMember::Constructor(ConstructorDecl {
        params: vec![Param::new("options", TsType::named(&opts_name))],
        body: ctor_body,
    }));

    // Service getters
    for service in &ir.services {
        let getter_docs = service
            .docs
            .summary
            .as_ref()
            .map(|s| JsDoc::from_text(s.clone()));

        let private_field = format!("this._{}", service.name.camel);
        let service_class = format!("{}Service", service.name.pascal);

        let mut new_args = vec![Expr::this_dot("baseUrl")];
        if has_auth {
            new_args.push(Expr::this_dot("security"));
        }
        new_args.push(Expr::this_dot("hooks"));

        class_members.push(ClassMember::BlankLine);
        class_members.push(ClassMember::Getter(GetterDecl {
            name: service.name.camel.clone(),
            visibility: None,
            return_type: Some(TsType::named(&service_class)),
            docs: getter_docs,
            body: vec![
                Stmt::If {
                    cond: Expr::Raw(format!("!{}", private_field)),
                    then: vec![Stmt::Raw(format!(
                        "{} = new {}({});",
                        private_field,
                        service_class,
                        new_args
                            .iter()
                            .map(crate::lower::render_expr)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))],
                    else_: None,
                },
                Stmt::Return(Some(Expr::Raw(private_field.clone()))),
            ],
        }));
    }

    let class_docs = if let Some(summary) = &ir.api.docs.summary {
        let mut lines = vec![JsDocLine::Text(summary.clone())];
        if let Some(desc) = &ir.api.docs.description {
            lines.push(JsDocLine::Text(String::new()));
            lines.push(JsDocLine::Text(desc.clone()));
        }
        lines.push(JsDocLine::Text(String::new()));
        lines.push(JsDocLine::Text(format!("@version {}", ir.api.version)));
        Some(JsDoc { lines })
    } else {
        None
    };

    items.push(Item::Class(ClassDecl {
        name: sdk_name,
        exported: true,
        docs: class_docs,
        extends: None,
        members: class_members,
    }));

    Module {
        imports: imports.into_decls(),
        items,
    }
}

/// Emit the root src/index.ts re-export module.
pub fn emit_sdk_index(ir: &GenIr) -> Module {
    let mut items = vec![
        Item::Comment("Generated SDK entry point".into()),
        Item::BlankLine,
        Item::ReExport("./types".into()),
        Item::ReExport("./types/errors".into()),
        Item::ReExport("./services/client".into()),
    ];

    for service in &ir.services {
        items.push(Item::NamedReExport {
            names: vec![format!("{}Service", service.name.pascal)],
            source: format!("./services/{}", service.name.snake),
        });
    }

    Module {
        imports: Vec::new(),
        items,
    }
}
