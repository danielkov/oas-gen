//! Lower TypeScript AST nodes to pretty-printer `Doc` nodes.

use std::rc::Rc;

use codegen::pretty::*;

use crate::ast::*;

/// Lower a complete module to a `Doc`.
pub fn lower_module(module: &Module) -> Rc<Doc> {
    let mut parts: Vec<Rc<Doc>> = Vec::new();

    // Imports
    if !module.imports.is_empty() {
        // Detect the boundary between third-party and local imports
        let mut prev_is_local: Option<bool> = None;
        for imp in &module.imports {
            let source = match imp {
                ImportDecl::Named { source, .. } | ImportDecl::TypeOnly { source, .. } => source,
            };
            let is_local = source.starts_with("./") || source.starts_with("../");

            if let Some(prev) = prev_is_local
                && !prev
                && is_local
            {
                // Insert blank line between third-party and local
                parts.push(hardline());
            }
            prev_is_local = Some(is_local);
            parts.push(lower_import(imp));
            parts.push(hardline());
        }
    }

    // Items
    let items: Vec<Rc<Doc>> = module.items.iter().map(lower_item).collect();

    if !module.imports.is_empty() && !items.is_empty() {
        // Already have a trailing hardline from imports
    }

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            parts.push(hardline());
        }
        parts.push(item.clone());
    }

    // Trailing newline
    parts.push(hardline());

    concat_all(parts)
}

fn lower_import(imp: &ImportDecl) -> Rc<Doc> {
    match imp {
        ImportDecl::Named { names, source } => {
            let names_str = names.join(", ");
            text(format!("import {{ {} }} from '{}';", names_str, source))
        }
        ImportDecl::TypeOnly { names, source } => {
            let names_str = names.join(", ");
            text(format!(
                "import type {{ {} }} from '{}';",
                names_str, source
            ))
        }
    }
}

fn lower_item(item: &Item) -> Rc<Doc> {
    match item {
        Item::Interface(decl) => lower_interface(decl),
        Item::TypeAlias(decl) => lower_type_alias(decl),
        Item::ConstDecl(decl) => lower_const_decl(decl),
        Item::Class(decl) => lower_class(decl),
        Item::ReExport(source) => text(format!("export * from '{}';", source)),
        Item::NamedReExport { names, source } => text(format!(
            "export {{ {} }} from '{}';",
            names.join(", "),
            source
        )),
        Item::BlankLine => nil(),
        Item::Comment(s) => text(format!("// {}", s)),
        Item::Raw(s) => text(s.clone()),
    }
}

fn lower_interface(decl: &InterfaceDecl) -> Rc<Doc> {
    let mut parts = Vec::new();

    if let Some(docs) = &decl.docs {
        parts.push(lower_jsdoc(docs));
        parts.push(hardline());
    }

    let prefix = if decl.exported {
        format!("export interface {} {{", decl.name)
    } else {
        format!("interface {} {{", decl.name)
    };

    if decl.fields.is_empty() {
        parts.push(text(format!("{}}}", prefix)));
        return concat_all(parts);
    }

    parts.push(text(prefix));

    for field in &decl.fields {
        parts.push(hardline());
        if let Some(docs) = &field.docs {
            parts.push(lower_field_jsdoc(docs));
            parts.push(hardline());
        }
        let opt = if field.optional { "?" } else { "" };
        let ty_str = render_ts_type(&field.ty);
        parts.push(text(format!("  {}{}: {};", field.name, opt, ty_str)));
    }

    parts.push(hardline());
    parts.push(text("}"));

    concat_all(parts)
}

fn lower_type_alias(decl: &TypeAliasDecl) -> Rc<Doc> {
    let mut parts = Vec::new();

    if let Some(docs) = &decl.docs {
        parts.push(lower_jsdoc(docs));
        parts.push(hardline());
    }

    let prefix = if decl.exported { "export " } else { "" };
    let ty_str = render_ts_type(&decl.ty);
    parts.push(text(format!("{}type {} = {};", prefix, decl.name, ty_str)));

    concat_all(parts)
}

fn lower_const_decl(decl: &ConstDecl) -> Rc<Doc> {
    let prefix = if decl.exported { "export " } else { "" };
    let ty_part = if let Some(ty) = &decl.ty_annotation {
        format!(": {}", render_ts_type(ty))
    } else {
        String::new()
    };

    let value_str = render_expr(&decl.value);
    text(format!(
        "{}const {}{} = {};",
        prefix, decl.name, ty_part, value_str
    ))
}

