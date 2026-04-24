use crate::ast::{BinOp, Expr, FnDecl, File, Item, Stmt, Ty};

pub fn generate(file: &File) -> String {
    let mut out = String::new();
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdio.h>\n\n");
    // Forward-declare all functions so order doesn't matter
    for item in &file.items {
        let Item::Fn(f) = item;
        if f.name != "main" {
                out.push_str(&format!("{};\n", fn_signature(f)));
        }
    }
    if file.items.len() > 1 { out.push('\n'); }
    for item in &file.items {
        match item {
            Item::Fn(f) => emit_fn(&mut out, f),
        }
    }
    out
}

fn ty_str(ty: &Ty) -> &'static str {
    match ty {
        Ty::I32  => "int32_t",
        Ty::I64  => "int64_t",
        Ty::Bool => "bool",
        Ty::Unit => "void",
    }
}

fn fn_signature(f: &FnDecl) -> String {
    let ret = ty_str(&f.return_ty);
    if f.name == "main" {
        return "int main(void)".to_string();
    }
    let params = if f.params.is_empty() {
        "void".to_string()
    } else {
        f.params.iter()
            .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("{ret} {}({params})", f.name)
}

fn emit_fn(out: &mut String, f: &FnDecl) {
    out.push_str(&format!("{} {{\n", fn_signature(f)));
    for stmt in &f.body.stmts {
        emit_stmt(out, stmt);
    }
    if f.name == "main" {
        out.push_str("    return 0;\n");
    }
    out.push_str("}\n\n");
}

fn emit_stmt(out: &mut String, stmt: &Stmt) {
    match stmt {
        Stmt::Println { format, args } => emit_println(out, format, args),
        Stmt::Let { name, mutable, expr } => {
            let kw = if *mutable { "" } else { "const " };
            out.push_str(&format!("    {kw}int64_t {name} = {};\n", emit_expr(expr)));
        }
        Stmt::Assign { name, expr } => {
            out.push_str(&format!("    {name} = {};\n", emit_expr(expr)));
        }
        Stmt::Return(expr) => match expr {
            Some(e) => out.push_str(&format!("    return {};\n", emit_expr(e))),
            None    => out.push_str("    return;\n"),
        },
        Stmt::If { cond, then_block, else_block } => {
            out.push_str(&format!("    if ({}) {{\n", emit_expr(cond)));
            for s in &then_block.stmts { emit_stmt_indented(out, s, 2); }
            match else_block {
                None => out.push_str("    }\n"),
                Some(blk) => {
                    // Detect `else if` (block with a single If stmt) vs plain else
                    if blk.stmts.len() == 1 {
                        if let Stmt::If { cond: c2, then_block: t2, else_block: e2 } = &blk.stmts[0] {
                            out.push_str(&format!("    }} else if ({}) {{\n", emit_expr(c2)));
                            for s in &t2.stmts { emit_stmt_indented(out, s, 2); }
                            emit_else_tail(out, e2);
                            return;
                        }
                    }
                    out.push_str("    } else {\n");
                    for s in &blk.stmts { emit_stmt_indented(out, s, 2); }
                    out.push_str("    }\n");
                }
            }
        }
        Stmt::While { cond, body } => {
            out.push_str(&format!("    while ({}) {{\n", emit_expr(cond)));
            for s in &body.stmts { emit_stmt_indented(out, s, 2); }
            out.push_str("    }\n");
        }
        Stmt::Expr(expr) => {
            out.push_str(&format!("    {};\n", emit_expr(expr)));
        }
    }
}

fn emit_println(out: &mut String, format: &str, args: &[Expr]) {
    // Replace each `{}` with `%lld`, escape for C.
    // All args are cast to (long long) so the format specifier is always correct
    // regardless of the underlying expression type (no type checker yet).
    let mut fmt_c = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'}') {
            chars.next();
            fmt_c.push_str("%lld");
        } else {
            match ch {
                '"'  => fmt_c.push_str("\\\""),
                '\\' => fmt_c.push_str("\\\\"),
                '\n' => fmt_c.push_str("\\n"),
                '\t' => fmt_c.push_str("\\t"),
                c    => fmt_c.push(c),
            }
        }
    }

    if args.is_empty() {
        out.push_str(&format!("    printf(\"{fmt_c}\\n\");\n"));
    } else {
        let args_str: Vec<String> = args.iter()
            .map(|a| format!("(long long)({})", emit_expr(a)))
            .collect();
        out.push_str(&format!("    printf(\"{fmt_c}\\n\", {});\n", args_str.join(", ")));
    }
}

/// Emit a statement with `indent` levels of 4-space indentation.
fn emit_stmt_indented(out: &mut String, stmt: &Stmt, indent: usize) {
    let prefix = "    ".repeat(indent);
    // Temporarily capture and re-indent
    let mut buf = String::new();
    emit_stmt(&mut buf, stmt);
    for line in buf.lines() {
        out.push_str(&prefix);
        // strip the leading 4 spaces emit_stmt already adds
        out.push_str(line.strip_prefix("    ").unwrap_or(line));
        out.push('\n');
    }
}

/// Emit the tail of an else-if chain.
fn emit_else_tail(out: &mut String, else_block: &Option<crate::ast::Block>) {
    match else_block {
        None => out.push_str("    }\n"),
        Some(blk) => {
            if blk.stmts.len() == 1 {
                if let Stmt::If { cond, then_block, else_block: inner } = &blk.stmts[0] {
                    out.push_str(&format!("    }} else if ({}) {{\n", emit_expr(cond)));
                    for s in &then_block.stmts { emit_stmt_indented(out, s, 2); }
                    emit_else_tail(out, inner);
                    return;
                }
            }
            out.push_str("    } else {\n");
            for s in &blk.stmts { emit_stmt_indented(out, s, 2); }
            out.push_str("    }\n");
        }
    }
}

fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Int(n)    => format!("INT64_C({n})"),
        Expr::Bool(b)   => if *b { "true".to_string() } else { "false".to_string() },
        Expr::Var(name) => name.clone(),
        Expr::Call { name, args } => {
            let args_str: Vec<String> = args.iter().map(emit_expr).collect();
            format!("{name}({})", args_str.join(", "))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Eq  => "==",
                BinOp::Ne  => "!=",
                BinOp::Lt  => "<",
                BinOp::Gt  => ">",
                BinOp::Le  => "<=",
                BinOp::Ge  => ">=",
                BinOp::And => "&&",
                BinOp::Or  => "||",
            };
            format!("({} {} {})", emit_expr(lhs), op_str, emit_expr(rhs))
        }
    }
}
