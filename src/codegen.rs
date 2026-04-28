use crate::ast::{BinOp, Expr, File, Item, Stmt};

pub fn generate(file: &File) -> String {
    let mut out = String::new();
    out.push_str("#include <stdio.h>\n\n");
    for item in &file.items {
        match item {
            Item::Fn(f) => {
                if f.name == "main" {
                    out.push_str("int main(void) {\n");
                } else {
                    out.push_str(&format!("void {}(void) {{\n", f.name));
                }
                for stmt in &f.body.stmts {
                    match stmt {
                        Stmt::Println { format, args } => {
                            emit_println(&mut out, format, args);
                        }
                    }
                }
                if f.name == "main" {
                    out.push_str("    return 0;\n");
                }
                out.push_str("}\n");
            }
        }
    }
    out
}

fn emit_println(out: &mut String, format: &str, args: &[Expr]) {
    // Replace each `{}` in the format string with `%lld`, escape for C.
    let mut fmt_c = String::new();
    let mut arg_idx = 0;
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'}') {
            chars.next(); // consume '}'
            fmt_c.push_str("%lld");
            arg_idx += 1;
        } else {
            // c_escape the character inline
            match ch {
                '"'  => fmt_c.push_str("\\\""),
                '\\' => fmt_c.push_str("\\\\"),
                '\n' => fmt_c.push_str("\\n"),
                '\t' => fmt_c.push_str("\\t"),
                c    => fmt_c.push(c),
            }
        }
    }
    let _ = arg_idx;

    if args.is_empty() {
        out.push_str(&format!("    printf(\"{fmt_c}\\n\");\n"));
    } else {
        let args_str: Vec<String> = args.iter().map(emit_expr).collect();
        out.push_str(&format!("    printf(\"{fmt_c}\\n\", {});\n", args_str.join(", ")));
    }
}

fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Int(n) => format!("{n}LL"),
        Expr::BinOp { op, lhs, rhs } => {
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
            };
            // Wrap in parens to preserve brust precedence in C output
            format!("({} {} {})", emit_expr(lhs), op_str, emit_expr(rhs))
        }
    }
}