fn lower_class(decl: &ClassDecl) -> Rc<Doc> {
    let mut parts = Vec::new();

    if let Some(docs) = &decl.docs {
        parts.push(lower_jsdoc(docs));
        parts.push(hardline());
    }

    let prefix = if decl.exported { "export " } else { "" };
    let extends = if let Some(base) = &decl.extends {
        format!(" extends {}", base)
    } else {
        String::new()
    };
    parts.push(text(format!("{}class {}{} {{", prefix, decl.name, extends)));

    for (i, member) in decl.members.iter().enumerate() {
        match member {
            ClassMember::BlankLine => {
                parts.push(hardline());
            }
            _ => {
                // Add newline before member (always before first, and between non-blank members)
                if i == 0 || !matches!(decl.members.get(i - 1), Some(ClassMember::BlankLine)) {
                    parts.push(hardline());
                }
                parts.push(lower_class_member(member));
            }
        }
    }

    parts.push(hardline());
    parts.push(text("}"));

    concat_all(parts)
}

fn lower_class_member(member: &ClassMember) -> Rc<Doc> {
    match member {
        ClassMember::Field(f) => lower_class_field(f),
        ClassMember::Constructor(c) => lower_constructor(c),
        ClassMember::Method(m) => lower_method(m, true),
        ClassMember::Getter(g) => lower_getter(g),
        ClassMember::BlankLine => hardline(),
        ClassMember::Raw(s) => text(format!("  {}", s)),
    }
}

fn lower_class_field(f: &ClassField) -> Rc<Doc> {
    let vis = match f.visibility {
        Some(Visibility::Private) => "private ",
        Some(Visibility::Public) => "",
        None => "",
    };
    let ro = if f.readonly { "readonly " } else { "" };
    let ty_part = if let Some(ty) = &f.ty {
        format!(": {}", render_ts_type(ty))
    } else {
        String::new()
    };
    let val_part = if let Some(val) = &f.value {
        format!(" = {}", render_expr(val))
    } else {
        String::new()
    };
    text(format!("  {}{}{}{}{};", vis, ro, f.name, ty_part, val_part))
}

fn lower_constructor(c: &ConstructorDecl) -> Rc<Doc> {
    let params_str = c
        .params
        .iter()
        .map(render_param)
        .collect::<Vec<_>>()
        .join(", ");

    let mut parts = Vec::new();
    parts.push(text(format!("  constructor({}) {{", params_str)));

    for stmt in &c.body {
        parts.push(hardline());
        parts.push(indent_stmt(stmt, 4));
    }

    parts.push(hardline());
    parts.push(text("  }"));
    concat_all(parts)
}

fn lower_method(m: &MethodDecl, in_class: bool) -> Rc<Doc> {
    let indent = if in_class { 2 } else { 0 };
    let pad = " ".repeat(indent);
    let body_pad = " ".repeat(indent + 2);

    let mut parts = Vec::new();

    if let Some(docs) = &m.docs {
        parts.push(lower_member_jsdoc(docs, indent));
        parts.push(hardline());
    }

    let vis = match m.visibility {
        Some(Visibility::Private) => "private ",
        Some(Visibility::Public) => "",
        None => "",
    };
    let async_kw = if m.is_async { "async " } else { "" };
    let ret = if let Some(ty) = &m.return_type {
        format!(": {}", render_ts_type(ty))
    } else {
        String::new()
    };

    let params_str = m
        .params
        .iter()
        .map(render_param)
        .collect::<Vec<_>>()
        .join(", ");

    parts.push(text(format!(
        "{}{}{}{}({}){}",
        pad,
        vis,
        async_kw,
        m.name,
        params_str,
        if ret.is_empty() {
            " {".to_string()
        } else {
            format!("{} {{", ret)
        }
    )));

    for stmt in &m.body {
        parts.push(hardline());
        parts.push(indent_stmt(stmt, indent + 2));
        let _ = &body_pad; // suppress unused warning
    }

    parts.push(hardline());
    parts.push(text(format!("{}}}", pad)));
    concat_all(parts)
}

