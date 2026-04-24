use std::collections::HashMap;
use crate::ast::{
    BinOp, Block, EnumDecl, EnumVariant, Expr, FnDecl, File, ImplBlock, Item,
    MatchArm, Pat, PatBindings, Receiver, Stmt, StructDecl, Ty, UnOp, VariantFields,
};

// ---------------------------------------------------------------------------
// Codegen context (per-function state)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    impl_type: Option<String>,
    ref_self: bool,
    /// Maps variable names to their struct/enum type name, for method call resolution.
    type_env: HashMap<String, String>,
    /// Maps variable names to their type, for printf format specifier selection.
    var_types: HashMap<String, Ty>,
    /// Return type of the current function, used to hint tuple literal emission.
    return_ty: Option<Ty>,
}

impl Ctx {
    fn new() -> Self { Self { impl_type: None, ref_self: false, type_env: HashMap::new(), var_types: HashMap::new(), return_ty: None } }

    fn for_method(impl_type: &str, receiver: &Receiver, params_type_env: HashMap<String, String>) -> Self {
        let ref_self = matches!(receiver, Receiver::Ref | Receiver::RefMut);
        let mut type_env = params_type_env;
        type_env.insert("self".to_string(), impl_type.to_string());
        Self { impl_type: Some(impl_type.to_string()), ref_self, type_env, var_types: HashMap::new(), return_ty: None }
    }
}

// ---------------------------------------------------------------------------
// Codegen struct (owns all state)
// ---------------------------------------------------------------------------

struct Codegen {
    /// Enum declarations indexed by name, for tagged union resolution in match.
    enums: HashMap<String, EnumDecl>,
    /// Function/method param types (mangled name → param type list) for tuple arg hints.
    fn_params: HashMap<String, Vec<Ty>>,
    out: String,
}

pub fn generate(file: &File) -> String {
    let enums: HashMap<String, EnumDecl> = file.items.iter()
        .filter_map(|i| if let Item::Enum(e) = i { Some((e.name.clone(), e.clone())) } else { None })
        .collect();
    // Collect param types for all functions and methods
    let mut fn_params: HashMap<String, Vec<Ty>> = HashMap::new();
    for item in &file.items {
        match item {
            Item::Fn(f) => {
                fn_params.insert(f.name.clone(), f.params.iter().map(|p| p.ty.clone()).collect());
            }
            Item::Impl(imp) => {
                for m in &imp.methods {
                    let mangled = format!("{}_{}", imp.type_name, m.name);
                    fn_params.insert(mangled, m.params.iter().map(|p| p.ty.clone()).collect());
                }
            }
            _ => {}
        }
    }
    let mut cg = Codegen { enums, fn_params, out: String::new() };
    cg.run(file);
    cg.out
}

