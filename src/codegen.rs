use std::collections::HashMap;
use crate::ast::{BinOp, EnumDecl, Expr, FnDecl, File, ImplBlock, Item, Receiver, Stmt, StructDecl, Ty, UnOp};

// ---------------------------------------------------------------------------
// Codegen context
// ---------------------------------------------------------------------------

struct Ctx {
    /// The type name of the current impl block (if any).
    impl_type: Option<String>,
    /// True when `self` is emitted as a pointer (ref/refmut receiver).
    ref_self: bool,
    /// Maps variable names to their struct type name, for method call resolution.
    type_env: HashMap<String, String>,
}

impl Ctx {
    fn new() -> Self {
        Self { impl_type: None, ref_self: false, type_env: HashMap::new() }
    }

    fn for_method(impl_type: &str, receiver: &Receiver) -> Self {
        let ref_self = matches!(receiver, Receiver::Ref | Receiver::RefMut);
        let mut type_env = HashMap::new();
        type_env.insert("self".to_string(), impl_type.to_string());
        Self { impl_type: Some(impl_type.to_string()), ref_self, type_env }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn generate(file: &File) -> String {
    let mut out = String::new();
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdio.h>\n");

    // Emit struct typedefs first, then enum typedefs
    for item in &file.items {
        match item {
            Item::Struct(s) => { out.push('\n'); emit_struct(&mut out, s); }
            Item::Enum(e)   => { out.push('\n'); emit_enum(&mut out, e); }
            _ => {}
        }
    }

    // Forward-declare all functions and methods
    out.push('\n');
    for item in &file.items {
        match item {
            Item::Fn(f) if f.name != "main" => {
                out.push_str(&format!("{};\n", fn_signature(f, None)));
            }
            Item::Impl(imp) => {
                for m in &imp.methods {
                    out.push_str(&format!("{};\n", fn_signature(m, Some(&imp.type_name))));
                }
            }
            _ => {}
        }
    }

    // Emit definitions
    out.push('\n');
    for item in &file.items {
        match item {
            Item::Fn(f) => emit_fn(&mut out, f, None),
            Item::Impl(imp) => emit_impl(&mut out, imp),
            Item::Struct(_) | Item::Enum(_) => {} // already emitted
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

fn ty_str(ty: &Ty) -> String {
    match ty {
        Ty::I8    => "int8_t".to_string(),
        Ty::I16   => "int16_t".to_string(),
        Ty::I32   => "int32_t".to_string(),
        Ty::I64   => "int64_t".to_string(),
        Ty::Isize => "intptr_t".to_string(),
        Ty::U8    => "uint8_t".to_string(),
        Ty::U16   => "uint16_t".to_string(),
        Ty::U32   => "uint32_t".to_string(),
        Ty::U64   => "uint64_t".to_string(),
        Ty::Usize => "uintptr_t".to_string(),
        Ty::Bool  => "bool".to_string(),
        Ty::Unit  => "void".to_string(),
        Ty::Named(n) => n.clone(),
    }
}

fn default_ty() -> &'static str { "int64_t" }

// ---------------------------------------------------------------------------
// Struct
// ---------------------------------------------------------------------------

fn emit_struct(out: &mut String, s: &StructDecl) {
    out.push_str(&format!("typedef struct {} {{\n", s.name));
    for f in &s.fields {
        out.push_str(&format!("    {} {};\n", ty_str(&f.ty), f.name));
    }
    out.push_str(&format!("}} {};\n", s.name));
}

fn emit_enum(out: &mut String, e: &EnumDecl) {
    out.push_str(&format!("typedef enum {{\n"));
    for (i, variant) in e.variants.iter().enumerate() {
        let comma = if i + 1 < e.variants.len() { "," } else { "" };
        out.push_str(&format!("    {}_{} = {}{}\n", e.name, variant, i, comma));
    }
    out.push_str(&format!("}} {};\n", e.name));
}

// ---------------------------------------------------------------------------
// Functions and methods
// ---------------------------------------------------------------------------

fn fn_signature(f: &FnDecl, impl_type: Option<&str>) -> String {
    if f.name == "main" {
        return "int main(void)".to_string();
    }

    let ret = ty_str(&f.return_ty);

    // Build parameter list
    let mut param_parts: Vec<String> = Vec::new();

    // Self receiver
    if let (Some(recv), Some(itype)) = (&f.receiver, impl_type) {
        let self_param = match recv {
            Receiver::Value  => format!("{itype}* self"),
            Receiver::Ref    => format!("const {itype}* self"),
            Receiver::RefMut => format!("{itype}* self"),
        };
        param_parts.push(self_param);
    }

    // Regular params
    for p in &f.params {
        param_parts.push(format!("{} {}", ty_str(&p.ty), p.name));
    }

    let params = if param_parts.is_empty() {
        "void".to_string()
    } else {
        param_parts.join(", ")
    };

    // Mangle name: for methods, prefix with TypeName_
    let mangled = match impl_type {
        Some(t) => format!("{t}_{}", f.name),
        None    => f.name.clone(),
    };

    format!("{ret} {mangled}({params})")
}

fn emit_fn(out: &mut String, f: &FnDecl, impl_type: Option<&str>) {
    out.push_str(&format!("{} {{\n", fn_signature(f, impl_type)));

    let mut ctx = match (&f.receiver, impl_type) {
        (Some(recv), Some(itype)) => {
            let mut c = Ctx::for_method(itype, recv);
            // Add params to type_env
            for p in &f.params {
                if let Ty::Named(n) = &p.ty {
                    c.type_env.insert(p.name.clone(), n.clone());
                }
            }
            c
        }
        _ => {
            let mut c = Ctx::new();
            for p in &f.params {
                if let Ty::Named(n) = &p.ty {
                    c.type_env.insert(p.name.clone(), n.clone());
                }
            }
            c
        }
    };

    for stmt in &f.body.stmts {
        emit_stmt(out, stmt, &mut ctx, 1);
    }
    if f.name == "main" {
        out.push_str("    return 0;\n");
    }
    out.push_str("}\n\n");
}

fn emit_impl(out: &mut String, imp: &ImplBlock) {
    for m in &imp.methods {
        emit_fn(out, m, Some(&imp.type_name));
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

fn pad(indent: usize) -> String { "    ".repeat(indent) }

fn emit_stmt(out: &mut String, stmt: &Stmt, ctx: &mut Ctx, indent: usize) {
    let p = pad(indent);
    match stmt {
        Stmt::Println { format, args } => emit_println(out, format, args, ctx, &p),

        Stmt::Let { name, mutable, ty, expr } => {
            let c_ty = ty.as_ref().map(|t| ty_str(t))
                .unwrap_or_else(|| default_ty().to_string());
            // Track struct type in env for method resolution
            if let Some(Ty::Named(n)) = ty { ctx.type_env.insert(name.clone(), n.clone()); }
            let kw = if *mutable { "" } else { "const " };
            out.push_str(&format!("{p}{kw}{c_ty} {name} = {};\n", emit_expr(expr, ctx)));
        }

        Stmt::Assign { name, expr } => {
            out.push_str(&format!("{p}{name} = {};\n", emit_expr(expr, ctx)));
        }

        Stmt::Return(expr) => match expr {
            Some(e) => out.push_str(&format!("{p}return {};\n", emit_expr(e, ctx))),
            None    => out.push_str(&format!("{p}return;\n")),
        },

        Stmt::If { cond, then_block, else_block } => {
            out.push_str(&format!("{p}if ({}) {{\n", emit_expr(cond, ctx)));
            for s in &then_block.stmts { emit_stmt(out, s, ctx, indent + 1); }
            emit_else(out, else_block, ctx, indent);
        }

        Stmt::While { cond, body } => {
            out.push_str(&format!("{p}while ({}) {{\n", emit_expr(cond, ctx)));
            for s in &body.stmts { emit_stmt(out, s, ctx, indent + 1); }
            out.push_str(&format!("{p}}}\n"));
        }

        Stmt::Match { expr, arms } => {
            out.push_str(&format!("{p}switch ({}) {{\n", emit_expr(expr, ctx)));
            for arm in arms {
                let case = match &arm.pat {
                    crate::ast::Pat::Wildcard  => format!("{p}default:"),
                    crate::ast::Pat::Bool(b)   => format!("{p}case {}:", if *b { 1 } else { 0 }),
                    crate::ast::Pat::Int(n)     => format!("{p}case {}:", n),
                    crate::ast::Pat::EnumVariant { type_name, variant } =>
                        format!("{p}case {type_name}_{variant}:"),
                };
                out.push_str(&format!("{case} {{\n"));
                for s in &arm.body.stmts { emit_stmt(out, s, ctx, indent + 2); }
                out.push_str(&format!("{}    break;\n", p));
                out.push_str(&format!("{}  }}\n", p));
            }
            out.push_str(&format!("{p}}}\n"));
        }

        Stmt::Expr(expr) => {
            // Field assignment: BinOp::Eq used as assignment marker
            if let Expr::BinOp { op: BinOp::Eq, lhs, rhs } = expr {
                out.push_str(&format!("{p}{} = {};\n",
                    emit_expr(lhs, ctx), emit_expr(rhs, ctx)));
            } else {
                out.push_str(&format!("{p}{};\n", emit_expr(expr, ctx)));
            }
        }
    }
}

fn emit_else(out: &mut String, else_block: &Option<crate::ast::Block>, ctx: &mut Ctx, indent: usize) {
    let p = pad(indent);
    match else_block {
        None => out.push_str(&format!("{p}}}\n")),
        Some(blk) => {
            if blk.stmts.len() == 1 {
                if let Stmt::If { cond, then_block, else_block: inner } = &blk.stmts[0] {
                    out.push_str(&format!("{p}}} else if ({}) {{\n", emit_expr(cond, ctx)));
                    for s in &then_block.stmts { emit_stmt(out, s, ctx, indent + 1); }
                    emit_else(out, inner, ctx, indent);
                    return;
                }
            }
            out.push_str(&format!("{p}}} else {{\n"));
            for s in &blk.stmts { emit_stmt(out, s, ctx, indent + 1); }
            out.push_str(&format!("{p}}}\n"));
        }
    }
}

fn emit_println(out: &mut String, format: &str, args: &[Expr], ctx: &mut Ctx, p: &str) {
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
        out.push_str(&format!("{p}printf(\"{fmt_c}\\n\");\n"));
    } else {
        let args_str: Vec<String> = args.iter()
            .map(|a| format!("(long long)({})", emit_expr(a, ctx)))
            .collect();
        out.push_str(&format!("{p}printf(\"{fmt_c}\\n\", {});\n", args_str.join(", ")));
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

fn emit_expr(expr: &Expr, ctx: &mut Ctx) -> String {
    match expr {
        Expr::Int(n)    => format!("INT64_C({n})"),
        Expr::Bool(b)   => if *b { "true".to_string() } else { "false".to_string() },
        Expr::Var(name) => name.clone(),

        Expr::StructLit { name, fields } => {
            let field_inits: Vec<String> = fields.iter()
                .map(|(n, e)| format!(".{n} = {}", emit_expr(e, ctx)))
                .collect();
            format!("({name}){{{}}}", field_inits.join(", "))
        }

        Expr::Field { expr, field } => {
            // Use -> when accessing through a pointer self, . otherwise
            let is_self_ptr = matches!(expr.as_ref(), Expr::Var(n) if n == "self") && ctx.ref_self;
            if is_self_ptr {
                format!("self->{field}")
            } else {
                format!("{}.{field}", emit_expr(expr, ctx))
            }
        }

        Expr::Call { name, args } => {
            let args_str: Vec<String> = args.iter().map(|a| emit_expr(a, ctx)).collect();
            format!("{name}({})", args_str.join(", "))
        }

        Expr::AssocCall { type_name, method, args } => {
            if args.is_empty() {
                // Enum variant or zero-arg associated function
                // Emit as variant constant if no parens needed
                format!("{type_name}_{method}")
            } else {
                let args_str: Vec<String> = args.iter().map(|a| emit_expr(a, ctx)).collect();
                format!("{type_name}_{method}({})", args_str.join(", "))
            }
        }

        Expr::MethodCall { expr, method, args } => {
            // Look up the type of expr to mangle the call: TypeName_method
            let type_name = match expr.as_ref() {
                Expr::Var(name) => ctx.type_env.get(name.as_str()).cloned(),
                _ => None,
            };
            let args_str: Vec<String> = args.iter().map(|a| emit_expr(a, ctx)).collect();
            let expr_c = emit_expr(expr, ctx);

            match type_name {
                Some(t) => {
                    // Pass self as pointer
                    let self_arg = if matches!(expr.as_ref(), Expr::Var(n) if n == "self") && ctx.ref_self {
                        expr_c // already a pointer
                    } else {
                        format!("&({expr_c})")
                    };
                    if args_str.is_empty() {
                        format!("{t}_{method}({self_arg})")
                    } else {
                        format!("{t}_{method}({self_arg}, {})", args_str.join(", "))
                    }
                }
                None => {
                    // Unknown type: emit as best-effort comment
                    format!("/* unknown type */{expr_c}.{method}({})", args_str.join(", "))
                }
            }
        }

        Expr::UnOp { op, operand } => {
            let op_str = match op {
                UnOp::Neg    => "-",
                UnOp::Not    => "!",
                UnOp::BitNot => "~",
            };
            format!("({op_str}{})", emit_expr(operand, ctx))
        }

        Expr::BinOp { op, lhs, rhs } => {
            let op_str = match op {
                BinOp::Add    => "+",  BinOp::Sub => "-", BinOp::Mul => "*",
                BinOp::Div    => "/",  BinOp::Rem => "%",
                BinOp::BitAnd => "&",  BinOp::BitOr => "|", BinOp::BitXor => "^",
                BinOp::Shl    => "<<", BinOp::Shr => ">>",
                BinOp::Eq     => "==", BinOp::Ne => "!=",
                BinOp::Lt     => "<",  BinOp::Gt => ">",
                BinOp::Le     => "<=", BinOp::Ge => ">=",
                BinOp::And    => "&&", BinOp::Or => "||",
            };
            format!("({} {} {})", emit_expr(lhs, ctx), op_str, emit_expr(rhs, ctx))
        }
    }
}