fn lower_getter(g: &GetterDecl) -> Rc<Doc> {
    let mut parts = Vec::new();

    if let Some(docs) = &g.docs {
        parts.push(lower_member_jsdoc(docs, 2));
        parts.push(hardline());
    }

    let vis = match g.visibility {
        Some(Visibility::Private) => "private ",
        Some(Visibility::Public) => "",
        None => "",
    };
    let ret = if let Some(ty) = &g.return_type {
        format!(": {}", render_ts_type(ty))
    } else {
        String::new()
    };

    parts.push(text(format!("  {}get {}(){} {{", vis, g.name, ret)));

    for stmt in &g.body {
        parts.push(hardline());
        parts.push(indent_stmt(stmt, 4));
    }

    parts.push(hardline());
    parts.push(text("  }"));
    concat_all(parts)
}

/// Re-indent a rendered string: lines after the first get `pad` prepended.
fn reindent_multiline(rendered: String, pad: &str) -> Rc<Doc> {
    if !rendered.contains('\n') {
        return text(rendered);
    }
    let mut parts = Vec::new();
    for (i, line) in rendered.lines().enumerate() {
        if i > 0 {
            parts.push(hardline());
        }
        if i == 0 || line.is_empty() {
            parts.push(text(line.to_string()));
        } else {
            parts.push(text(format!("{}{}", pad, line)));
        }
    }
    concat_all(parts)
}

fn indent_stmt(stmt: &Stmt, indent: usize) -> Rc<Doc> {
    let pad = " ".repeat(indent);
    match stmt {
        Stmt::BlankLine => nil(),
        Stmt::Raw(s) => {
            // Handle multi-line raw strings
            reindent_multiline(format!("{}{}", pad, s), &pad)
        }
        Stmt::VarDecl {
            kind,
            name,
            ty,
            init,
        } => {
            let kw = match kind {
                VarKind::Const => "const",
                VarKind::Let => "let",
            };
            let ty_part = if let Some(ty) = ty {
                format!(": {}", render_ts_type(ty))
            } else {
                String::new()
            };
            let init_part = if let Some(init) = init {
                format!(" = {}", render_expr(init))
            } else {
                String::new()
            };
            reindent_multiline(
                format!("{}{} {}{}{};", pad, kw, name, ty_part, init_part),
                &pad,
            )
        }
        Stmt::Expr(e) => reindent_multiline(format!("{}{};", pad, render_expr(e)), &pad),
        Stmt::Return(None) => text(format!("{}return;", pad)),
        Stmt::Return(Some(e)) => {
            reindent_multiline(format!("{}return {};", pad, render_expr(e)), &pad)
        }
        Stmt::Throw(e) => reindent_multiline(format!("{}throw {};", pad, render_expr(e)), &pad),
        Stmt::If {
            cond,
            then,
            else_: None,
        } => {
            let mut parts = Vec::new();
            parts.push(text(format!("{}if ({}) {{", pad, render_expr(cond))));
            for s in then {
                parts.push(hardline());
                parts.push(indent_stmt(s, indent + 2));
            }
            parts.push(hardline());
            parts.push(text(format!("{}}}", pad)));
            concat_all(parts)
        }
        Stmt::If {
            cond,
            then,
            else_: Some(else_body),
        } => {
            let mut parts = Vec::new();
            parts.push(text(format!("{}if ({}) {{", pad, render_expr(cond))));
            for s in then {
                parts.push(hardline());
                parts.push(indent_stmt(s, indent + 2));
            }
            parts.push(hardline());
            parts.push(text(format!("{}}} else {{", pad)));
            for s in else_body {
                parts.push(hardline());
                parts.push(indent_stmt(s, indent + 2));
            }
            parts.push(hardline());
            parts.push(text(format!("{}}}", pad)));
            concat_all(parts)
        }
        Stmt::Switch {
            expr,
            cases,
            default,
        } => {
            let mut parts = Vec::new();
            parts.push(text(format!("{}switch ({}) {{", pad, render_expr(expr))));
            for case in cases {
                parts.push(hardline());
                parts.push(text(format!(
                    "{}  case {}: {{",
                    pad,
                    render_expr(&case.value)
                )));
                for s in &case.body {
                    parts.push(hardline());
                    parts.push(indent_stmt(s, indent + 4));
                }
                parts.push(hardline());
                parts.push(text(format!("{}  }}", pad)));
            }
            if let Some(default_body) = default {
                parts.push(hardline());
                parts.push(text(format!("{}  default:", pad)));
                for s in default_body {
                    parts.push(hardline());
                    parts.push(indent_stmt(s, indent + 4));
                }
            }
            parts.push(hardline());
            parts.push(text(format!("{}}}", pad)));
            concat_all(parts)
        }
        Stmt::TryCatch {
            try_body,
            catch_param,
            catch_body,
        } => {
            let mut parts = Vec::new();
            parts.push(text(format!("{}try {{", pad)));
            for s in try_body {
                parts.push(hardline());
                parts.push(indent_stmt(s, indent + 2));
            }
            parts.push(hardline());
            parts.push(text(format!("{}}} catch ({}) {{", pad, catch_param)));
            for s in catch_body {
                parts.push(hardline());
                parts.push(indent_stmt(s, indent + 2));
            }
            parts.push(hardline());
            parts.push(text(format!("{}}}", pad)));
            concat_all(parts)
        }
    }
}