impl Codegen {
    fn run(&mut self, file: &File) {
        self.out.push_str("#include <stdbool.h>\n#include <stdint.h>\n#include <stdio.h>\n");

        // Collect and emit tuple typedefs (pre-scan)
        let tuple_types = collect_tuple_types(file);
        for tys in &tuple_types {
            self.out.push('\n');
            emit_tuple_typedef(&mut self.out, tys);
        }

        // Emit struct and enum type definitions
        for item in &file.items {
            match item {
                Item::Struct(s) => { self.out.push('\n'); emit_struct(&mut self.out, s); }
                Item::Enum(e)   => { self.out.push('\n'); self.emit_enum(e); }
                _ => {}
            }
        }

        // Forward declarations
        self.out.push('\n');
        for item in &file.items {
            match item {
                Item::Fn(f) if f.name != "main" => {
                    self.out.push_str(&format!("{};\n", fn_signature(f, None)));
                }
                Item::Impl(imp) => {
                    for m in &imp.methods {
                        self.out.push_str(&format!("{};\n", fn_signature(m, Some(&imp.type_name))));
                    }
                }
                _ => {}
            }
        }

        // Function / method definitions
        self.out.push('\n');
        for item in &file.items {
            match item {
                Item::Fn(f) => self.emit_fn(f, None),
                Item::Impl(imp) => self.emit_impl(imp),
                Item::Struct(_) | Item::Enum(_) => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Struct and enum emission
    // -----------------------------------------------------------------------

    fn emit_enum(&mut self, e: &EnumDecl) {
        let has_data = e.variants.iter().any(|v| !matches!(v.fields, VariantFields::Unit));
        if !has_data {
            // Simple C enum — all unit variants
            self.out.push_str("typedef enum {\n");
            for (i, v) in e.variants.iter().enumerate() {
                let comma = if i + 1 < e.variants.len() { "," } else { "" };
                self.out.push_str(&format!("    {}_{} = {}{}\n", e.name, v.name, i, comma));
            }
            self.out.push_str(&format!("}} {};\n", e.name));
        } else {
            // Tagged union: tag enum + struct with union data + static inline constructors
            self.out.push_str("typedef enum {\n");
            for (i, v) in e.variants.iter().enumerate() {
                let comma = if i + 1 < e.variants.len() { "," } else { "" };
                self.out.push_str(&format!("    {}_{}_tag = {}{}\n", e.name, v.name, i, comma));
            }
            self.out.push_str(&format!("}} {}_tag;\n", e.name));

            self.out.push_str(&format!("typedef struct {{\n    {}_tag tag;\n    union {{\n", e.name));
            for v in &e.variants {
                match &v.fields {
                    VariantFields::Unit => {}
                    VariantFields::Tuple(tys) => {
                        self.out.push_str("        struct {\n");
                        for (i, ty) in tys.iter().enumerate() {
                            self.out.push_str(&format!("            {} _{};\n", ty_str(ty), i));
                        }
                        self.out.push_str(&format!("        }} {};\n", v.name));
                    }
                    VariantFields::Named(fields) => {
                        self.out.push_str("        struct {\n");
                        for f in fields {
                            self.out.push_str(&format!("            {} {};\n", ty_str(&f.ty), f.name));
                        }
                        self.out.push_str(&format!("        }} {};\n", v.name));
                    }
                }
            }
            self.out.push_str(&format!("    }} data;\n}} {};\n", e.name));

            // Static inline constructor functions (safe for complex arg expressions)
            for v in &e.variants {
                self.emit_enum_constructor(e, v);
            }
        }
    }

    fn emit_enum_constructor(&mut self, e: &EnumDecl, v: &EnumVariant) {
        let tag = format!("{}_{}_tag", e.name, v.name);
        let mangled = format!("{}_{}", e.name, v.name);
        match &v.fields {
            VariantFields::Unit => {
                // Unit variant in tagged union: zero-arg inline function
                self.out.push_str(&format!(
                    "static inline {name} {mangled}(void) {{ return ({name}){{ .tag = {tag} }}; }}\n",
                    name = e.name
                ));
            }
            VariantFields::Tuple(tys) => {
                let params: Vec<String> = tys.iter().enumerate()
                    .map(|(i, ty)| format!("{} _{i}", ty_str(ty)))
                    .collect();
                let inits: Vec<String> = (0..tys.len())
                    .map(|i| format!(".data.{vname}._{i} = _{i}", vname = v.name))
                    .collect();
                self.out.push_str(&format!(
                    "static inline {name} {mangled}({params}) {{ return ({name}){{ .tag = {tag}, {inits} }}; }}\n",
                    name = e.name,
                    params = params.join(", "), inits = inits.join(", ")
                ));
                // Register param types for tuple arg hinting at call sites
                self.fn_params.insert(mangled, tys.clone());
            }
            VariantFields::Named(fields) => {
                let params: Vec<String> = fields.iter()
                    .map(|f| format!("{} {}", ty_str(&f.ty), f.name))
                    .collect();
                let inits: Vec<String> = fields.iter()
                    .map(|f| format!(".data.{vname}.{fname} = {fname}", vname = v.name, fname = f.name))
                    .collect();
                self.out.push_str(&format!(
                    "static inline {name} {mangled}({params}) {{ return ({name}){{ .tag = {tag}, {inits} }}; }}\n",
                    name = e.name,
                    params = params.join(", "), inits = inits.join(", ")
                ));
                // Register param types (in field order) for call-site hinting
                self.fn_params.insert(mangled, fields.iter().map(|f| f.ty.clone()).collect());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Functions and methods
    // -----------------------------------------------------------------------

    fn emit_fn(&mut self, f: &FnDecl, impl_type: Option<&str>) {
        let sig = fn_signature(f, impl_type);
        self.out.push_str(&format!("{sig} {{\n"));

        let mut params_env = HashMap::new();
        for p in &f.params {
            if let Ty::Named(n) = &p.ty { params_env.insert(p.name.clone(), n.clone()); }
        }

        let mut ctx = match (&f.receiver, impl_type) {
            (Some(recv), Some(itype)) => Ctx::for_method(itype, recv, params_env),
            _ => Ctx { type_env: params_env, ..Ctx::new() },
        };
        ctx.return_ty = Some(f.return_ty.clone());

        for stmt in &f.body.stmts {
            self.emit_stmt(stmt, &mut ctx, 1);
        }
        if f.name == "main" { self.out.push_str("    return 0;\n"); }
        self.out.push_str("}\n\n");
    }

    fn emit_impl(&mut self, imp: &ImplBlock) {
        for m in &imp.methods {
            self.emit_fn(m, Some(&imp.type_name));
        }
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt, ctx: &mut Ctx, indent: usize) {
        let p = pad(indent);
        match stmt {
            Stmt::Println { format, args } => {
                let s = self.emit_println(format, args, ctx);
                self.out.push_str(&format!("{p}{s}\n"));
            }

            Stmt::Let { name, mutable, ty, expr } => {
                let c_ty = ty.as_ref().map(|t| ty_str(t)).unwrap_or_else(|| "int64_t".to_string());
                // Track named types for method resolution
                if let Some(Ty::Named(n)) = ty { ctx.type_env.insert(name.clone(), n.clone()); }
                // Track all types for printf format specifier selection
                if let Some(t) = ty { ctx.var_types.insert(name.clone(), t.clone()); }
                // Don't add `const` prefix for pointer/ref types — constness lives in the type already
                let is_ptr = matches!(ty, Some(Ty::Ref(_) | Ty::RefMut(_) | Ty::RawConst(_) | Ty::RawMut(_)));
                let kw = if *mutable || is_ptr { "" } else { "const " };
                let val = self.emit_expr_hint(expr, ctx, ty.as_ref());
                self.out.push_str(&format!("{p}{kw}{c_ty} {name} = {val};\n"));
            }

            Stmt::Assign { name, expr } => {
                let val = self.emit_expr(expr, ctx);
                self.out.push_str(&format!("{p}{name} = {val};\n"));
            }

            Stmt::Return(expr) => match expr {
                Some(Expr::Tuple(elems)) if elems.is_empty() => {
                    // unit () return - emit bare return if function returns void
                    if ctx.return_ty == Some(Ty::Unit) {
                        self.out.push_str(&format!("{p}return;\n"));
                    } else {
                        self.out.push_str(&format!("{p}return 0;\n"));
                    }
                }
                Some(e) => {
                    let hint = ctx.return_ty.clone();
                    let s = self.emit_expr_hint(e, ctx, hint.as_ref());
                    self.out.push_str(&format!("{p}return {s};\n"));
                }
                None => self.out.push_str(&format!("{p}return;\n")),
            },

            Stmt::If { cond, then_block, else_block } => {
                let cond_s = self.emit_expr(cond, ctx);
                self.out.push_str(&format!("{p}if ({cond_s}) {{\n"));
                for s in &then_block.stmts { self.emit_stmt(s, ctx, indent + 1); }
                self.emit_else(else_block, ctx, indent);
            }

            Stmt::While { cond, body } => {
                let cond_s = self.emit_expr(cond, ctx);
                self.out.push_str(&format!("{p}while ({cond_s}) {{\n"));
                for s in &body.stmts { self.emit_stmt(s, ctx, indent + 1); }
                self.out.push_str(&format!("{p}}}\n"));
            }

            Stmt::Match { expr, arms } => self.emit_match(expr, arms, ctx, indent),

            Stmt::Expr(expr) => {
                // Field/tuple-index assignment encoded as BinOp::Eq
                if let Expr::BinOp { op: BinOp::Eq, lhs, rhs } = expr {
                    let ls = self.emit_expr(lhs, ctx);
                    let rs = self.emit_expr(rhs, ctx);
                    self.out.push_str(&format!("{p}{ls} = {rs};\n"));
                } else if let Expr::Unsafe(block) = expr {
                    // `unsafe { ... }` as a statement: emit stmts directly
                    for s in &block.stmts { self.emit_stmt(s, ctx, indent); }
                } else {
                    let s = self.emit_expr(expr, ctx);
                    self.out.push_str(&format!("{p}{s};\n"));
                }
            }
        }
    }

    fn emit_else(&mut self, else_block: &Option<Block>, ctx: &mut Ctx, indent: usize) {
        let p = pad(indent);
        match else_block {
            None => self.out.push_str(&format!("{p}}}\n")),
            Some(blk) => {
                if blk.stmts.len() == 1 {
                    if let Stmt::If { cond, then_block, else_block: inner } = &blk.stmts[0] {
                        let cond_s = self.emit_expr(cond, ctx);
                        self.out.push_str(&format!("{p}}} else if ({cond_s}) {{\n"));
                        for s in &then_block.stmts { self.emit_stmt(s, ctx, indent + 1); }
                        self.emit_else(inner, ctx, indent);
                        return;
                    }
                }
                self.out.push_str(&format!("{p}}} else {{\n"));
                for s in &blk.stmts { self.emit_stmt(s, ctx, indent + 1); }
                self.out.push_str(&format!("{p}}}\n"));
            }
        }
    }

    fn emit_match(&mut self, expr: &Expr, arms: &[MatchArm], ctx: &mut Ctx, indent: usize) {
        let p  = pad(indent);
        let ip = pad(indent + 1);
        let bp = pad(indent + 2);

        // Determine enum type from patterns (if any arm is an enum variant)
        let enum_type_name: Option<String> = arms.iter().find_map(|a| {
            if let Pat::EnumVariant { type_name, .. } = &a.pat { Some(type_name.clone()) } else { None }
        });
        let enum_decl: Option<EnumDecl> = enum_type_name.as_ref()
            .and_then(|tn| self.enums.get(tn).cloned());
        let is_tagged = enum_decl.as_ref()
            .map_or(false, |e| e.variants.iter().any(|v| !matches!(v.fields, VariantFields::Unit)));

        // Materialize scrutinee into a temp var — always for tagged unions,
        // prevents double evaluation and enables field access.
        let match_var = if is_tagged {
            match expr {
                Expr::Var(n) => n.clone(),
                _ => {
                    let type_name = enum_type_name.as_deref().unwrap_or("int64_t");
                    let expr_s = self.emit_expr(expr, ctx);
                    self.out.push_str(&format!("{p}const {type_name} _match_val = {expr_s};\n"));
                    "_match_val".to_string()
                }
            }
        } else {
            // For simple enums / int / bool, just use the expression inline
            self.emit_expr(expr, ctx)
        };

        let switch_cond = if is_tagged { format!("{match_var}.tag") } else { match_var.clone() };
        self.out.push_str(&format!("{p}switch ({switch_cond}) {{\n"));

        for arm in arms {
            // Clone type_env so bindings don't leak across arms
            let mut arm_ctx = ctx.clone();

            match &arm.pat {
                Pat::Wildcard => { self.out.push_str(&format!("{ip}default: {{\n")); }
                Pat::Bool(b)  => { self.out.push_str(&format!("{ip}case {}: {{\n", if *b { 1 } else { 0 })); }
                Pat::Int(n)   => { self.out.push_str(&format!("{ip}case {n}: {{\n")); }
                Pat::EnumVariant { type_name, variant, bindings } => {
                    if is_tagged {
                        self.out.push_str(&format!("{ip}case {type_name}_{variant}_tag: {{\n"));
                        // Emit binding declarations from variant fields
                        if let Some(ref edecl) = enum_decl {
                            if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                                match bindings {
                                    PatBindings::None => {}
                                    PatBindings::Tuple(binds) => {
                                        if let VariantFields::Tuple(tys) = &ev.fields {
                                            for (i, binding) in binds.iter().enumerate() {
                                                if binding == "_" { continue; }
                                                let fty = tys.get(i).map(ty_str)
                                                    .unwrap_or_else(|| "int64_t".to_string());
                                                self.out.push_str(&format!(
                                                    "{bp}{fty} {binding} = {match_var}.data.{variant}._{i};\n"
                                                ));
                                                if let Some(Ty::Named(n)) = tys.get(i) {
                                                    arm_ctx.type_env.insert(binding.clone(), n.clone());
                                                }
                                            }
                                        }
                                    }
                                    PatBindings::Named(binds) => {
                                        if let VariantFields::Named(fields) = &ev.fields {
                                            for (field_name, binding_name) in binds {
                                                if binding_name == "_" { continue; }
                                                let fty = fields.iter().find(|f| f.name == *field_name)
                                                    .map(|f| ty_str(&f.ty))
                                                    .unwrap_or_else(|| "int64_t".to_string());
                                                self.out.push_str(&format!(
                                                    "{bp}{fty} {binding_name} = {match_var}.data.{variant}.{field_name};\n"
                                                ));
                                                if let Some(f) = fields.iter().find(|f| f.name == *field_name) {
                                                    if let Ty::Named(n) = &f.ty {
                                                        arm_ctx.type_env.insert(binding_name.clone(), n.clone());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        self.out.push_str(&format!("{ip}case {type_name}_{variant}: {{\n"));
                    }
                }
            }

            for s in &arm.body.stmts { self.emit_stmt(s, &mut arm_ctx, indent + 2); }
            self.out.push_str(&format!("{bp}break;\n{ip}}}\n"));
        }

        self.out.push_str(&format!("{p}}}\n"));
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    /// Emit an expression, with an optional type hint for tuple literals.
    fn emit_expr_hint(&self, expr: &Expr, ctx: &mut Ctx, hint: Option<&Ty>) -> String {
        match expr {
            Expr::Tuple(elems) => {
                let elem_tys: Vec<Ty> = match hint {
                    Some(Ty::Tuple(tys)) => tys.clone(),
                    _ => elems.iter().map(|_| Ty::I64).collect(),
                };
                let name = tuple_typedef_name(&elem_tys);
                let fields: Vec<String> = elems.iter().enumerate()
                    .map(|(i, e)| format!("._{i} = {}", self.emit_expr_hint(e, ctx, elem_tys.get(i))))
                    .collect();
                format!("({name}){{{}}}", fields.join(", "))
            }
            _ => self.emit_expr(expr, ctx),
        }
    }

    fn emit_expr(&self, expr: &Expr, ctx: &mut Ctx) -> String {
        match expr {
            Expr::Int(n)  => format!("INT64_C({n})"),
            Expr::Float(f) => {
                // Emit with enough precision; always include decimal point
                if f.fract() == 0.0 { format!("{f:.1}") } else { format!("{f}") }
            }
            Expr::Char(c) => format!("UINT32_C({c})"),
            Expr::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
            Expr::Var(name) => name.clone(),

            Expr::Tuple(elems) if elems.is_empty() => "/* () */0".to_string(),
            Expr::Tuple(_) => self.emit_expr_hint(expr, ctx, None),

            Expr::StructLit { name, fields } => {
                let inits: Vec<String> = fields.iter()
                    .map(|(n, e)| format!(".{n} = {}", self.emit_expr(e, ctx)))
                    .collect();
                format!("({name}){{{}}}", inits.join(", "))
            }

            // Struct-like enum variant: `Type::Variant { x: e, ... }`
            // Look up field order in the enum declaration, emit constructor call.
            Expr::EnumStructLit { type_name, variant, fields } => {
                let mangled = format!("{type_name}_{variant}");
                let arg_exprs: Vec<String> = if let Some(edecl) = self.enums.get(type_name.as_str()) {
                    if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                        if let VariantFields::Named(decl_fields) = &ev.fields {
                            // Emit args in declared field order
                            decl_fields.iter().map(|df| {
                                let val = fields.iter().find(|(n, _)| n == &df.name)
                                    .map(|(_, e)| self.emit_expr(e, ctx))
                                    .unwrap_or_else(|| "0".to_string());
                                val
                            }).collect()
                        } else { vec![] }
                    } else { vec![] }
                } else { vec![] };
                format!("{mangled}({})", arg_exprs.join(", "))
            }

            Expr::Field { expr, field } => {
                let is_self_ptr = matches!(expr.as_ref(), Expr::Var(n) if n == "self") && ctx.ref_self;
                let obj = self.emit_expr(expr, ctx);
                // Numeric field name → tuple index `._N`; otherwise `.field` or `->field`
                if field.chars().all(|c| c.is_ascii_digit()) {
                    format!("{obj}._{field}")
                } else if is_self_ptr {
                    format!("self->{field}")
                } else {
                    format!("{obj}.{field}")
                }
            }

            Expr::Call { name, args } => {
                let param_tys = self.fn_params.get(name.as_str()).cloned().unwrap_or_default();
                let args_s: Vec<String> = args.iter().enumerate()
                    .map(|(i, a)| self.emit_expr_hint(a, ctx, param_tys.get(i)))
                    .collect();
                format!("{name}({})", args_s.join(", "))
            }

            Expr::AssocCall { type_name, method, args } => {
                let is_enum = self.enums.contains_key(type_name.as_str());
                let is_tagged_enum = is_enum && self.enums.get(type_name.as_str())
                    .map_or(false, |e| e.variants.iter().any(|v| !matches!(v.fields, VariantFields::Unit)));

                if is_enum && !is_tagged_enum {
                    // Simple C enum: emit as constant
                    format!("{type_name}_{method}")
                } else {
                    let mangled = format!("{type_name}_{method}");
                    let param_tys = self.fn_params.get(&mangled).cloned().unwrap_or_default();
                    let args_s: Vec<String> = args.iter().enumerate()
                        .map(|(i, a)| self.emit_expr_hint(a, ctx, param_tys.get(i)))
                        .collect();
                    if args.is_empty() {
                        if is_tagged_enum { format!("{type_name}_{method}()") }
                        else { format!("{type_name}_{method}") }
                    } else {
                        format!("{type_name}_{method}({})", args_s.join(", "))
                    }
                }
            }

            Expr::MethodCall { expr, method, args } => {
                let type_name = match expr.as_ref() {
                    Expr::Var(n) => ctx.type_env.get(n.as_str()).cloned(),
                    _ => None,
                };
                let args_s: Vec<String> = args.iter().map(|a| self.emit_expr(a, ctx)).collect();
                let expr_c = self.emit_expr(expr, ctx);

                match type_name {
                    Some(t) => {
                        let self_arg = if matches!(expr.as_ref(), Expr::Var(n) if n == "self") && ctx.ref_self {
                            expr_c
                        } else {
                            format!("&({expr_c})")
                        };
                        if args_s.is_empty() { format!("{t}_{method}({self_arg})") }
                        else { format!("{t}_{method}({self_arg}, {})", args_s.join(", ")) }
                    }
                    None => format!("/* unknown type */{expr_c}.{method}({})", args_s.join(", ")),
                }
            }

            Expr::UnOp { op, operand } => {
                let op_s = match op { UnOp::Neg => "-", UnOp::Not => "!", UnOp::BitNot => "~" };
                format!("({op_s}{})", self.emit_expr(operand, ctx))
            }

            Expr::BinOp { op, lhs, rhs } => {
                let op_s = match op {
                    BinOp::Add => "+",  BinOp::Sub => "-",  BinOp::Mul => "*",
                    BinOp::Div => "/",  BinOp::Rem => "%",
                    BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                    BinOp::Shl => "<<", BinOp::Shr => ">>",
                    BinOp::Eq  => "==", BinOp::Ne  => "!=",
                    BinOp::Lt  => "<",  BinOp::Gt  => ">",
                    BinOp::Le  => "<=", BinOp::Ge  => ">=",
                    BinOp::And => "&&", BinOp::Or  => "||",
                };
                format!("({} {op_s} {})", self.emit_expr(lhs, ctx), self.emit_expr(rhs, ctx))
            }

            Expr::AddrOf { mutable, expr } => {
                let _ = mutable; // semantics preserved in C type; no runtime difference
                format!("(&{})", self.emit_expr(expr, ctx))
            }

            Expr::Deref(expr) => {
                format!("(*{})", self.emit_expr(expr, ctx))
            }

            Expr::Cast { expr, ty } => {
                format!("(({})({}))", ty_str(ty), self.emit_expr(expr, ctx))
            }

            // `unsafe { stmts }` — emit as a GNU statement expression `({ stmts })`
            // or just emit the block inline (last expr becomes the value).
            Expr::Unsafe(block) => {
                if block.stmts.is_empty() {
                    "/* unsafe {} */".to_string()
                } else if block.stmts.len() == 1 {
                    // Single expression inside unsafe: emit directly
                    match &block.stmts[0] {
                        Stmt::Return(Some(e)) | Stmt::Expr(e) => self.emit_expr(e, ctx),
                        _ => "/* unsafe block */".to_string(),
                    }
                } else {
                    // Multi-statement unsafe: use GCC/Clang statement expression
                    let mut out = "({ ".to_string();
                    for (i, s) in block.stmts.iter().enumerate() {
                        match s {
                            Stmt::Return(Some(e)) if i + 1 == block.stmts.len() => {
                                out.push_str(&self.emit_expr(e, ctx));
                                out.push_str("; ");
                            }
                            Stmt::Expr(e) if i + 1 == block.stmts.len() => {
                                out.push_str(&self.emit_expr(e, ctx));
                                out.push_str("; ");
                            }
                            _ => {} // complex stmts in unsafe: skip for now
                        }
                    }
                    out.push_str("})");
                    out
                }
            }
        }
    }

    fn emit_println(&self, format: &str, args: &[Expr], ctx: &mut Ctx) -> String {
        let mut fmt_parts: Vec<String> = Vec::new();
        let mut fmt_c = String::new();
        let mut chars = format.chars().peekable();
        let mut arg_idx = 0usize;
        while let Some(ch) = chars.next() {
            if ch == '{' && chars.peek() == Some(&'}') {
                chars.next();
                let spec = if let Some(arg) = args.get(arg_idx) {
                    printf_spec(arg, ctx)
                } else {
                    "%lld".to_string()
                };
                fmt_c.push_str(&spec);
                fmt_parts.push(spec);
                arg_idx += 1;
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
            format!("printf(\"{fmt_c}\\n\");")
        } else {
            let args_s: Vec<String> = args.iter().enumerate()
                .map(|(i, a)| {
                    let spec = fmt_parts.get(i).map(|s| s.as_str()).unwrap_or("%lld");
                    let e = self.emit_expr(a, ctx);
                    if spec == "%f" {
                        format!("(double)({e})")
                    } else if spec == "%u" {
                        format!("(unsigned int)({e})")
                    } else {
                        format!("(long long)({e})")
                    }
                })
                .collect();
            format!("printf(\"{fmt_c}\\n\", {});", args_s.join(", "))
        }
    }
}

/// Choose printf format specifier and cast for an expression.
fn printf_spec(expr: &Expr, ctx: &Ctx) -> String {
    match expr {
        Expr::Float(_) => "%f".to_string(),
        Expr::Char(_)  => "%u".to_string(),
        Expr::Cast { ty: Ty::F32, .. } | Expr::Cast { ty: Ty::F64, .. } => "%f".to_string(),
        Expr::Cast { ty: Ty::Char, .. } => "%u".to_string(),
        Expr::Var(name) => match ctx.var_types.get(name) {
            Some(Ty::F32) | Some(Ty::F64) => "%f".to_string(),
            Some(Ty::Char) => "%u".to_string(),
            _ => "%lld".to_string(),
        },
        _ => "%lld".to_string(),
    }
}

fn collect_tuple_types(file: &File) -> Vec<Vec<Ty>> {
    let mut found: Vec<Vec<Ty>> = Vec::new();
    for item in &file.items {
        match item {
            Item::Fn(f) => {
                scan_ty(&f.return_ty, &mut found);
                for p in &f.params { scan_ty(&p.ty, &mut found); }
                scan_block(&f.body, &mut found);
            }
            Item::Struct(s) => { for f in &s.fields { scan_ty(&f.ty, &mut found); } }
            Item::Enum(e)   => {
                for v in &e.variants {
                    match &v.fields {
                        VariantFields::Unit => {}
                        VariantFields::Tuple(tys) => { for ty in tys { scan_ty(ty, &mut found); } }
                        VariantFields::Named(fields) => { for f in fields { scan_ty(&f.ty, &mut found); } }
                    }
                }
            }
            Item::Impl(imp) => {
                for m in &imp.methods {
                    scan_ty(&m.return_ty, &mut found);
                    for p in &m.params { scan_ty(&p.ty, &mut found); }
                    scan_block(&m.body, &mut found);
                }
            }
        }
    }
    found
}

fn scan_ty(ty: &Ty, found: &mut Vec<Vec<Ty>>) {
    match ty {
        Ty::Tuple(tys) => {
            if !found.iter().any(|f| f == tys) { found.push(tys.clone()); }
            for t in tys { scan_ty(t, found); }
        }
        Ty::Ref(inner) | Ty::RefMut(inner) | Ty::RawConst(inner) | Ty::RawMut(inner) => {
            scan_ty(inner, found);
        }
        _ => {}
    }
}

fn scan_block(block: &crate::ast::Block, found: &mut Vec<Vec<Ty>>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { ty, expr, .. } => {
                if let Some(t) = ty { scan_ty(t, found); }
                scan_expr(expr, ty.as_ref(), found);
            }
            Stmt::If { cond, then_block, else_block } => {
                scan_expr(cond, None, found);
                scan_block(then_block, found);
                if let Some(b) = else_block { scan_block(b, found); }
            }
            Stmt::While { cond, body } => { scan_expr(cond, None, found); scan_block(body, found); }
            Stmt::Match { expr, arms } => {
                scan_expr(expr, None, found);
                for arm in arms { scan_block(&arm.body, found); }
            }
            Stmt::Return(Some(e)) => scan_expr(e, None, found),
            Stmt::Println { args, .. } => { for a in args { scan_expr(a, None, found); } }
            Stmt::Expr(e) => scan_expr(e, None, found),
            _ => {}
        }
    }
}

fn scan_expr(expr: &Expr, hint: Option<&Ty>, found: &mut Vec<Vec<Ty>>) {
    match expr {
        Expr::Tuple(elems) => {
            let tys: Vec<Ty> = match hint {
                Some(Ty::Tuple(tys)) => tys.clone(),
                _ => elems.iter().map(|_| Ty::I64).collect(),
            };
            if !found.iter().any(|f| f == &tys) { found.push(tys); }
            for e in elems { scan_expr(e, None, found); }
        }
        Expr::BinOp { lhs, rhs, .. } => { scan_expr(lhs, None, found); scan_expr(rhs, None, found); }
        Expr::UnOp { operand, .. }   => scan_expr(operand, None, found),
        Expr::Call { args, .. }      => { for a in args { scan_expr(a, None, found); } }
        Expr::AssocCall { args, .. } => { for a in args { scan_expr(a, None, found); } }
        Expr::MethodCall { expr, args, .. } => {
            scan_expr(expr, None, found);
            for a in args { scan_expr(a, None, found); }
        }
        Expr::Field { expr, .. }          => scan_expr(expr, None, found),
        Expr::StructLit { fields, .. }    => { for (_, e) in fields { scan_expr(e, None, found); } }
        Expr::EnumStructLit { fields, .. } => { for (_, e) in fields { scan_expr(e, None, found); } }
        Expr::AddrOf { expr, .. }  => scan_expr(expr, None, found),
        Expr::Deref(expr)          => scan_expr(expr, None, found),
        Expr::Cast { expr, ty }    => { scan_expr(expr, None, found); scan_ty(ty, found); }
        Expr::Unsafe(block)        => scan_block(block, found),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers: type strings, signatures, padding
// ---------------------------------------------------------------------------

fn ty_str(ty: &Ty) -> String {
    match ty {
        Ty::I8    => "int8_t".into(),   Ty::I16  => "int16_t".into(),
        Ty::I32   => "int32_t".into(),  Ty::I64  => "int64_t".into(),  Ty::Isize => "intptr_t".into(),
        Ty::U8    => "uint8_t".into(),  Ty::U16  => "uint16_t".into(),
        Ty::U32   => "uint32_t".into(), Ty::U64  => "uint64_t".into(), Ty::Usize => "uintptr_t".into(),
        Ty::F32   => "float".into(),    Ty::F64  => "double".into(),
        Ty::Bool  => "bool".into(),
        Ty::Char  => "uint32_t".into(),
        Ty::Unit  => "void".into(),
        Ty::Named(n) => n.clone(),
        Ty::Tuple(tys) => tuple_typedef_name(tys),
        Ty::Ref(inner)      => format!("const {}*", ty_str(inner)),
        Ty::RefMut(inner)   => format!("{}*", ty_str(inner)),
        Ty::RawConst(inner) => format!("const {}*", ty_str(inner)),
        Ty::RawMut(inner)   => format!("{}*", ty_str(inner)),
    }
}

fn ty_key(ty: &Ty) -> String {
    match ty {
        Ty::I8    => "i8".into(),   Ty::I16  => "i16".into(),
        Ty::I32   => "i32".into(),  Ty::I64  => "i64".into(),  Ty::Isize => "isize".into(),
        Ty::U8    => "u8".into(),   Ty::U16  => "u16".into(),
        Ty::U32   => "u32".into(),  Ty::U64  => "u64".into(),  Ty::Usize => "usize".into(),
        Ty::F32   => "f32".into(),  Ty::F64  => "f64".into(),
        Ty::Bool  => "bool".into(), Ty::Char => "char".into(), Ty::Unit => "unit".into(),
        Ty::Named(n) => n.clone(),
        Ty::Tuple(tys)      => format!("({})", tys.iter().map(ty_key).collect::<Vec<_>>().join("_")),
        Ty::Ref(inner)      => format!("ref_{}", ty_key(inner)),
        Ty::RefMut(inner)   => format!("refmut_{}", ty_key(inner)),
        Ty::RawConst(inner) => format!("ptr_{}", ty_key(inner)),
        Ty::RawMut(inner)   => format!("ptrm_{}", ty_key(inner)),
    }
}

fn tuple_typedef_name(tys: &[Ty]) -> String {
    format!("Tuple_{}", tys.iter().map(ty_key).collect::<Vec<_>>().join("_"))
}

fn emit_tuple_typedef(out: &mut String, tys: &[Ty]) {
    let name = tuple_typedef_name(tys);
    out.push_str("typedef struct {\n");
    for (i, ty) in tys.iter().enumerate() {
        out.push_str(&format!("    {} _{};\n", ty_str(ty), i));
    }
    out.push_str(&format!("}} {name};\n"));
}

fn emit_struct(out: &mut String, s: &StructDecl) {
    out.push_str(&format!("typedef struct {} {{\n", s.name));
    for f in &s.fields {
        out.push_str(&format!("    {} {};\n", ty_str(&f.ty), f.name));
    }
    out.push_str(&format!("}} {};\n", s.name));
}

fn fn_signature(f: &FnDecl, impl_type: Option<&str>) -> String {
    if f.name == "main" { return "int main(void)".to_string(); }

    let ret = ty_str(&f.return_ty);
    let mut param_parts: Vec<String> = Vec::new();

    if let (Some(recv), Some(itype)) = (&f.receiver, impl_type) {
        let self_param = match recv {
            Receiver::Value  => format!("{itype}* self"),
            Receiver::Ref    => format!("const {itype}* self"),
            Receiver::RefMut => format!("{itype}* self"),
        };
        param_parts.push(self_param);
    }
    for p in &f.params {
        param_parts.push(format!("{} {}", ty_str(&p.ty), p.name));
    }
    let params = if param_parts.is_empty() { "void".to_string() } else { param_parts.join(", ") };

    let mangled = match impl_type {
        Some(t) => format!("{t}_{}", f.name),
        None    => f.name.clone(),
    };
    format!("{ret} {mangled}({params})")
}

fn pad(indent: usize) -> String { "    ".repeat(indent) }

// ---------------------------------------------------------------------------
// Tuple typedef collection (pre-scan)
// ---------------------------------------------------------------------------