fn render_param(p: &Param) -> String {
    let vis = match p.visibility {
        Some(Visibility::Private) => "private ",
        Some(Visibility::Public) => "public ",
        None => "",
    };
    let ro = if p.readonly { "readonly " } else { "" };
    let opt = if p.optional { "?" } else { "" };
    let ty = if let Some(ty) = &p.ty {
        format!(": {}", render_ts_type(ty))
    } else {
        String::new()
    };
    format!("{}{}{}{}{}", vis, ro, p.name, opt, ty)
}

/// Render a TypeScript type to a string.
pub fn render_ts_type(ty: &TsType) -> String {
    match ty {
        TsType::Named(n) => n.clone(),
        TsType::Generic(name, args) => {
            let args_str = args
                .iter()
                .map(render_ts_type)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{}>", name, args_str)
        }
        TsType::Union(types) => types
            .iter()
            .map(render_ts_type)
            .collect::<Vec<_>>()
            .join(" | "),
        TsType::Array(inner) => format!("Array<{}>", render_ts_type(inner)),
        TsType::Literal(s) => s.clone(),
        TsType::Object(fields) => {
            if fields.is_empty() {
                return "{}".to_string();
            }
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f| {
                    let opt = if f.optional { "?" } else { "" };
                    format!("{}{}: {}", f.name, opt, render_ts_type(&f.ty))
                })
                .collect();
            format!("{{ {} }}", field_strs.join("; "))
        }
        TsType::Function { params, ret } => {
            let params_str = params
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, render_ts_type(ty)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({}) => {}", params_str, render_ts_type(ret))
        }
        TsType::Void => "void".to_string(),
    }
}

/// Render an expression to a string.
pub fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(s) => s.clone(),
        Expr::StringLit(s) => format!("'{}'", s.replace('\'', "\\'")),
        Expr::TemplateLit(parts) => {
            let mut out = String::from("`");
            for part in parts {
                match part {
                    TemplPart::Str(s) => out.push_str(s),
                    TemplPart::Expr(e) => {
                        out.push_str("${");
                        out.push_str(&render_expr(e));
                        out.push('}');
                    }
                }
            }
            out.push('`');
            out
        }
        Expr::NumberLit(n) => n.clone(),
        Expr::BoolLit(b) => b.to_string(),
        Expr::Null => "null".to_string(),
        Expr::Object(entries) => {
            if entries.is_empty() {
                return "{}".to_string();
            }
            let parts: Vec<String> = entries
                .iter()
                .map(|e| match e {
                    ObjEntry::KeyValue(k, v) => format!("{}: {}", k, render_expr(v)),
                    ObjEntry::Shorthand(k) => k.clone(),
                    ObjEntry::Spread(e) => format!("...{}", render_expr(e)),
                    ObjEntry::Getter(name, body) => {
                        let body_str = body
                            .iter()
                            .map(|s| render_stmt_inline(s, 2))
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!("get {}() {{\n{}\n}}", name, body_str)
                    }
                })
                .collect();
            // Use single-line if short enough, multi-line otherwise
            let single = format!("{{ {} }}", parts.join(", "));
            if single.len() <= 60 && !single.contains('\n') {
                single
            } else {
                format!(
                    "{{\n{}\n}}",
                    parts
                        .iter()
                        .map(|p| {
                            if p.contains('\n') {
                                // Getter or multi-line: indent each line, add trailing comma
                                let indented = p
                                    .lines()
                                    .map(|l| format!("  {}", l))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                format!("{},", indented)
                            } else {
                                format!("  {},", p)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }
        Expr::Array(items) => {
            let parts: Vec<String> = items.iter().map(render_expr).collect();
            format!("[{}]", parts.join(", "))
        }
        Expr::Member(obj, field) => format!("{}.{}", render_expr(obj), field),
        Expr::Index(obj, idx) => format!("{}[{}]", render_expr(obj), render_expr(idx)),
        Expr::Call(callee, args) => {
            let args_str = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{}({})", render_expr(callee), args_str)
        }
        Expr::MethodCall(obj, method, args) => {
            let args_str = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{}.{}({})", render_expr(obj), method, args_str)
        }
        Expr::New(callee, args) => {
            let args_str = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("new {}({})", render_expr(callee), args_str)
        }
        Expr::Await(inner) => format!("await {}", render_expr(inner)),
        Expr::Binary(l, op, r) => format!("{} {} {}", render_expr(l), op, render_expr(r)),
        Expr::As(inner, ty) => format!("{} as {}", render_expr(inner), render_ts_type(ty)),
        Expr::Instanceof(inner, ty) => format!("{} instanceof {}", render_expr(inner), ty),
        Expr::Ternary(cond, then, else_) => format!(
            "{} ? {} : {}",
            render_expr(cond),
            render_expr(then),
            render_expr(else_)
        ),
        Expr::Arrow { params, body } => {
            let params_str = if params.len() == 1 {
                params[0].clone()
            } else {
                format!("({})", params.join(", "))
            };
            match body {
                ArrowBody::Expr(e) => format!("{} => {}", params_str, render_expr(e)),
                ArrowBody::Block(stmts) => {
                    let body_str = stmts
                        .iter()
                        .map(|s| render_stmt_inline(s, 2))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("{} => {{\n{}\n}}", params_str, body_str)
                }
            }
        }
        Expr::AsConst(inner) => format!("{} as const", render_expr(inner)),
        Expr::Raw(s) => s.clone(),
    }
}

fn render_stmt_inline(stmt: &Stmt, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match stmt {
        Stmt::Return(Some(e)) => format!("{}return {};", pad, render_expr(e)),
        Stmt::Return(None) => format!("{}return;", pad),
        Stmt::Expr(e) => format!("{}{};", pad, render_expr(e)),
        _ => format!("{}/* unsupported stmt */", pad),
    }
}

fn lower_jsdoc(doc: &JsDoc) -> Rc<Doc> {
    if doc.lines.is_empty() {
        return nil();
    }

    let mut parts = Vec::new();
    parts.push(text("/**"));

    for line in &doc.lines {
        parts.push(hardline());
        parts.push(text(format!(" * {}", render_jsdoc_line(line))));
    }

    parts.push(hardline());
    parts.push(text(" */"));
    concat_all(parts)
}

fn lower_field_jsdoc(doc: &JsDoc) -> Rc<Doc> {
    if doc.lines.is_empty() {
        return nil();
    }

    let mut parts = Vec::new();
    parts.push(text("  /**"));

    for line in &doc.lines {
        parts.push(hardline());
        parts.push(text(format!("   * {}", render_jsdoc_line(line))));
    }

    parts.push(hardline());
    parts.push(text("   */"));
    concat_all(parts)
}

fn lower_member_jsdoc(doc: &JsDoc, indent: usize) -> Rc<Doc> {
    if doc.lines.is_empty() {
        return nil();
    }

    let pad = " ".repeat(indent);
    let mut parts = Vec::new();
    parts.push(text(format!("{}/**", pad)));

    for line in &doc.lines {
        parts.push(hardline());
        parts.push(text(format!("{} * {}", pad, render_jsdoc_line(line))));
    }

    parts.push(hardline());
    parts.push(text(format!("{} */", pad)));
    concat_all(parts)
}

fn render_jsdoc_line(line: &JsDocLine) -> String {
    match line {
        JsDocLine::Text(s) => s.clone(),
        JsDocLine::Param { name, desc } if desc.is_empty() => format!("@param {}", name),
        JsDocLine::Param { name, desc } => format!("@param {} {}", name, desc),
        JsDocLine::Returns(s) => format!("@returns {}", s),
        JsDocLine::Throws(s) => format!("@throws {{{}}}", s),
        JsDocLine::Deprecated(Some(s)) => format!("@deprecated {}", s),
        JsDocLine::Deprecated(None) => "@deprecated".to_string(),
    }
}

/// Render a module to a formatted string.
pub fn render_module(module: &Module) -> String {
    let doc = lower_module(module);
    let config = PrintConfig::default();
    render(&doc, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsdoc_param_without_description_has_no_trailing_space() {
        let line = JsDocLine::Param {
            name: "body".to_string(),
            desc: String::new(),
        };

        assert_eq!(render_jsdoc_line(&line), "@param body");
    }
}
