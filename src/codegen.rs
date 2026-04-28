use crate::ast::{
    BinOp, Block, EnumDecl, EnumVariant, Expr, ExprKind, ExternFnDecl, File, FnDecl, ImplBlock,
    Item, MatchArm, Pat, PatBindings, Receiver, Stmt, StmtKind, StructDecl, TraitDecl, Ty, UnOp,
    VariantFields,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Codegen context (per-function state)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    /// True when `self` is passed by reference (i.e. `&self` or `&mut self` receiver).
    self_by_ref: bool,
    /// Maps variable names to their struct/enum type name, for method call resolution.
    type_env: HashMap<String, String>,
    /// Maps variable names to their type, for printf format specifier selection.
    var_types: HashMap<String, Ty>,
    /// Return type of the current function, used to hint tuple literal emission.
    return_ty: Option<Ty>,
    /// Function/method return types shared from Codegen, for printf spec on call expressions.
    fn_ret_tys: HashMap<String, Ty>,
    /// Type aliases shared from Codegen, for resolving Named types in printf spec.
    type_aliases: HashMap<String, Ty>,
    /// Methods whose self parameter is by value (not a pointer), for correct call-site emission.
    value_self_fns: std::collections::HashSet<String>,
    /// Variable name used to capture `break val` inside a `loop { }` expression.
    loop_result_var: Option<String>,
    /// Nesting depth of `loop`/`while` statement loops, used to gate loop_result_var.
    loop_depth: usize,
    /// True when emitting the body of `fn main()`, so `return;` emits `return 0;`.
    in_main: bool,
}

impl Ctx {
    fn new() -> Self {
        Self {
            self_by_ref: false,
            type_env: HashMap::new(),
            var_types: HashMap::new(),
            return_ty: None,
            fn_ret_tys: HashMap::new(),
            type_aliases: HashMap::new(),
            value_self_fns: std::collections::HashSet::new(),
            loop_result_var: None,
            loop_depth: 0,
            in_main: false,
        }
    }

    fn for_method(
        impl_type: &str,
        receiver: &Receiver,
        params_type_env: HashMap<String, String>,
    ) -> Self {
        let self_by_ref = matches!(receiver, Receiver::Ref | Receiver::RefMut);
        let mut type_env = params_type_env;
        type_env.insert("self".to_string(), impl_type.to_string());
        Self {
            self_by_ref,
            type_env,
            var_types: HashMap::new(),
            return_ty: None,
            fn_ret_tys: HashMap::new(),
            type_aliases: HashMap::new(),
            value_self_fns: std::collections::HashSet::new(),
            loop_result_var: None,
            loop_depth: 0,
            in_main: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Codegen struct (owns all state)
// ---------------------------------------------------------------------------

struct Codegen {
    /// Enum declarations indexed by name, for tagged union resolution in match.
    enums: HashMap<String, EnumDecl>,
    /// Struct declarations indexed by name, for field type lookup in printf_spec.
    structs: HashMap<String, StructDecl>,
    /// Function/method param types (mangled name -> param type list) for tuple arg hints.
    fn_params: HashMap<String, Vec<Ty>>,
    /// Function/method return types (mangled name -> return Ty) for printf spec inference.
    fn_ret_tys: HashMap<String, Ty>,
    /// Type aliases (alias name -> underlying Ty) for printf spec resolution.
    type_aliases: HashMap<String, Ty>,
    /// Functions that return `!` — calls to these inside a return emit as statements.
    never_fns: std::collections::HashSet<String>,
    /// Methods whose `self` is passed by value (not a pointer) in the C signature.
    value_self_fns: std::collections::HashSet<String>,
    /// Maps (concrete_type_name, method_name) -> trampoline symbol for trait dispatch.
    trait_method_trampolines: HashMap<String, HashMap<String, String>>,
    /// Global constants and statics with their types, for printf format specifier selection.
    const_types: HashMap<String, Ty>,
    /// Counter for generating unique temporary variable names.
    tmp_counter: usize,
    out: String,
}

pub fn generate(file: &File) -> String {
    let mut enums: HashMap<String, EnumDecl> = HashMap::new();
    let mut structs: HashMap<String, StructDecl> = HashMap::new();
    let mut fn_params: HashMap<String, Vec<Ty>> = HashMap::new();
    let mut fn_ret_tys: HashMap<String, Ty> = HashMap::new();
    let mut type_aliases: HashMap<String, Ty> = HashMap::new();
    let mut never_fns = std::collections::HashSet::new();
    let mut value_self_fns = std::collections::HashSet::new();
    let mut traits: HashMap<String, TraitDecl> = HashMap::new();
    let mut trait_method_trampolines: HashMap<String, HashMap<String, String>> = HashMap::new();
    collect_type_aliases(&file.items, &mut type_aliases);
    collect_traits(&file.items, &mut traits);
    collect_items(
        &file.items,
        "",
        &mut enums,
        &mut structs,
        &mut fn_params,
        &mut fn_ret_tys,
        &mut never_fns,
        &mut value_self_fns,
        &mut trait_method_trampolines,
    );
    // Register dyn trait method return types as "dyn_TraitName_method" for printf spec.
    for (trait_name, tr) in &traits {
        for m in &tr.methods {
            fn_ret_tys.insert(format!("dyn_{trait_name}_{}", m.name), m.return_ty.clone());
        }
    }
    let mut cg = Codegen {
        enums,
        structs,
        fn_params,
        fn_ret_tys,
        type_aliases,
        never_fns,
        value_self_fns,
        trait_method_trampolines,
        const_types: HashMap::new(),
        tmp_counter: 0,
        out: String::new(),
    };
    cg.run(file);
    cg.out
}

fn collect_type_aliases(items: &[Item], aliases: &mut HashMap<String, Ty>) {
    for item in items {
        if let Item::TypeAlias { name, ty, .. } = item {
            aliases.insert(name.clone(), ty.clone());
        }
    }
}

/// Collect all trait declarations from the item list (recursing into mods).
fn collect_traits(items: &[Item], traits: &mut HashMap<String, TraitDecl>) {
    for item in items {
        match item {
            Item::Trait(t) => {
                traits.insert(t.name.clone(), t.clone());
            }
            Item::Mod {
                items: mod_items, ..
            } => collect_traits(mod_items, traits),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_items(
    items: &[Item],
    prefix: &str,
    enums: &mut HashMap<String, EnumDecl>,
    structs: &mut HashMap<String, StructDecl>,
    fn_params: &mut HashMap<String, Vec<Ty>>,
    fn_ret_tys: &mut HashMap<String, Ty>,
    never_fns: &mut std::collections::HashSet<String>,
    value_self_fns: &mut std::collections::HashSet<String>,
    trait_method_trampolines: &mut HashMap<String, HashMap<String, String>>,
) {
    for item in items {
        match item {
            Item::Fn(f) => {
                let name = format!("{prefix}{}", f.name);
                fn_params.insert(
                    name.clone(),
                    f.params.iter().map(|p| p.ty.clone()).collect(),
                );
                fn_ret_tys.insert(name.clone(), f.return_ty.clone());
                if f.return_ty == Ty::Never {
                    never_fns.insert(name);
                }
            }
            Item::Impl(imp) => {
                let type_name = format!("{prefix}{}", imp.type_name);
                for m in &imp.methods {
                    let ret_ty = m.return_ty.resolve_self(&type_name);
                    let param_tys: Vec<Ty> = m
                        .params
                        .iter()
                        .map(|p| p.ty.resolve_self(&type_name))
                        .collect();
                    if let Some(trait_name) = &imp.trait_name {
                        // Trait impl: trampoline symbol is TypeName__TraitName__method.
                        let trampoline = format!("{type_name}__{trait_name}__{}", m.name);
                        fn_ret_tys.insert(trampoline.clone(), ret_ty);
                        fn_params.insert(trampoline.clone(), param_tys);
                        trait_method_trampolines
                            .entry(type_name.clone())
                            .or_default()
                            .insert(m.name.clone(), trampoline);
                    } else {
                        // Inherent impl: mangled as TypeName_method.
                        let mangled = format!("{type_name}_{}", m.name);
                        if ret_ty == Ty::Never {
                            never_fns.insert(mangled.clone());
                        }
                        fn_params.insert(mangled.clone(), param_tys);
                        fn_ret_tys.insert(mangled.clone(), ret_ty);
                        if matches!(m.receiver, Some(Receiver::Value)) {
                            value_self_fns.insert(mangled);
                        }
                    }
                }
            }
            Item::Struct(s) => {
                let name = format!("{prefix}{}", s.name);
                let mut prefixed = s.clone();
                prefixed.name = name.clone();
                // Register tuple struct constructors as functions for type-hint emission.
                if s.is_tuple {
                    fn_params.insert(
                        name.clone(),
                        s.fields.iter().map(|f| f.ty.clone()).collect(),
                    );
                    fn_ret_tys.insert(name.clone(), Ty::Named(name.clone()));
                }
                structs.insert(name, prefixed);
            }
            Item::Enum(e) => {
                let name = format!("{prefix}{}", e.name);
                let mut prefixed = e.clone();
                prefixed.name = name.clone();
                enums.insert(name, prefixed);
            }
            Item::Mod {
                name,
                items: mod_items,
                ..
            } => {
                collect_items(
                    mod_items,
                    &format!("{prefix}{name}_"),
                    enums,
                    structs,
                    fn_params,
                    fn_ret_tys,
                    never_fns,
                    value_self_fns,
                    trait_method_trampolines,
                );
            }
            _ => {}
        }
    }
}

impl Codegen {
    fn run(&mut self, file: &File) {
        self.out
            .push_str("#include <stdbool.h>\n#include <stdint.h>\n#include <stdio.h>\n");

        // Collect and emit tuple typedefs (pre-scan)
        let tuple_types = collect_tuple_types(file);
        for tys in &tuple_types {
            self.out.push('\n');
            emit_tuple_typedef(&mut self.out, tys);
        }

        // Emit type aliases, struct and enum type definitions
        self.emit_type_defs(&file.items, "");

        // Emit vtable structs and dyn fat-pointer structs for all traits
        self.emit_trait_type_defs(&file.items, "");

        // Forward declarations
        self.out.push('\n');
        self.emit_forward_decls(&file.items, "");

        // Function / method definitions
        self.out.push('\n');
        self.emit_items(&file.items, "");
    }

    fn emit_type_defs(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::TypeAlias { name, ty, .. } => {
                    self.out.push('\n');
                    let prefixed = format!("{prefix}{name}");
                    let decl = ty_str_decl(ty, &prefixed);
                    self.out.push_str(&format!("typedef {decl};\n"));
                }
                Item::Struct(s) => {
                    self.out.push('\n');
                    emit_struct(&mut self.out, s, prefix);
                }
                Item::Enum(e) => {
                    self.out.push('\n');
                    self.emit_enum(e, prefix);
                }
                Item::Const { name, ty, expr, .. } => {
                    self.out.push('\n');
                    let prefixed = format!("{prefix}{name}");
                    let decl = ty_str_decl(ty, &prefixed);
                    let mut ctx = Ctx::new();
                    let val = self.emit_expr(expr, &mut ctx);
                    self.const_types.insert(prefixed.clone(), ty.clone());
                    self.out
                        .push_str(&format!("static const {decl} = {val};\n"));
                }
                Item::Static {
                    name,
                    ty,
                    expr,
                    mutable,
                    ..
                } => {
                    self.out.push('\n');
                    let prefixed = format!("{prefix}{name}");
                    let decl = ty_str_decl(ty, &prefixed);
                    let mut ctx = Ctx::new();
                    let val = self.emit_expr(expr, &mut ctx);
                    let kw = if *mutable { "" } else { "const " };
                    self.const_types.insert(prefixed.clone(), ty.clone());
                    self.out.push_str(&format!("static {kw}{decl} = {val};\n"));
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.emit_type_defs(mod_items, &format!("{prefix}{name}_"));
                }
                _ => {}
            }
        }
    }

    /// Emit vtable struct typedefs and `dyn_Trait` fat-pointer structs for every trait.
    /// Must be called after `emit_type_defs` so that method param/return types are available.
    fn emit_trait_type_defs(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Trait(t) => {
                    let full = format!("{prefix}{}", t.name);
                    // Vtable struct: one function-pointer field per method.
                    self.out.push_str("\ntypedef struct {\n");
                    for m in &t.methods {
                        let ret = ty_str(&m.return_ty);
                        let params: Vec<String> = std::iter::once("void* _self".to_string())
                            .chain(m.params.iter().map(|p| ty_str(&p.ty)))
                            .collect();
                        self.out.push_str(&format!(
                            "    {ret} (*{})({});\n",
                            m.name,
                            params.join(", ")
                        ));
                    }
                    self.out.push_str(&format!("}} {full}_vtable;\n"));
                    // Fat pointer: { void* data; const Trait_vtable* vtable; }
                    self.out.push_str(&format!(
                        "typedef struct {{ void* data; const {full}_vtable* vtable; }} dyn_{full};\n"
                    ));
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.emit_trait_type_defs(mod_items, &format!("{prefix}{name}_"));
                }
                _ => {}
            }
        }
    }

    fn emit_forward_decls(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Fn(f) if f.name != "main" => {
                    self.out
                        .push_str(&format!("{};\n", fn_signature(f, None, prefix)));
                }
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    if let Some(trait_name) = &imp.trait_name {
                        // Trait impl: forward-declare trampolines.
                        for m in &imp.methods {
                            let trampoline = format!("{type_name}__{trait_name}__{}", m.name);
                            let ret = ty_str(&m.return_ty.resolve_self(&type_name));
                            let params: Vec<String> =
                                std::iter::once("void* _self".to_string())
                                    .chain(m.params.iter().map(|p| {
                                        ty_str_decl(&p.ty.resolve_self(&type_name), &p.name)
                                    }))
                                    .collect();
                            self.out.push_str(&format!(
                                "static {ret} {trampoline}({});\n",
                                params.join(", ")
                            ));
                        }
                    } else {
                        // Inherent impl: forward-declare under TypeName_method.
                        for m in &imp.methods {
                            self.out
                                .push_str(&format!("{};\n", fn_signature(m, Some(&type_name), "")));
                        }
                    }
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.emit_forward_decls(mod_items, &format!("{prefix}{name}_"));
                }
                Item::ExternBlock(fns) => {
                    for f in fns {
                        self.out.push_str(&extern_fn_decl(f));
                        self.out.push('\n');
                    }
                }
                _ => {}
            }
        }
    }

    fn emit_items(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Fn(f) => self.emit_fn(f, None, prefix),
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    if let Some(trait_name) = &imp.trait_name {
                        // Emit trampolines + static vtable instance.
                        self.emit_trait_impl(imp, &type_name, trait_name);
                    } else {
                        // Inherent impl: emit methods under TypeName_method.
                        for m in &imp.methods {
                            self.emit_fn(m, Some(&type_name), "");
                        }
                    }
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.emit_items(mod_items, &format!("{prefix}{name}_"));
                }
                Item::Struct(_)
                | Item::Enum(_)
                | Item::TypeAlias { .. }
                | Item::Trait(_)
                | Item::ExternBlock(_)
                | Item::Const { .. }
                | Item::Static { .. }
                | Item::Skip => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Struct and enum emission
    // -----------------------------------------------------------------------

    fn emit_enum(&mut self, e: &EnumDecl, prefix: &str) {
        let name = format!("{prefix}{}", e.name);
        let has_data = e
            .variants
            .iter()
            .any(|v| !matches!(v.fields, VariantFields::Unit));
        if !has_data {
            // Simple C enum — all unit variants
            self.out.push_str("typedef enum {\n");
            for (i, v) in e.variants.iter().enumerate() {
                let comma = if i + 1 < e.variants.len() { "," } else { "" };
                self.out
                    .push_str(&format!("    {}_{} = {}{}\n", name, v.name, i, comma));
            }
            self.out.push_str(&format!("}} {name};\n"));
        } else {
            // Tagged union: tag enum + struct with union data + static inline constructors
            self.out.push_str("typedef enum {\n");
            for (i, v) in e.variants.iter().enumerate() {
                let comma = if i + 1 < e.variants.len() { "," } else { "" };
                self.out
                    .push_str(&format!("    {}_{}_tag = {}{}\n", name, v.name, i, comma));
            }
            self.out.push_str(&format!("}} {name}_tag;\n"));

            self.out.push_str(&format!(
                "typedef struct {{\n    {name}_tag tag;\n    union {{\n"
            ));
            for v in &e.variants {
                match &v.fields {
                    VariantFields::Unit => {}
                    VariantFields::Tuple(tys) => {
                        self.out.push_str("        struct {\n");
                        for (i, ty) in tys.iter().enumerate() {
                            self.out
                                .push_str(&format!("            {} _{};\n", ty_str(ty), i));
                        }
                        self.out.push_str(&format!("        }} {};\n", v.name));
                    }
                    VariantFields::Named(fields) => {
                        self.out.push_str("        struct {\n");
                        for f in fields {
                            self.out.push_str(&format!(
                                "            {} {};\n",
                                ty_str(&f.ty),
                                f.name
                            ));
                        }
                        self.out.push_str(&format!("        }} {};\n", v.name));
                    }
                }
            }
            self.out.push_str(&format!("    }} data;\n}} {name};\n"));

            // Static inline constructor functions (safe for complex arg expressions)
            for v in &e.variants {
                self.emit_enum_constructor(&name, e, v);
            }
        }
    }

    fn emit_enum_constructor(&mut self, name: &str, _e: &EnumDecl, v: &EnumVariant) {
        let tag = format!("{name}_{}_tag", v.name);
        let mangled = format!("{name}_{}", v.name);
        match &v.fields {
            VariantFields::Unit => {
                self.out.push_str(&format!(
                    "static inline {name} {mangled}(void) {{ return ({name}){{ .tag = {tag} }}; }}\n",
                ));
            }
            VariantFields::Tuple(tys) => {
                let params: Vec<String> = tys
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| format!("{} _{i}", ty_str(ty)))
                    .collect();
                let inits: Vec<String> = (0..tys.len())
                    .map(|i| format!(".data.{vname}._{i} = _{i}", vname = v.name))
                    .collect();
                self.out.push_str(&format!(
                    "static inline {name} {mangled}({params}) {{ return ({name}){{ .tag = {tag}, {inits} }}; }}\n",
                    params = params.join(", "), inits = inits.join(", ")
                ));
                self.fn_params.insert(mangled, tys.clone());
            }
            VariantFields::Named(fields) => {
                let params: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{} {}", ty_str(&f.ty), f.name))
                    .collect();
                let inits: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        format!(
                            ".data.{vname}.{fname} = {fname}",
                            vname = v.name,
                            fname = f.name
                        )
                    })
                    .collect();
                self.out.push_str(&format!(
                    "static inline {name} {mangled}({params}) {{ return ({name}){{ .tag = {tag}, {inits} }}; }}\n",
                    params = params.join(", "), inits = inits.join(", ")
                ));
                self.fn_params
                    .insert(mangled, fields.iter().map(|f| f.ty.clone()).collect());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Functions and methods
    // -----------------------------------------------------------------------

    fn emit_fn(&mut self, f: &FnDecl, impl_type: Option<&str>, prefix: &str) {
        let sig = fn_signature(f, impl_type, prefix);
        self.out.push_str(&format!("{sig} {{\n"));

        let mut params_env = HashMap::new();
        let mut params_var_types = HashMap::new();
        for p in &f.params {
            let ty = match impl_type {
                Some(itype) => p.ty.resolve_self(itype),
                None => p.ty.clone(),
            };
            if let Ty::Named(n) = &ty {
                params_env.insert(p.name.clone(), n.clone());
            }
            params_var_types.insert(p.name.clone(), ty);
        }

        let mut ctx = match (&f.receiver, impl_type) {
            (Some(recv), Some(itype)) => Ctx::for_method(itype, recv, params_env),
            _ => Ctx {
                type_env: params_env,
                ..Ctx::new()
            },
        };
        ctx.var_types = params_var_types;
        ctx.return_ty = Some(match impl_type {
            Some(itype) => f.return_ty.resolve_self(itype),
            None => f.return_ty.clone(),
        });
        ctx.fn_ret_tys = self.fn_ret_tys.clone();
        ctx.type_aliases = self.type_aliases.clone();
        ctx.value_self_fns = self.value_self_fns.clone();
        if f.name == "main" {
            ctx.in_main = true;
        }

        // Emit all statements. If the tail is a `loop` in a non-void function,
        // emit it so the loop result is returned.
        let ret_ty = f.return_ty.resolve_self(impl_type.unwrap_or(""));
        let stmts = &f.body.stmts;
        let needs_loop_return = ret_ty != Ty::Unit
            && f.name != "main"
            && matches!(stmts.last().map(|s| &s.kind), Some(StmtKind::Loop(_)));

        if needs_loop_return {
            let (rest, last) = stmts.split_at(stmts.len() - 1);
            for stmt in rest {
                self.emit_stmt(stmt, &mut ctx, 1);
            }
            // Emit the loop with a result var, then return that var.
            ctx.loop_result_var = Some("_loop_ret".to_string());
            let ret_ty_s = ty_str(&ret_ty);
            self.out.push_str(&format!("    {ret_ty_s} _loop_ret;\n"));
            self.emit_stmt(&last[0], &mut ctx, 1);
            ctx.loop_result_var = None;
            self.out.push_str("    return _loop_ret;\n");
        } else {
            for stmt in stmts {
                self.emit_stmt(stmt, &mut ctx, 1);
            }
            if f.name == "main" {
                self.out.push_str("    return 0;\n");
            }
        }
        self.out.push_str("}\n\n");
    }

    /// Emit trampoline functions and a static vtable instance for `impl TraitName for TypeName`.
    fn emit_trait_impl(&mut self, imp: &ImplBlock, type_name: &str, trait_name: &str) {
        for m in &imp.methods {
            let trampoline = format!("{type_name}__{trait_name}__{}", m.name);
            let ret_ty = m.return_ty.resolve_self(type_name);
            let ret = ty_str(&ret_ty);
            let params: Vec<String> = std::iter::once("void* _self".to_string())
                .chain(
                    m.params
                        .iter()
                        .map(|p| ty_str_decl(&p.ty.resolve_self(type_name), &p.name)),
                )
                .collect();
            self.out.push_str(&format!(
                "static {ret} {trampoline}({}) {{\n",
                params.join(", ")
            ));
            // Always use pointer-to-struct for self inside the trampoline.
            let const_kw = if matches!(m.receiver, Some(Receiver::Ref)) {
                "const "
            } else {
                ""
            };
            self.out.push_str(&format!(
                "    {const_kw}{type_name}* self = ({const_kw}{type_name}*)_self;\n"
            ));

            // Build context for the method body.
            let mut params_env = HashMap::new();
            let mut params_var_types = HashMap::new();
            for p in &m.params {
                let ty = p.ty.resolve_self(type_name);
                if let Ty::Named(n) = &ty {
                    params_env.insert(p.name.clone(), n.clone());
                }
                params_var_types.insert(p.name.clone(), ty);
            }
            let mut ctx = Ctx::for_method(type_name, &Receiver::Ref, params_env);
            ctx.var_types = params_var_types;
            ctx.return_ty = Some(ret_ty);
            ctx.fn_ret_tys = self.fn_ret_tys.clone();
            ctx.type_aliases = self.type_aliases.clone();
            ctx.value_self_fns = self.value_self_fns.clone();
            for stmt in &m.body.stmts {
                self.emit_stmt(stmt, &mut ctx, 1);
            }
            self.out.push_str("}\n\n");
        }

        // Static vtable instance after all trampolines.
        self.out.push_str(&format!(
            "static const {trait_name}_vtable {type_name}__{trait_name}__vtable = {{\n"
        ));
        for m in &imp.methods {
            let trampoline = format!("{type_name}__{trait_name}__{}", m.name);
            // Cast is needed when the trampoline's signature differs from the vtable's
            // (e.g. the vtable uses void* for Self but the trampoline uses the concrete type).
            let needs_cast =
                m.return_ty.contains_self() || m.params.iter().any(|p| p.ty.contains_self());
            if needs_cast {
                let vtable_ret = ty_str(&m.return_ty);
                let vtable_params: Vec<String> = std::iter::once("void*".to_string())
                    .chain(m.params.iter().map(|p| ty_str(&p.ty)))
                    .collect();
                let cast = format!("({vtable_ret} (*)({}))", vtable_params.join(", "));
                self.out
                    .push_str(&format!("    .{} = {cast}{trampoline},\n", m.name));
            } else {
                self.out
                    .push_str(&format!("    .{} = {trampoline},\n", m.name));
            }
        }
        self.out.push_str("};\n\n");
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt, ctx: &mut Ctx, indent: usize) {
        let p = pad(indent);
        match &stmt.kind {
            StmtKind::Println {
                format,
                args,
                newline,
                stderr,
            } => {
                let s = self.emit_println(format, args, *newline, *stderr, ctx);
                self.out.push_str(&format!("{p}{s}\n"));
            }

            StmtKind::Let {
                name,
                mutable,
                ty,
                expr,
            } => {
                // After type checking, ty is always Some (type checker fills it in).
                // Track named types for method resolution and printf format specifier selection.
                if let Some(Ty::Named(n)) = ty {
                    ctx.type_env.insert(name.clone(), n.clone());
                }
                if let Some(t) = ty {
                    ctx.var_types.insert(name.clone(), t.clone());
                }
                let val = self.emit_expr_hint(expr, ctx, ty.as_ref());
                if let Some(t) = ty {
                    let is_ptr = matches!(
                        t,
                        Ty::Ref(_)
                            | Ty::RefMut(_)
                            | Ty::RawConst(_)
                            | Ty::RawMut(_)
                            | Ty::Array(_, _)
                            | Ty::Str
                            | Ty::FnPtr { .. }
                    );
                    let kw = if *mutable || is_ptr { "" } else { "const " };
                    let decl = ty_str_decl(t, name);
                    self.out.push_str(&format!("{p}{kw}{decl} = {val};\n"));
                } else {
                    // Should not happen: type checker always fills in let type.
                    unreachable!("StmtKind::Let has no type after type checking");
                }
            }

            StmtKind::Assign { name, expr } => {
                let val = self.emit_expr(expr, ctx);
                self.out.push_str(&format!("{p}{name} = {val};\n"));
            }

            StmtKind::Return(expr) => match expr {
                Some(e) if matches!(&e.kind, ExprKind::Tuple(v) if v.is_empty()) => {
                    // unit () return -- emit bare `return;` when the function returns void
                    if ctx.return_ty == Some(Ty::Unit) {
                        self.out.push_str(&format!("{p}return;\n"));
                    } else {
                        self.out.push_str(&format!("{p}return 0;\n"));
                    }
                }
                Some(e)
                    if (matches!(&e.kind, ExprKind::Unsafe(_) | ExprKind::Block(_))
                        || matches!(&e.kind, ExprKind::If { .. }))
                        && ctx.return_ty == Some(Ty::Unit) =>
                {
                    // Block/unsafe/if as the tail of a unit function: inline as statements.
                    match &e.kind {
                        ExprKind::Unsafe(block) | ExprKind::Block(block) => {
                            for s in &block.stmts {
                                self.emit_stmt(s, ctx, indent);
                            }
                        }
                        ExprKind::If {
                            cond,
                            then_block,
                            else_block,
                        } => {
                            let cond_s = self.emit_expr(cond, ctx);
                            self.out.push_str(&format!("{p}if ({cond_s}) {{\n"));
                            for s in &then_block.stmts {
                                self.emit_stmt(s, ctx, indent + 1);
                            }
                            self.emit_else(else_block, ctx, indent);
                        }
                        _ => unreachable!(),
                    }
                }
                Some(e) => {
                    // For _Noreturn (!) functions the tail expression is a statement, not a return.
                    // Also, calling a never-returning function inside a return emits as a statement.
                    let call_is_never = matches!(&e.kind, ExprKind::Call { name, .. } if self.never_fns.contains(name.as_str()));
                    if ctx.return_ty == Some(Ty::Never) || call_is_never {
                        let s = self.emit_expr(e, ctx);
                        self.out.push_str(&format!("{p}{s};\n"));
                    } else {
                        let hint = ctx.return_ty.clone();
                        let s = self.emit_expr_hint(e, ctx, hint.as_ref());
                        self.out.push_str(&format!("{p}return {s};\n"));
                    }
                }
                None => {
                    // `return;` in main must emit `return 0;` since C main returns int.
                    if ctx.in_main {
                        self.out.push_str(&format!("{p}return 0;\n"));
                    } else {
                        self.out.push_str(&format!("{p}return;\n"));
                    }
                }
            },

            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let cond_s = self.emit_expr(cond, ctx);
                self.out.push_str(&format!("{p}if ({cond_s}) {{\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }

            StmtKind::While { cond, body } => {
                let cond_s = self.emit_expr(cond, ctx);
                self.out.push_str(&format!("{p}while ({cond_s}) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }

            StmtKind::Loop(body) => {
                ctx.loop_depth += 1;
                self.out.push_str(&format!("{p}for (;;) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                ctx.loop_depth -= 1;
                self.out.push_str(&format!("{p}}}\n"));
            }

            StmtKind::For {
                var,
                iter,
                body,
                elem_ty,
                iter_ty,
            } => {
                let p1 = "    ".repeat(indent + 1);
                // `for i in lo..hi` -- emit as a numeric C for-loop.
                if let ExprKind::Range { start, end } = &iter.kind {
                    let lo = start
                        .as_deref()
                        .map(|e| self.emit_expr(e, ctx))
                        .unwrap_or_else(|| "0".to_string());
                    let hi = end
                        .as_deref()
                        .map(|e| self.emit_expr(e, ctx))
                        .unwrap_or_else(|| "/* unbounded */0".to_string());
                    self.out.push_str(&format!(
                        "{p}for (int64_t {var} = {lo}; {var} < {hi}; {var}++) {{\n"
                    ));
                } else if let Some(Ty::Array(_, n)) = iter_ty {
                    // Compile-time known array length: use it directly.
                    let iter_s = self.emit_expr(iter, ctx);
                    self.out.push_str(&format!(
                        "{p}for (size_t _brust_i = 0; _brust_i < {n}; _brust_i++) {{\n"
                    ));
                    let decl = typed_or_auto_decl(elem_ty.as_ref(), var);
                    self.out
                        .push_str(&format!("{p1}{decl} = {iter_s}[_brust_i];\n"));
                } else {
                    // Fallback: sizeof trick (works for local stack-allocated C arrays).
                    let iter_s = self.emit_expr(iter, ctx);
                    self.out.push_str(&format!(
                        "{p}for (size_t _brust_i = 0; _brust_i < sizeof({iter_s})/sizeof({iter_s}[0]); _brust_i++) {{\n"
                    ));
                    let decl = typed_or_auto_decl(elem_ty.as_ref(), var);
                    self.out
                        .push_str(&format!("{p1}{decl} = {iter_s}[_brust_i];\n"));
                }
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }

            StmtKind::Break(val) => {
                if let Some(v) = val {
                    // `break val` inside the outermost result-capturing loop.
                    let lv = ctx.loop_result_var.clone();
                    if let Some(lv) = lv.filter(|_| ctx.loop_depth <= 1) {
                        let vs = self.emit_expr(v, ctx);
                        self.out.push_str(&format!("{p}{lv} = {vs};\n"));
                    }
                }
                self.out.push_str(&format!("{p}break;\n"));
            }

            StmtKind::Continue => {
                self.out.push_str(&format!("{p}continue;\n"));
            }

            StmtKind::CompoundAssign { op, lhs, rhs } => {
                let ls = self.emit_expr(lhs, ctx);
                let rs = self.emit_expr(rhs, ctx);
                let op_s = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Rem => "%=",
                    BinOp::BitAnd => "&=",
                    BinOp::BitOr => "|=",
                    BinOp::BitXor => "^=",
                    BinOp::Shl => "<<=",
                    BinOp::Shr => ">>=",
                    _ => "+=",
                };
                self.out.push_str(&format!("{p}{ls} {op_s} {rs};\n"));
            }

            StmtKind::IfLet {
                pat,
                expr,
                then_block,
                else_block,
                expr_ty,
                and_cond,
            } => {
                self.emit_if_let(
                    pat,
                    expr,
                    expr_ty.as_ref(),
                    and_cond.as_ref(),
                    then_block,
                    else_block,
                    ctx,
                    indent,
                );
            }

            StmtKind::WhileLet {
                pat,
                expr,
                expr_ty,
                body,
            } => {
                self.emit_while_let(pat, expr, expr_ty.as_ref(), body, ctx, indent);
            }

            StmtKind::Match {
                expr,
                arms,
                scrutinee_ty,
            } => self.emit_match(expr, arms, ctx, indent, scrutinee_ty.as_ref()),

            StmtKind::LetPat {
                pat,
                ty,
                expr,
                else_block,
            } => {
                if let Some(else_block) = else_block {
                    self.emit_let_else(pat, ty.as_ref(), expr, else_block, ctx, indent);
                } else {
                    self.emit_let_pat(pat, ty.as_ref(), expr, ctx, indent);
                }
            }

            StmtKind::Expr(expr) => {
                if let ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs,
                    rhs,
                } = &expr.kind
                {
                    // Field/tuple-index/pointer assignment encoded as BinOp::Eq in lvalue position.
                    let ls = self.emit_expr(lhs, ctx);
                    let rs = self.emit_expr(rhs, ctx);
                    self.out.push_str(&format!("{p}{ls} = {rs};\n"));
                } else if let ExprKind::Unsafe(block) | ExprKind::Block(block) = &expr.kind {
                    // Unsafe/block as a statement: inline the inner statements.
                    for s in &block.stmts {
                        if let StmtKind::Return(Some(inner)) = &s.kind {
                            let val = self.emit_expr(inner, ctx);
                            self.out.push_str(&format!("{p}{val};\n"));
                        } else {
                            self.emit_stmt(s, ctx, indent);
                        }
                    }
                } else if let ExprKind::If {
                    cond,
                    then_block,
                    else_block,
                } = &expr.kind
                {
                    // if expression used as a statement: emit as proper C if statement.
                    let cond_s = self.emit_expr(cond, ctx);
                    self.out.push_str(&format!("{p}if ({cond_s}) {{\n"));
                    for s in &then_block.stmts {
                        self.emit_stmt(s, ctx, indent + 1);
                    }
                    self.emit_else(else_block, ctx, indent);
                } else if let ExprKind::Match {
                    expr: match_expr,
                    arms,
                    ..
                } = &expr.kind
                {
                    // match expression used as a statement: emit as proper C switch.
                    self.emit_match(match_expr, arms, ctx, indent, None);
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
                if blk.stmts.len() == 1
                    && let StmtKind::If {
                        cond,
                        then_block,
                        else_block: inner,
                    } = &blk.stmts[0].kind
                {
                    let cond_s = self.emit_expr(cond, ctx);
                    self.out.push_str(&format!("{p}}} else if ({cond_s}) {{\n"));
                    for s in &then_block.stmts {
                        self.emit_stmt(s, ctx, indent + 1);
                    }
                    self.emit_else(inner, ctx, indent);
                    return;
                }
                self.out.push_str(&format!("{p}}} else {{\n"));
                for s in &blk.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
        }
    }

    fn emit_if_let(
        &mut self,
        pat: &Pat,
        expr: &Expr,
        expr_ty: Option<&Ty>,
        and_cond: Option<&Expr>,
        then_block: &Block,
        else_block: &Option<Block>,
        ctx: &mut Ctx,
        indent: usize,
    ) {
        let p = pad(indent);
        let ip = pad(indent + 1);
        let expr_s = self.emit_expr(expr, ctx);
        match pat {
            Pat::Wildcard => {
                // Always matches. Use `if (1)` when there is an else block so the
                // generated C is valid (bare `{` cannot be followed by `else`).
                if else_block.is_some() {
                    self.out.push_str(&format!("{p}if (1) {{\n"));
                } else {
                    self.out.push_str(&format!("{p}{{\n"));
                }
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            Pat::Bool(b) => {
                let cond = if *b { expr_s } else { format!("!({expr_s})") };
                self.out.push_str(&format!("{p}if ({cond}) {{\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            Pat::Int(n) => {
                self.out
                    .push_str(&format!("{p}if (({expr_s}) == {n}) {{\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            Pat::Range { lo, hi } => {
                self.out.push_str(&format!(
                    "{p}if (({expr_s}) >= {lo} && ({expr_s}) <= {hi}) {{\n"
                ));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } => {
                // Materialize the scrutinee once.
                let match_var = match &expr.kind {
                    ExprKind::Var(n) => n.clone(),
                    _ => {
                        self.out
                            .push_str(&format!("{p}const {type_name} _iflet_val = {expr_s};\n"));
                        "_iflet_val".to_string()
                    }
                };
                let pat_cond = format!("{match_var}.tag == {type_name}_{variant}_tag");
                // When there's an and_cond and an else branch, use a flag to route control.
                if and_cond.is_some() && else_block.is_some() {
                    let ok_var = format!("_iflet_ok{}", self.tmp_counter);
                    self.tmp_counter += 1;
                    self.out.push_str(&format!("{p}int {ok_var} = 0;\n"));
                    self.out.push_str(&format!("{p}if ({pat_cond}) {{\n"));
                    let mut arm_ctx = ctx.clone();
                    self.emit_if_let_enum_bindings(
                        type_name,
                        variant,
                        bindings,
                        &match_var,
                        &ip,
                        &mut arm_ctx,
                    );
                    let cond_s = self.emit_expr(and_cond.unwrap(), &mut arm_ctx);
                    self.out.push_str(&format!("{ip}if ({cond_s}) {{\n"));
                    let bp = pad(indent + 2);
                    self.out.push_str(&format!("{bp}{ok_var} = 1;\n"));
                    for s in &then_block.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 2);
                    }
                    self.out.push_str(&format!("{ip}}}\n"));
                    self.out.push_str(&format!("{p}}}\n"));
                    self.out.push_str(&format!("{p}if (!{ok_var}) {{\n"));
                    for s in &else_block.as_ref().unwrap().stmts {
                        self.emit_stmt(s, ctx, indent + 1);
                    }
                    self.out.push_str(&format!("{p}}}\n"));
                } else if let Some(cond) = and_cond {
                    // No else: just nest the condition.
                    self.out.push_str(&format!("{p}if ({pat_cond}) {{\n"));
                    let mut arm_ctx = ctx.clone();
                    self.emit_if_let_enum_bindings(
                        type_name,
                        variant,
                        bindings,
                        &match_var,
                        &ip,
                        &mut arm_ctx,
                    );
                    let cond_s = self.emit_expr(cond, &mut arm_ctx);
                    self.out.push_str(&format!("{ip}if ({cond_s}) {{\n"));
                    for s in &then_block.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 2);
                    }
                    self.out.push_str(&format!("{ip}}}\n"));
                    self.out.push_str(&format!("{p}}}\n"));
                } else {
                    self.out.push_str(&format!("{p}if ({pat_cond}) {{\n"));
                    let mut arm_ctx = ctx.clone();
                    self.emit_if_let_enum_bindings(
                        type_name,
                        variant,
                        bindings,
                        &match_var,
                        &ip,
                        &mut arm_ctx,
                    );
                    for s in &then_block.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 1);
                    }
                    self.emit_else(else_block, ctx, indent);
                }
            }
            Pat::Or(alternatives) => {
                // Materialize the scrutinee into a temp if it's not already a variable,
                // to avoid evaluating it multiple times in the condition.
                let cond_expr = match &expr.kind {
                    ExprKind::Var(_) => expr_s.clone(),
                    _ => {
                        self.out
                            .push_str(&format!("{p}__auto_type _iflet_or_val = {expr_s};\n"));
                        "_iflet_or_val".to_string()
                    }
                };
                let conditions: Vec<String> = alternatives
                    .iter()
                    .map(|alt| match alt {
                        Pat::Wildcard | Pat::Binding(_) => "1".to_string(),
                        Pat::Bool(b) => {
                            if *b {
                                cond_expr.clone()
                            } else {
                                format!("!({cond_expr})")
                            }
                        }
                        Pat::Int(n) => format!("({cond_expr}) == {n}"),
                        Pat::Range { lo, hi } => {
                            format!("({cond_expr}) >= {lo} && ({cond_expr}) <= {hi}")
                        }
                        Pat::EnumVariant {
                            type_name, variant, ..
                        } => {
                            format!("({cond_expr}).tag == {type_name}_{variant}_tag")
                        }
                        _ => "0".to_string(),
                    })
                    .collect();
                let full_cond = conditions.join(" || ");
                self.out.push_str(&format!("{p}if ({full_cond}) {{\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            Pat::Binding(name) => {
                // Binding pattern always matches; bind value in block.
                let decl = typed_or_auto_decl(expr_ty, name);
                // Use `if (1)` so a following `else` is valid C.
                if else_block.is_some() {
                    self.out.push_str(&format!("{p}if (1) {{\n"));
                } else {
                    self.out.push_str(&format!("{p}{{\n"));
                }
                self.out.push_str(&format!("{ip}{decl} = {expr_s};\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
            _ => {
                // Fallback for other patterns (Range in if-let, etc.)
                self.out.push_str(&format!("{p}{{\n"));
                for s in &then_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.emit_else(else_block, ctx, indent);
            }
        }
    }

    /// Emit bindings for an enum variant pattern inside an if-let block.
    fn emit_if_let_enum_bindings(
        &mut self,
        type_name: &str,
        variant: &str,
        bindings: &PatBindings,
        match_var: &str,
        ip: &str,
        arm_ctx: &mut Ctx,
    ) {
        if let Some(edecl) = self.enums.get(type_name).cloned() {
            if let Some(ev) = edecl.variants.iter().find(|v| v.name == variant) {
                match bindings {
                    PatBindings::Tuple(pats) => {
                        if let VariantFields::Tuple(tys) = &ev.fields {
                            for (i, sub_pat) in pats.iter().enumerate() {
                                let binding = match sub_pat {
                                    Pat::Binding(n) if n != "_" => n.clone(),
                                    _ => continue,
                                };
                                let fty = tys
                                    .get(i)
                                    .map(ty_str)
                                    .unwrap_or_else(|| "int64_t".to_string());
                                self.out.push_str(&format!(
                                    "{ip}{fty} {binding} = {match_var}.data.{variant}._{i};\n"
                                ));
                                if let Some(Ty::Named(n)) = tys.get(i) {
                                    arm_ctx.type_env.insert(binding, n.clone());
                                }
                            }
                        }
                    }
                    PatBindings::Named(fields_bind, _) => {
                        if let VariantFields::Named(decl_fields) = &ev.fields {
                            for (field_name, binding) in fields_bind {
                                if let Some(df) = decl_fields.iter().find(|f| f.name == *field_name)
                                {
                                    let fty = ty_str(&df.ty);
                                    self.out.push_str(&format!(
                                        "{ip}{fty} {binding} = {match_var}.data.{variant}.{field_name};\n"
                                    ));
                                    if let Ty::Named(n) = &df.ty {
                                        arm_ctx.type_env.insert(binding.clone(), n.clone());
                                    }
                                }
                            }
                        }
                    }
                    PatBindings::None => {}
                }
            }
        }
    }

    fn emit_let_pat(
        &mut self,
        pat: &Pat,
        ty: Option<&Ty>,
        expr: &Expr,
        ctx: &mut Ctx,
        indent: usize,
    ) {
        let p = pad(indent);
        let expr_s = self.emit_expr(expr, ctx);
        let tmp_id = self.tmp_counter;
        self.tmp_counter += 1;
        let tmp = format!("_let_tmp{tmp_id}");
        match pat {
            Pat::Tuple(pats) => {
                // `let (a, b) = tup;`
                self.out
                    .push_str(&format!("{p}__auto_type {tmp} = {expr_s};\n"));
                let tys: Vec<Ty> = match ty {
                    Some(Ty::Tuple(t)) => t.clone(),
                    _ => pats.iter().map(|_| Ty::I64).collect(),
                };
                for (i, sub_pat) in pats.iter().enumerate() {
                    if let Pat::Binding(name) = sub_pat {
                        if name == "_" {
                            continue;
                        }
                        let field_ty = tys.get(i).cloned().unwrap_or(Ty::I64);
                        let decl = ty_str_decl(&field_ty, name);
                        self.out.push_str(&format!("{p}{decl} = {tmp}._{i};\n"));
                    }
                }
            }
            Pat::TupleStruct { type_name, fields } => {
                // `let Point(x, y) = p;`
                let ts = ty_str(&Ty::Named(type_name.clone()));
                self.out
                    .push_str(&format!("{p}const {ts} {tmp} = {expr_s};\n"));
                for (i, sub_pat) in fields.iter().enumerate() {
                    if let Pat::Binding(name) = sub_pat {
                        if name == "_" {
                            continue;
                        }
                        self.out
                            .push_str(&format!("{p}__auto_type {name} = {tmp}._{i};\n"));
                    }
                }
            }
            Pat::Binding(name) => {
                let decl = typed_or_auto_decl(ty, name);
                self.out.push_str(&format!("{p}{decl} = {expr_s};\n"));
            }
            _ => {
                // Generic fallback: store in temp
                self.out
                    .push_str(&format!("{p}__auto_type {tmp} = {expr_s};\n"));
            }
        }
    }

    fn emit_let_else(
        &mut self,
        pat: &Pat,
        _ty: Option<&Ty>,
        expr: &Expr,
        else_block: &Block,
        ctx: &mut Ctx,
        indent: usize,
    ) {
        let p = pad(indent);
        let ip = pad(indent + 1);
        let expr_s = self.emit_expr(expr, ctx);
        match pat {
            Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } => {
                let match_var = match &expr.kind {
                    ExprKind::Var(n) => n.clone(),
                    _ => {
                        self.out
                            .push_str(&format!("{p}const {type_name} _le_tmp = {expr_s};\n"));
                        "_le_tmp".to_string()
                    }
                };
                // If pattern doesn't match, run else block.
                self.out.push_str(&format!(
                    "{p}if (!({match_var}.tag == {type_name}_{variant}_tag)) {{\n"
                ));
                for s in &else_block.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
                // Bind variables after the guard.
                let mut arm_ctx = ctx.clone();
                self.emit_if_let_enum_bindings(
                    type_name,
                    variant,
                    bindings,
                    &match_var,
                    &ip,
                    &mut arm_ctx,
                );
            }
            Pat::TupleStruct { type_name, fields } => {
                let ts = ty_str(&Ty::Named(type_name.clone()));
                self.out
                    .push_str(&format!("{p}const {ts} _le_tmp = {expr_s};\n"));
                // Tuple structs always match -- just bind fields.
                for (i, sub_pat) in fields.iter().enumerate() {
                    if let Pat::Binding(name) = sub_pat {
                        if name == "_" {
                            continue;
                        }
                        self.out
                            .push_str(&format!("{p}__auto_type {name} = _le_tmp._{i};\n"));
                    }
                }
            }
            Pat::Binding(name) => {
                // `let x = expr else { ... };` -- always matches, bind.
                self.out
                    .push_str(&format!("{p}__auto_type {name} = {expr_s};\n"));
            }
            _ => {
                // Generic: store temp, run else if condition fails.
                self.out
                    .push_str(&format!("{p}__auto_type _le_tmp = {expr_s};\n"));
            }
        }
    }

    fn emit_while_let(
        &mut self,
        pat: &Pat,
        expr: &Expr,
        expr_ty: Option<&Ty>,
        body: &Block,
        ctx: &mut Ctx,
        indent: usize,
    ) {
        let p = pad(indent);
        let ip = pad(indent + 1);
        match pat {
            Pat::Wildcard => {
                self.out.push_str(&format!("{p}for (;;) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Bool(b) => {
                let expr_s = self.emit_expr(expr, ctx);
                let cond = if *b { expr_s } else { format!("!({expr_s})") };
                self.out.push_str(&format!("{p}while ({cond}) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Int(n) => {
                let expr_s = self.emit_expr(expr, ctx);
                self.out
                    .push_str(&format!("{p}while (({expr_s}) == {n}) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Binding(name) => {
                // Binding always matches: declare before loop, update inside condition.
                let decl = typed_or_auto_decl(expr_ty, name);
                let expr_s = self.emit_expr(expr, ctx);
                self.out.push_str(&format!("{p}{decl} = {expr_s};\n"));
                self.out.push_str(&format!("{p}for (;;) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                // Update binding at end of loop body before re-checking.
                let expr_s2 = self.emit_expr(expr, ctx);
                self.out.push_str(&format!("{ip}{name} = {expr_s2};\n"));
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } => {
                // For Var scrutinees: check tag directly each iteration, bind from var.
                // For other exprs: use a compound-assignment temp to avoid re-evaluating.
                let (cond, use_temp) = match &expr.kind {
                    ExprKind::Var(n) => (format!("{n}.tag == {type_name}_{variant}_tag"), false),
                    _ => {
                        let expr_s = self.emit_expr(expr, ctx);
                        // Declare temp before loop; update it in the while condition.
                        let ty_s = ty_str(&Ty::Named(type_name.clone()));
                        self.out.push_str(&format!("{p}{ty_s} _wlet_tmp;\n"));
                        let cond =
                            format!("(_wlet_tmp = {expr_s}).tag == {type_name}_{variant}_tag");
                        (cond, true)
                    }
                };
                self.out.push_str(&format!("{p}while ({cond}) {{\n"));
                let mut arm_ctx = ctx.clone();
                let match_src = if use_temp {
                    "_wlet_tmp"
                } else if let ExprKind::Var(n) = &expr.kind {
                    n.as_str()
                } else {
                    "_wlet_tmp"
                };
                if let Some(edecl) = self.enums.get(type_name).cloned() {
                    if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                        match bindings {
                            PatBindings::Tuple(pats) => {
                                if let VariantFields::Tuple(tys) = &ev.fields {
                                    for (i, sub_pat) in pats.iter().enumerate() {
                                        let binding = match sub_pat {
                                            Pat::Binding(n) if n != "_" => n.clone(),
                                            _ => continue,
                                        };
                                        let fty = tys
                                            .get(i)
                                            .map(ty_str)
                                            .unwrap_or_else(|| "int64_t".to_string());
                                        self.out.push_str(&format!(
                                            "{ip}{fty} {binding} = {match_src}.data.{variant}._{i};\n"
                                        ));
                                        if let Some(Ty::Named(n)) = tys.get(i) {
                                            arm_ctx.type_env.insert(binding, n.clone());
                                        }
                                    }
                                }
                            }
                            PatBindings::Named(fields_bind, _) => {
                                if let VariantFields::Named(decl_fields) = &ev.fields {
                                    for (field_name, binding) in fields_bind {
                                        if let Some(df) =
                                            decl_fields.iter().find(|f| f.name == *field_name)
                                        {
                                            let fty = ty_str(&df.ty);
                                            self.out.push_str(&format!(
                                                "{ip}{fty} {binding} = {match_src}.data.{variant}.{field_name};\n"
                                            ));
                                            if let Ty::Named(n) = &df.ty {
                                                arm_ctx.type_env.insert(binding.clone(), n.clone());
                                            }
                                        }
                                    }
                                }
                            }
                            PatBindings::None => {}
                        }
                    }
                }
                for s in &body.stmts {
                    self.emit_stmt(s, &mut arm_ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Or(alternatives) => {
                let expr_s = self.emit_expr(expr, ctx);
                let conditions: Vec<String> = alternatives
                    .iter()
                    .map(|alt| match alt {
                        Pat::Wildcard | Pat::Binding(_) => "1".to_string(),
                        Pat::Bool(b) => {
                            if *b {
                                expr_s.clone()
                            } else {
                                format!("!({expr_s})")
                            }
                        }
                        Pat::Int(n) => format!("({expr_s}) == {n}"),
                        Pat::EnumVariant {
                            type_name, variant, ..
                        } => format!("({expr_s}).tag == {type_name}_{variant}_tag"),
                        _ => "0".to_string(),
                    })
                    .collect();
                let full_cond = conditions.join(" || ");
                self.out.push_str(&format!("{p}while ({full_cond}) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Range { lo, hi } => {
                let expr_s = self.emit_expr(expr, ctx);
                self.out.push_str(&format!(
                    "{p}while (({expr_s}) >= {lo} && ({expr_s}) <= {hi}) {{\n"
                ));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
            Pat::Tuple(_) | Pat::TupleStruct { .. } => {
                // Tuple/TupleStruct patterns in while-let are unusual; treat as infinite loop.
                self.out.push_str(&format!("{p}for (;;) {{\n"));
                for s in &body.stmts {
                    self.emit_stmt(s, ctx, indent + 1);
                }
                self.out.push_str(&format!("{p}}}\n"));
            }
        }
    }

    fn emit_match_arm_bindings(
        &mut self,
        pat: &Pat,
        match_var: &str,
        is_tagged: bool,
        enum_decl: &Option<EnumDecl>,
        bp: &str,
        arm_ctx: &mut Ctx,
        scrutinee_ty: Option<&Ty>,
    ) {
        // Only EnumVariant patterns introduce bindings.
        let (_type_name, variant, bindings) = match pat {
            Pat::Binding(name) => {
                // Bind the whole matched value to `name`.
                let decl = typed_or_auto_decl(scrutinee_ty, name);
                self.out.push_str(&format!("{bp}{decl} = {match_var};\n"));
                return;
            }
            Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } => (type_name, variant, bindings),
            _ => return,
        };
        if !is_tagged {
            // Check if this is a struct pattern (type_name == variant, and it's a known struct).
            if let Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } = pat
                && type_name == variant
                && let Some(sdecl) = self.structs.get(type_name.as_str()).cloned()
            {
                if let PatBindings::Named(binds, _) = bindings {
                    for (field_name, binding_name) in binds {
                        if binding_name == "_" {
                            continue;
                        }
                        let fty = sdecl
                            .fields
                            .iter()
                            .find(|f| f.name == *field_name)
                            .map(|f| ty_str(&f.ty))
                            .unwrap_or_else(|| "int64_t".to_string());
                        self.out.push_str(&format!(
                            "{bp}{fty} {binding_name} = {match_var}.{field_name};\n"
                        ));
                        if let Some(f) = sdecl.fields.iter().find(|f| f.name == *field_name)
                            && let Ty::Named(n) = &f.ty
                        {
                            arm_ctx.type_env.insert(binding_name.clone(), n.clone());
                        }
                    }
                }
            }
            return;
        }
        if let Some(edecl) = enum_decl
            && let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant)
        {
            match bindings {
                PatBindings::None => {}
                PatBindings::Tuple(pats) => {
                    if let VariantFields::Tuple(tys) = &ev.fields {
                        for (i, sub_pat) in pats.iter().enumerate() {
                            let binding = match sub_pat {
                                Pat::Binding(n) if n != "_" => n.clone(),
                                _ => continue,
                            };
                            let fty = tys
                                .get(i)
                                .map(ty_str)
                                .unwrap_or_else(|| "int64_t".to_string());
                            self.out.push_str(&format!(
                                "{bp}{fty} {binding} = {match_var}.data.{variant}._{i};\n"
                            ));
                            if let Some(Ty::Named(n)) = tys.get(i) {
                                arm_ctx.type_env.insert(binding, n.clone());
                            }
                        }
                    }
                }
                PatBindings::Named(binds, _) => {
                    if let VariantFields::Named(fields) = &ev.fields {
                        for (field_name, binding_name) in binds {
                            if binding_name == "_" {
                                continue;
                            }
                            let fty = fields
                                .iter()
                                .find(|f| f.name == *field_name)
                                .map(|f| ty_str(&f.ty))
                                .unwrap_or_else(|| "int64_t".to_string());
                            self.out.push_str(&format!(
                                "{bp}{fty} {binding_name} = {match_var}.data.{variant}.{field_name};\n"
                            ));
                            if let Some(f) = fields.iter().find(|f| f.name == *field_name)
                                && let Ty::Named(n) = &f.ty
                            {
                                arm_ctx.type_env.insert(binding_name.clone(), n.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Emit a `match` as an if-else chain.
    /// Used when binding patterns are present (can't use C switch).
    fn emit_match_if_chain(
        &mut self,
        expr: &Expr,
        arms: &[MatchArm],
        ctx: &mut Ctx,
        indent: usize,
        scrutinee_ty: Option<&Ty>,
    ) {
        let p = pad(indent);
        let ip = pad(indent + 1);
        // Materialize scrutinee once with a unique name to avoid redefinition in nested match.
        let ms_id = self.tmp_counter;
        self.tmp_counter += 1;
        let ms = format!("_ms{ms_id}");
        let expr_s = self.emit_expr(expr, ctx);
        let ms_decl = typed_or_auto_decl(scrutinee_ty, &ms);
        self.out.push_str(&format!("{p}{ms_decl} = {expr_s};\n"));

        for (i, arm) in arms.iter().enumerate() {
            let mut arm_ctx = ctx.clone();
            let keyword = if i == 0 { "if" } else { "else if" };

            match (&arm.pat, &arm.guard) {
                // Binding pattern with guard: use GNU stmt expr for condition
                (Pat::Binding(name), Some(guard)) => {
                    // Condition: ({ TYPE name = ms; guard_expr; })
                    arm_ctx.type_env.insert(name.clone(), "i64".to_string());
                    let gs = self.emit_expr(guard, &mut arm_ctx);
                    let cond_decl = typed_or_auto_decl(scrutinee_ty, name);
                    let cond = format!("({{ {cond_decl} = {ms}; {gs}; }})");
                    self.out.push_str(&format!("{p}{keyword} ({cond}) {{\n"));
                    let body_decl = typed_or_auto_decl(scrutinee_ty, name);
                    self.out.push_str(&format!("{ip}{body_decl} = {ms};\n"));
                    for s in &arm.body.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 1);
                    }
                    self.out.push_str(&format!("{p}}}\n"));
                }
                // Wildcard/binding without guard: always matches
                (Pat::Wildcard, None) | (Pat::Binding(_), None) => {
                    let decl = if let Pat::Binding(name) = &arm.pat {
                        Some(format!(
                            "{ip}{} = {ms};\n",
                            typed_or_auto_decl(scrutinee_ty, name)
                        ))
                    } else {
                        None
                    };
                    if i == 0 {
                        self.out.push_str(&format!("{p}{{\n"));
                    } else {
                        self.out.push_str(&format!("{p}else {{\n"));
                    }
                    if let Some(d) = &decl {
                        self.out.push_str(d);
                    }
                    for s in &arm.body.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 1);
                    }
                    self.out.push_str(&format!("{p}}}\n"));
                }
                // Other patterns (Int, Bool, EnumVariant, Or, Range) with optional guard
                (pat, guard) => {
                    // Special case: TupleStruct pattern -- bind fields, then check guard
                    if let Pat::TupleStruct { type_name, fields } = pat {
                        let sdecl = self.structs.get(type_name.as_str()).cloned();
                        self.out.push_str(&format!("{p}{keyword} (1) {{\n"));
                        for (fi, sub_pat) in fields.iter().enumerate() {
                            if let Pat::Binding(name) = sub_pat {
                                if name == "_" {
                                    continue;
                                }
                                let fty = sdecl
                                    .as_ref()
                                    .and_then(|s| s.fields.get(fi))
                                    .map(|f| ty_str(&f.ty))
                                    .unwrap_or_else(|| "int64_t".to_string());
                                self.out
                                    .push_str(&format!("{ip}{fty} {name} = {ms}._{fi};\n"));
                            }
                        }
                        if let Some(g) = guard {
                            let gs = self.emit_expr(g, &mut arm_ctx);
                            self.out.push_str(&format!("{ip}if ({gs}) {{\n"));
                            for s in &arm.body.stmts {
                                self.emit_stmt(s, &mut arm_ctx, indent + 2);
                            }
                            self.out.push_str(&format!("{ip}}}\n"));
                        } else {
                            for s in &arm.body.stmts {
                                self.emit_stmt(s, &mut arm_ctx, indent + 1);
                            }
                        }
                        self.out.push_str(&format!("{p}}}\n"));
                        continue;
                    }

                    let pat_cond = match pat {
                        Pat::Bool(b) => format!("{ms} == {}", if *b { 1 } else { 0 }),
                        Pat::Int(n) => format!("{ms} == {n}"),
                        Pat::Range { lo, hi } => format!("{ms} >= {lo} && {ms} <= {hi}"),
                        Pat::EnumVariant {
                            type_name, variant, ..
                        } => {
                            // Struct pattern: type_name == variant means it's a plain struct (always matches).
                            if type_name == variant && self.structs.contains_key(type_name.as_str())
                            {
                                "1".to_string()
                            } else {
                                format!("{ms}.tag == {type_name}_{variant}_tag")
                            }
                        }
                        Pat::Or(alts) => alts
                            .iter()
                            .map(|p| match p {
                                Pat::Wildcard | Pat::Binding(_) => "1".to_string(),
                                Pat::Bool(b) => format!("{ms} == {}", if *b { 1 } else { 0 }),
                                Pat::Int(n) => format!("{ms} == {n}"),
                                Pat::Range { lo, hi } => format!("{ms} >= {lo} && {ms} <= {hi}"),
                                Pat::EnumVariant {
                                    type_name, variant, ..
                                } => {
                                    format!("{ms}.tag == {type_name}_{variant}_tag")
                                }
                                Pat::Or(_) => "0".to_string(),
                                Pat::Tuple(_) | Pat::TupleStruct { .. } => "0".to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(" || "),
                        _ => "1".to_string(),
                    };
                    let full_cond = match guard {
                        None => pat_cond,
                        Some(g) => {
                            let gs = self.emit_expr(g, &mut arm_ctx);
                            format!("({pat_cond}) && ({gs})")
                        }
                    };
                    self.out
                        .push_str(&format!("{p}{keyword} ({full_cond}) {{\n"));
                    // Resolve enum decl for binding extraction.
                    let (is_tagged, enum_decl) = if let Pat::EnumVariant { type_name, .. } = pat {
                        let edecl = self.enums.get(type_name.as_str()).cloned();
                        let is_t = edecl.as_ref().is_some_and(|e| {
                            e.variants
                                .iter()
                                .any(|v| !matches!(v.fields, VariantFields::Unit))
                        });
                        (is_t, edecl)
                    } else {
                        (false, None)
                    };
                    self.emit_match_arm_bindings(
                        pat,
                        &ms,
                        is_tagged,
                        &enum_decl,
                        &ip,
                        &mut arm_ctx,
                        scrutinee_ty,
                    );
                    for s in &arm.body.stmts {
                        self.emit_stmt(s, &mut arm_ctx, indent + 1);
                    }
                    self.out.push_str(&format!("{p}}}\n"));
                }
            }
        }
    }

    fn emit_match(
        &mut self,
        expr: &Expr,
        arms: &[MatchArm],
        ctx: &mut Ctx,
        indent: usize,
        scrutinee_ty: Option<&Ty>,
    ) {
        // When any arm uses a binding or range pattern, C switch can't express it -- use if-else chain.
        let needs_if_chain = arms.iter().any(|a| matches!(&a.pat, Pat::Binding(_) | Pat::Range { .. } | Pat::TupleStruct { .. } | Pat::Tuple(_)))
            || arms.iter().any(|a| matches!(&a.pat, Pat::EnumVariant { type_name, variant, .. } if type_name == variant && self.structs.contains_key(type_name.as_str())));
        if needs_if_chain {
            self.emit_match_if_chain(expr, arms, ctx, indent, scrutinee_ty);
            return;
        }

        let p = pad(indent);
        let ip = pad(indent + 1);
        let bp = pad(indent + 2);

        // Determine enum type from patterns (if any arm is an enum variant)
        let enum_type_name: Option<String> = arms.iter().find_map(|a| {
            if let Pat::EnumVariant { type_name, .. } = &a.pat {
                Some(type_name.clone())
            } else {
                None
            }
        });
        let enum_decl: Option<EnumDecl> = enum_type_name
            .as_ref()
            .and_then(|tn| self.enums.get(tn).cloned());
        let is_tagged = enum_decl.as_ref().is_some_and(|e| {
            e.variants
                .iter()
                .any(|v| !matches!(v.fields, VariantFields::Unit))
        });

        // Materialize scrutinee into a temp var — always for tagged unions,
        // prevents double evaluation and enables field access.
        let match_var = if is_tagged {
            match &expr.kind {
                ExprKind::Var(n) => n.clone(),
                _ => {
                    let type_name = enum_type_name.as_deref().unwrap_or("int64_t");
                    let expr_s = self.emit_expr(expr, ctx);
                    self.out
                        .push_str(&format!("{p}const {type_name} _match_val = {expr_s};\n"));
                    "_match_val".to_string()
                }
            }
        } else {
            // For simple enums / int / bool, just use the expression inline
            self.emit_expr(expr, ctx)
        };

        let switch_cond = if is_tagged {
            format!("{match_var}.tag")
        } else {
            match_var.clone()
        };
        self.out
            .push_str(&format!("{p}switch ({switch_cond}) {{\n"));

        for arm in arms {
            // Clone type_env so bindings don't leak across arms
            let mut arm_ctx = ctx.clone();

            // Emit case label(s) - Pat::Or emits multiple labels.
            let pats: &[Pat] = match &arm.pat {
                Pat::Or(alts) => alts.as_slice(),
                single => std::slice::from_ref(single),
            };
            // When there's a guard, we can't use switch `case` directly.
            // Fall back to using `default:` + inner `if (cond)` with a goto skip.
            if arm.guard.is_some() {
                // Generate a unique label to skip past this arm if guard fails.
                let skip_label = format!("_guard_skip_{}", arm.loc.line);
                // Emit all matching case labels for this guarded arm.
                for pat in pats {
                    match pat {
                        Pat::Wildcard | Pat::Binding(_) => {
                            self.out.push_str(&format!("{ip}default: {{\n"));
                        }
                        Pat::Bool(b) => {
                            self.out
                                .push_str(&format!("{ip}case {}: {{\n", if *b { 1 } else { 0 }));
                        }
                        Pat::Int(n) => {
                            self.out.push_str(&format!("{ip}case {n}: {{\n"));
                        }
                        Pat::EnumVariant {
                            type_name, variant, ..
                        } => {
                            if is_tagged {
                                self.out
                                    .push_str(&format!("{ip}case {type_name}_{variant}_tag: {{\n"));
                            } else {
                                self.out
                                    .push_str(&format!("{ip}case {type_name}_{variant}: {{\n"));
                            }
                        }
                        Pat::Or(_) => {}
                        Pat::Range { .. } | Pat::Tuple(_) | Pat::TupleStruct { .. } => {
                            // Handled by if-chain path; unreachable here.
                        }
                    }
                }
                let guard_s = self.emit_expr(arm.guard.as_ref().unwrap(), &mut arm_ctx);
                self.out
                    .push_str(&format!("{bp}if (!({guard_s})) goto {skip_label};\n"));
                // Emit bindings and body
                self.emit_match_arm_bindings(
                    &arm.pat,
                    &match_var,
                    is_tagged,
                    &enum_decl,
                    &bp,
                    &mut arm_ctx,
                    None,
                );
                for s in &arm.body.stmts {
                    self.emit_stmt(s, &mut arm_ctx, indent + 2);
                }
                self.out.push_str(&format!("{bp}break;\n{ip}}}\n"));
                self.out.push_str(&format!("{ip}{skip_label}:;\n"));
                continue;
            }

            // No guard: emit normal case labels.
            for (pi, pat) in pats.iter().enumerate() {
                let is_last = pi == pats.len() - 1;
                match pat {
                    Pat::Wildcard | Pat::Binding(_) => {
                        self.out.push_str(&format!("{ip}default: {{\n"));
                    }
                    Pat::Bool(b) => {
                        self.out.push_str(&format!(
                            "{ip}case {}: {}{{\n",
                            if *b { 1 } else { 0 },
                            if is_last { "" } else { "" }
                        ));
                        if !is_last {
                            self.out.push_str(&format!("{ip}}}\n"));
                            continue;
                        }
                    }
                    Pat::Int(n) => {
                        if is_last {
                            self.out.push_str(&format!("{ip}case {n}: {{\n"));
                        } else {
                            self.out.push_str(&format!("{ip}case {n}:\n"));
                            continue;
                        }
                    }
                    Pat::EnumVariant {
                        type_name, variant, ..
                    } => {
                        if is_tagged {
                            if is_last {
                                self.out
                                    .push_str(&format!("{ip}case {type_name}_{variant}_tag: {{\n"));
                            } else {
                                self.out
                                    .push_str(&format!("{ip}case {type_name}_{variant}_tag:\n"));
                                continue;
                            }
                        } else if is_last {
                            self.out
                                .push_str(&format!("{ip}case {type_name}_{variant}: {{\n"));
                        } else {
                            self.out
                                .push_str(&format!("{ip}case {type_name}_{variant}:\n"));
                            continue;
                        }
                    }
                    Pat::Or(_) => {
                        // Nested or-pattern — flatten
                        self.out.push_str(&format!("{ip}default: {{\n"));
                    }
                    Pat::Range { .. } | Pat::Tuple(_) | Pat::TupleStruct { .. } => {
                        // Handled by if-chain path; unreachable here.
                        self.out.push_str(&format!("{ip}default: {{\n"));
                    }
                }
            }
            // Emit bindings for the primary (last) pattern.
            self.emit_match_arm_bindings(
                &arm.pat,
                &match_var,
                is_tagged,
                &enum_decl,
                &bp,
                &mut arm_ctx,
                None,
            );
            for s in &arm.body.stmts {
                self.emit_stmt(s, &mut arm_ctx, indent + 2);
            }
            self.out.push_str(&format!("{bp}break;\n{ip}}}\n"));
        }

        self.out.push_str(&format!("{p}}}\n"));
    }

    // -----------------------------------------------------------------------
    // Block / if / match as expressions
    // -----------------------------------------------------------------------

    /// Emit a block as a C expression.
    /// Single-expr blocks emit the expression directly.
    /// Multi-stmt blocks use a GNU statement expression `({ stmts; tail; })`.
    fn emit_block_as_expr(&mut self, block: &Block, ctx: &mut Ctx) -> String {
        if block.stmts.is_empty() {
            return "(void)0".to_string();
        }
        let (last, rest) = block.stmts.split_last().unwrap();
        let tail = self.emit_stmt_as_expr(last, ctx);
        if rest.is_empty() {
            return tail;
        }
        // Multi-stmt: save/restore self.out to capture the emitted statements.
        let saved = std::mem::take(&mut self.out);
        for s in rest {
            self.emit_stmt(s, ctx, 0);
        }
        let inner = std::mem::take(&mut self.out);
        self.out = saved;
        let inner = inner.trim_end().to_string();
        format!("({{ {inner} {tail}; }})")
    }

    /// Extract the value expression from a statement.
    /// Return(Some(e)) or Expr(e) -> emit e.
    /// If/Match statements -> emit as ternary.
    fn emit_stmt_as_expr(&mut self, stmt: &Stmt, ctx: &mut Ctx) -> String {
        match &stmt.kind {
            StmtKind::Return(Some(e)) | StmtKind::Expr(e) => self.emit_expr(e, ctx),
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let cond_s = self.emit_expr(cond, ctx);
                let then_s = self.emit_block_as_expr(then_block, ctx);
                let else_s = else_block
                    .as_ref()
                    .map(|b| self.emit_block_as_expr(b, ctx))
                    .unwrap_or_else(|| "(void)0".to_string());
                format!("(({cond_s}) ? ({then_s}) : ({else_s}))")
            }
            StmtKind::Match { expr, arms, .. } => self.emit_match_as_expr(expr, arms, ctx),
            _ => "(void)0".to_string(),
        }
    }

    /// Emit a match expression as a nested ternary chain.
    /// Tagged enum variants with bindings use GNU statement expressions for binding decls.
    fn emit_match_as_expr(&mut self, expr: &Expr, arms: &[MatchArm], ctx: &mut Ctx) -> String {
        let enum_type_name: Option<String> = arms.iter().find_map(|a| {
            if let Pat::EnumVariant { type_name, .. } = &a.pat {
                Some(type_name.clone())
            } else {
                None
            }
        });
        let enum_decl: Option<EnumDecl> = enum_type_name
            .as_ref()
            .and_then(|tn| self.enums.get(tn).cloned());
        let is_tagged = enum_decl.as_ref().is_some_and(|e| {
            e.variants
                .iter()
                .any(|v| !matches!(v.fields, VariantFields::Unit))
        });

        // Materialize scrutinee into temp var for tagged enums or complex expressions.
        let (match_var, prefix) = if is_tagged || !matches!(&expr.kind, ExprKind::Var(_)) {
            let expr_s = self.emit_expr(expr, ctx);
            let type_str = enum_type_name.as_deref().unwrap_or("int64_t");
            let ms_id = self.tmp_counter;
            self.tmp_counter += 1;
            let ms = format!("_ms{ms_id}");
            (ms.clone(), format!("const {type_str} {ms} = {expr_s}; "))
        } else {
            (self.emit_expr(expr, ctx), String::new())
        };

        // Build ternary chain from last arm to first.
        // Start with a typed zero as unreachable fallback to avoid void-type mismatch.
        let mut chain = "0".to_string();
        for arm in arms.iter().rev() {
            let arm_body = self.emit_match_arm_as_expr(arm, &match_var, is_tagged, &enum_decl, ctx);
            chain = match &arm.pat {
                Pat::Wildcard | Pat::Binding(_) => arm_body,
                Pat::Bool(b) => format!(
                    "(({match_var}) == {} ? ({arm_body}) : ({chain}))",
                    if *b { 1 } else { 0 }
                ),
                Pat::Int(n) => format!("(({match_var}) == {n} ? ({arm_body}) : ({chain}))"),
                Pat::EnumVariant {
                    type_name, variant, ..
                } => {
                    let cond = if is_tagged {
                        format!("({match_var}).tag == {type_name}_{variant}_tag")
                    } else {
                        format!("({match_var}) == {type_name}_{variant}")
                    };
                    format!("(({cond}) ? ({arm_body}) : ({chain}))")
                }
                Pat::Or(alts) => {
                    // Build compound condition: any alternative matches.
                    let cond = alts
                        .iter()
                        .map(|p| match p {
                            Pat::Wildcard | Pat::Binding(_) => "1".to_string(),
                            Pat::Bool(b) => format!("({match_var}) == {}", if *b { 1 } else { 0 }),
                            Pat::Int(n) => format!("({match_var}) == {n}"),
                            Pat::EnumVariant {
                                type_name, variant, ..
                            } => {
                                if is_tagged {
                                    format!("({match_var}).tag == {type_name}_{variant}_tag")
                                } else {
                                    format!("({match_var}) == {type_name}_{variant}")
                                }
                            }
                            Pat::Or(_) => "0".to_string(),
                            Pat::Range { lo, hi } => {
                                format!("({match_var}) >= {lo} && ({match_var}) <= {hi}")
                            }
                            Pat::Tuple(_) | Pat::TupleStruct { .. } => "0".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" || ");
                    format!("(({cond}) ? ({arm_body}) : ({chain}))")
                }
                Pat::Range { lo, hi } => {
                    format!(
                        "(({match_var}) >= {lo} && ({match_var}) <= {hi} ? ({arm_body}) : ({chain}))"
                    )
                }
                Pat::Tuple(_) | Pat::TupleStruct { .. } => arm_body,
            };
        }

        if prefix.is_empty() {
            chain
        } else {
            format!("({{ {prefix}{chain}; }})")
        }
    }

    /// Emit a single match arm body as an expression.
    /// Tagged enum variants with data bindings use GNU `({ bindings; body; })`.
    fn emit_match_arm_as_expr(
        &mut self,
        arm: &MatchArm,
        match_var: &str,
        is_tagged: bool,
        enum_decl: &Option<EnumDecl>,
        ctx: &mut Ctx,
    ) -> String {
        if let Pat::EnumVariant {
            variant, bindings, ..
        } = &arm.pat
            && is_tagged
            && !matches!(bindings, PatBindings::None)
        {
            let mut binding_decls = String::new();
            let mut arm_ctx = ctx.clone();
            if let Some(edecl) = enum_decl
                && let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant)
            {
                match bindings {
                    PatBindings::Tuple(pats) => {
                        if let VariantFields::Tuple(tys) = &ev.fields {
                            for (i, sub_pat) in pats.iter().enumerate() {
                                let name = match sub_pat {
                                    Pat::Binding(n) if n != "_" => n.clone(),
                                    _ => continue,
                                };
                                let fty = tys
                                    .get(i)
                                    .map(ty_str)
                                    .unwrap_or_else(|| "int64_t".to_string());
                                binding_decls.push_str(&format!(
                                    "const {fty} {name} = {match_var}.data.{variant}._{i}; "
                                ));
                                if let Some(Ty::Named(n)) = tys.get(i) {
                                    arm_ctx.type_env.insert(name, n.clone());
                                }
                            }
                        }
                    }
                    PatBindings::Named(binds, _) => {
                        if let VariantFields::Named(fields) = &ev.fields {
                            for (field_name, binding_name) in binds {
                                if binding_name == "_" {
                                    continue;
                                }
                                let fty = fields
                                    .iter()
                                    .find(|f| f.name == *field_name)
                                    .map(|f| ty_str(&f.ty))
                                    .unwrap_or_else(|| "int64_t".to_string());
                                binding_decls.push_str(&format!(
                                    "const {fty} {binding_name} = {match_var}.data.{variant}.{field_name}; "
                                ));
                                if let Some(f) = fields.iter().find(|f| f.name == *field_name)
                                    && let Ty::Named(n) = &f.ty
                                {
                                    arm_ctx.type_env.insert(binding_name.clone(), n.clone());
                                }
                            }
                        }
                    }
                    PatBindings::None => {}
                }
            }
            let tail = self.emit_block_as_expr(&arm.body, &mut arm_ctx);
            if binding_decls.is_empty() {
                tail
            } else {
                format!("({{ {binding_decls}{tail}; }})")
            }
        } else {
            self.emit_block_as_expr(&arm.body, ctx)
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    /// Emit an expression, with an optional type hint for tuple/array literals.
    fn emit_expr_hint(&mut self, expr: &Expr, ctx: &mut Ctx, hint: Option<&Ty>) -> String {
        match &expr.kind {
            ExprKind::Tuple(elems) => {
                let elem_tys: Vec<Ty> = match hint {
                    Some(Ty::Tuple(tys)) => tys.clone(),
                    _ => elems.iter().map(|_| Ty::I64).collect(),
                };
                let name = tuple_typedef_name(&elem_tys);
                let fields: Vec<String> = elems
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        format!("._{i} = {}", self.emit_expr_hint(e, ctx, elem_tys.get(i)))
                    })
                    .collect();
                format!("({name}){{{}}}", fields.join(", "))
            }
            ExprKind::ArrayLit(elems) => {
                let elem_hint = if let Some(Ty::Array(inner, _)) = hint {
                    Some(inner.as_ref())
                } else {
                    None
                };
                let items: Vec<String> = elems
                    .iter()
                    .map(|e| self.emit_expr_hint(e, ctx, elem_hint))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            // Int literal with numeric type hint: emit a plain number (no INT64_C macro)
            ExprKind::Int(n) => match hint {
                Some(Ty::I8 | Ty::I16 | Ty::I32 | Ty::U8 | Ty::U16 | Ty::U32) => format!("{n}"),
                Some(Ty::U64 | Ty::Usize) => format!("{n}"),
                _ => format!("INT64_C({n})"),
            },
            _ => self.emit_expr(expr, ctx),
        }
    }

    fn emit_expr(&mut self, expr: &Expr, ctx: &mut Ctx) -> String {
        match &expr.kind {
            ExprKind::Int(n) => format!("INT64_C({n})"),
            ExprKind::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{f:.1}")
                } else {
                    format!("{f}")
                }
            }
            ExprKind::Char(c) => format!("UINT32_C({c})"),
            ExprKind::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ExprKind::Str(s) => {
                // Escape the string for C
                let mut out = "\"".to_string();
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '\r' => out.push_str("\\r"),
                        '\0' => out.push_str("\\0"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            ExprKind::Var(name) => {
                // Unit struct used as a value: emit a zero-initialized compound literal.
                if let Some(s) = self.structs.get(name.as_str())
                    && s.fields.is_empty()
                    && !s.is_tuple
                {
                    return format!("({name}){{}}");
                }
                name.clone()
            }

            ExprKind::ArrayLit(elems) => {
                let items: Vec<String> = elems.iter().map(|e| self.emit_expr(e, ctx)).collect();
                format!("{{{}}}", items.join(", "))
            }

            ExprKind::Index { expr, index } => {
                let expr_c = self.emit_expr(expr, ctx);
                // `arr[lo..hi]` -- emit as pointer offset `(arr + lo)`.
                // `arr[lo..]`   -- emit as `(arr + lo)`.
                // `arr[..hi]`   -- emit as `arr` (pointer from start).
                if let ExprKind::Range { start, end: _ } = &index.kind {
                    let start_c = start
                        .as_deref()
                        .map(|e| self.emit_expr(e, ctx))
                        .unwrap_or_else(|| "0".to_string());
                    format!("({expr_c} + {start_c})")
                } else {
                    format!("{expr_c}[{}]", self.emit_expr(index, ctx))
                }
            }

            // `lo..hi` as a standalone expression -- unusual outside index/for but valid.
            ExprKind::Range { .. } => "/* range */0".to_string(),

            ExprKind::Tuple(elems) if elems.is_empty() => "/* () */0".to_string(),
            ExprKind::Tuple(_) => self.emit_expr_hint(expr, ctx, None),

            ExprKind::StructLit { name, fields } => {
                let inits: Vec<String> = fields
                    .iter()
                    .map(|(n, e)| format!(".{n} = {}", self.emit_expr(e, ctx)))
                    .collect();
                format!("({name}){{{}}}", inits.join(", "))
            }

            // Struct-like enum variant: `Type::Variant { x: e, ... }`
            // Look up field order in the enum declaration, emit constructor call.
            // If type_name_variant is not an enum, treat as module-qualified struct literal.
            ExprKind::EnumStructLit {
                type_name,
                variant,
                fields,
            } => {
                let mangled = format!("{type_name}_{variant}");
                if let Some(edecl) = self.enums.get(type_name.as_str()).cloned() {
                    let arg_exprs: Vec<String> =
                        if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                            if let VariantFields::Named(decl_fields) = &ev.fields {
                                decl_fields
                                    .iter()
                                    .map(|df| {
                                        fields
                                            .iter()
                                            .find(|(n, _)| n == &df.name)
                                            .map(|(_, e)| self.emit_expr(e, ctx))
                                            .unwrap_or_else(|| "0".to_string())
                                    })
                                    .collect()
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        };
                    format!("{mangled}({})", arg_exprs.join(", "))
                } else {
                    // Module-qualified struct literal: math::Vec2 { x: 1.0, y: 2.0 }
                    let inits: Vec<String> = fields
                        .iter()
                        .map(|(n, e)| format!(".{n} = {}", self.emit_expr(e, ctx)))
                        .collect();
                    format!("({mangled}){{{}}}", inits.join(", "))
                }
            }

            ExprKind::Field { expr, field } => {
                let is_self_ptr =
                    matches!(&expr.kind, ExprKind::Var(n) if n == "self") && ctx.self_by_ref;
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

            ExprKind::Call { name, args } => {
                // Detect tuple struct constructor: `Point(1, 2)` -> `(Point){._0=1, ._1=2}`
                if let Some(s) = self.structs.get(name.as_str()).cloned()
                    && s.is_tuple
                {
                    let inits: Vec<String> = args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let hint = s.fields.get(i).map(|f| &f.ty);
                            format!("._{i} = {}", self.emit_expr_hint(a, ctx, hint))
                        })
                        .collect();
                    return format!("({name}){{{}}}", inits.join(", "));
                }
                let param_tys = self
                    .fn_params
                    .get(name.as_str())
                    .cloned()
                    .unwrap_or_default();
                let args_s: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| self.emit_expr_hint(a, ctx, param_tys.get(i)))
                    .collect();
                format!("{name}({})", args_s.join(", "))
            }

            ExprKind::AssocCall {
                type_name,
                method,
                args,
            } => {
                let is_enum = self.enums.contains_key(type_name.as_str());
                let is_tagged_enum = is_enum
                    && self.enums.get(type_name.as_str()).is_some_and(|e| {
                        e.variants
                            .iter()
                            .any(|v| !matches!(v.fields, VariantFields::Unit))
                    });

                if is_enum && !is_tagged_enum {
                    // Simple C enum: emit as constant
                    format!("{type_name}_{method}")
                } else {
                    let mangled = format!("{type_name}_{method}");
                    let param_tys = self.fn_params.get(&mangled).cloned().unwrap_or_default();
                    let args_s: Vec<String> = args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| self.emit_expr_hint(a, ctx, param_tys.get(i)))
                        .collect();
                    if args.is_empty() {
                        // Tagged enum unit variants and struct static methods both need `()`.
                        format!("{type_name}_{method}()")
                    } else {
                        format!("{type_name}_{method}({})", args_s.join(", "))
                    }
                }
            }

            ExprKind::MethodCall { expr, method, args } => {
                // Check for dyn trait dispatch first.
                if let ExprKind::Var(n) = &expr.kind
                    && let Some(Ty::DynTrait(_)) = ctx.var_types.get(n.as_str())
                {
                    let args_s: Vec<String> = args.iter().map(|a| self.emit_expr(a, ctx)).collect();
                    return if args_s.is_empty() {
                        format!("{n}.vtable->{method}({n}.data)")
                    } else {
                        format!("{n}.vtable->{method}({n}.data, {})", args_s.join(", "))
                    };
                }
                let type_name = match &expr.kind {
                    ExprKind::Var(n) => ctx.type_env.get(n.as_str()).cloned(),
                    _ => self.infer_type_name(expr, ctx),
                };
                let args_s: Vec<String> = args.iter().map(|a| self.emit_expr(a, ctx)).collect();
                let expr_c = self.emit_expr(expr, ctx);

                match type_name {
                    Some(ref t) => {
                        let mangled = format!("{t}_{method}");
                        if self.fn_params.contains_key(&mangled)
                            || ctx.value_self_fns.contains(mangled.as_str())
                        {
                            // Inherent method.
                            let self_arg = if ctx.value_self_fns.contains(mangled.as_str())
                                || (matches!(&expr.kind, ExprKind::Var(n) if n == "self")
                                    && ctx.self_by_ref)
                            {
                                // Value receiver OR already-pointer self: pass directly.
                                expr_c
                            } else {
                                format!("&({expr_c})")
                            };
                            if args_s.is_empty() {
                                format!("{mangled}({self_arg})")
                            } else {
                                format!("{mangled}({self_arg}, {})", args_s.join(", "))
                            }
                        } else if let Some(trampoline) = self
                            .trait_method_trampolines
                            .get(t.as_str())
                            .and_then(|m| m.get(method.as_str()))
                            .cloned()
                        {
                            // Trait impl method on concrete type — call trampoline with void* cast.
                            let self_ptr = if matches!(&expr.kind, ExprKind::Var(n) if n == "self")
                                && ctx.self_by_ref
                            {
                                format!("(void*){expr_c}")
                            } else {
                                format!("(void*)&({expr_c})")
                            };
                            if args_s.is_empty() {
                                format!("{trampoline}({self_ptr})")
                            } else {
                                format!("{trampoline}({self_ptr}, {})", args_s.join(", "))
                            }
                        } else {
                            format!(
                                "/* unknown method */{expr_c}.{method}({})",
                                args_s.join(", ")
                            )
                        }
                    }
                    None => format!("/* unknown type */{expr_c}.{method}({})", args_s.join(", ")),
                }
            }

            ExprKind::UnOp { op, operand } => {
                let op_s = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                    UnOp::BitNot => "~",
                };
                format!("({op_s}{})", self.emit_expr(operand, ctx))
            }

            ExprKind::BinOp { op, lhs, rhs } => {
                let op_s = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Rem => "%",
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    BinOp::Shl => "<<",
                    BinOp::Shr => ">>",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Gt => ">",
                    BinOp::Le => "<=",
                    BinOp::Ge => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                };
                format!(
                    "({} {op_s} {})",
                    self.emit_expr(lhs, ctx),
                    self.emit_expr(rhs, ctx)
                )
            }

            ExprKind::AddrOf { mutable, expr } => {
                let _ = mutable; // semantics preserved in C type; no runtime difference
                format!("(&{})", self.emit_expr(expr, ctx))
            }

            ExprKind::Deref(expr) => {
                format!("(*{})", self.emit_expr(expr, ctx))
            }

            ExprKind::Cast { expr, ty } => {
                if let Ty::DynTrait(trait_name) = ty {
                    // Coercion to dyn Trait: build a fat pointer.
                    // Try to infer the concrete type from the inner expression.
                    let concrete = match &expr.kind {
                        ExprKind::AddrOf { expr: inner, .. } | ExprKind::Deref(inner) => {
                            match &inner.kind {
                                ExprKind::Var(n) => ctx.type_env.get(n.as_str()).cloned(),
                                _ => None,
                            }
                        }
                        ExprKind::Var(n) => ctx.type_env.get(n.as_str()).cloned(),
                        _ => None,
                    };
                    let expr_c = self.emit_expr(expr, ctx);
                    if let Some(type_name) = concrete {
                        let vtable = format!("{type_name}__{trait_name}__vtable");
                        format!("(dyn_{trait_name}){{.data=(void*)({expr_c}), .vtable=&{vtable}}}")
                    } else {
                        // Fallback: emit without vtable (C compiler will catch missing fields).
                        format!("(dyn_{trait_name}){{.data=(void*)({expr_c})}}")
                    }
                } else {
                    format!("(({})({}))", ty_str(ty), self.emit_expr(expr, ctx))
                }
            }

            // `unsafe { stmts }` — delegate to emit_block_as_expr.
            ExprKind::Unsafe(block) => self.emit_block_as_expr(block, ctx),

            ExprKind::Block(block) => self.emit_block_as_expr(block, ctx),

            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let cond_s = self.emit_expr(cond, ctx);
                let then_s = self.emit_block_as_expr(then_block, ctx);
                let else_s = else_block
                    .as_ref()
                    .map(|b| self.emit_block_as_expr(b, ctx))
                    .unwrap_or_else(|| "(void)0".to_string());
                format!("(({cond_s}) ? ({then_s}) : ({else_s}))")
            }

            ExprKind::Match { expr, arms, .. } => self.emit_match_as_expr(expr, arms, ctx),

            ExprKind::Loop { body, result_ty } => {
                // `loop { ... }` as expression: use GNU statement expr with result variable.
                // `break val` inside stores val into `_lv`, then `break`.
                let saved = std::mem::take(&mut self.out);
                let mut inner_ctx = ctx.clone();
                inner_ctx.loop_result_var = Some("_lv".to_string());
                for s in &body.stmts {
                    self.emit_stmt(s, &mut inner_ctx, 0);
                }
                let inner = std::mem::take(&mut self.out);
                self.out = saved;
                let inner = inner.trim_end().to_string();
                let decl = typed_or_auto_decl(result_ty.as_ref(), "_lv");
                let init = result_ty.as_ref().map(zero_init).unwrap_or("0");
                format!("({{ {decl} = {init}; for (;;) {{ {inner} }} _lv; }})")
            }
        }
    }

    /// Try to infer the struct/enum type name of an expression for method dispatch.
    fn infer_type_name(&self, expr: &Expr, ctx: &Ctx) -> Option<String> {
        match &expr.kind {
            ExprKind::Var(n) => ctx.type_env.get(n.as_str()).cloned(),
            ExprKind::Call { name, .. } => {
                if let Some(Ty::Named(t)) = ctx.fn_ret_tys.get(name.as_str()) {
                    Some(t.clone())
                } else {
                    None
                }
            }
            ExprKind::AssocCall {
                type_name, method, ..
            } => {
                let mangled = format!("{type_name}_{method}");
                if let Some(Ty::Named(t)) = ctx.fn_ret_tys.get(mangled.as_str()) {
                    Some(t.clone())
                } else {
                    None
                }
            }
            ExprKind::MethodCall {
                expr: inner,
                method,
                ..
            } => {
                if let Some(recv_type) = self.infer_type_name(inner, ctx) {
                    let mangled = format!("{recv_type}_{method}");
                    if let Some(Ty::Named(t)) = ctx.fn_ret_tys.get(mangled.as_str()) {
                        return Some(t.clone());
                    }
                }
                None
            }
            ExprKind::Field { expr: base, field } => {
                if let Some(struct_name) = self.infer_type_name(base, ctx) {
                    if let Some(sdecl) = self.structs.get(&struct_name) {
                        if let Some(fd) = sdecl.fields.iter().find(|f| f.name == *field) {
                            if let Ty::Named(t) = &fd.ty {
                                return Some(t.clone());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn emit_println(
        &mut self,
        format: &str,
        args: &[Expr],
        newline: bool,
        stderr: bool,
        ctx: &mut Ctx,
    ) -> String {
        let mut fmt_parts: Vec<String> = Vec::new();
        let mut fmt_c = String::new();
        let mut chars = format.chars().peekable();
        let mut arg_idx = 0usize;
        while let Some(ch) = chars.next() {
            if ch == '{' && chars.peek() == Some(&'}') {
                chars.next();
                let spec = if let Some(arg) = args.get(arg_idx) {
                    printf_spec(self, arg, ctx)
                } else {
                    "%lld".to_string()
                };
                fmt_c.push_str(&spec);
                fmt_parts.push(spec);
                arg_idx += 1;
            } else {
                match ch {
                    '"' => fmt_c.push_str("\\\""),
                    '\\' => fmt_c.push_str("\\\\"),
                    '\n' => fmt_c.push_str("\\n"),
                    '\t' => fmt_c.push_str("\\t"),
                    c => fmt_c.push(c),
                }
            }
        }
        let nl = if newline { "\\n" } else { "" };
        let stream = if stderr { "stderr" } else { "stdout" };
        let fmt_str = format!("\"{fmt_c}{nl}\"");
        if args.is_empty() {
            format!("fprintf({stream}, {fmt_str});")
        } else {
            let args_s: Vec<String> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let spec = fmt_parts.get(i).map(|s| s.as_str()).unwrap_or("%lld");
                    let e = self.emit_expr(a, ctx);
                    if spec == "%f" {
                        format!("(double)({e})")
                    } else if spec == "%u" {
                        format!("(unsigned int)({e})")
                    } else if spec == "%s" {
                        e // no cast for strings
                    } else {
                        format!("(long long)({e})")
                    }
                })
                .collect();
            format!("fprintf({stream}, {fmt_str}, {});", args_s.join(", "))
        }
    }
}

/// Choose printf format specifier for an expression, returning `%f`, `%u`, `%s`, or `%lld`.
/// Uses variable types, cast targets, struct field types, and function return types.
fn printf_spec(codegen: &Codegen, expr: &Expr, ctx: &Ctx) -> String {
    match &expr.kind {
        ExprKind::Float(_) => "%f".to_string(),
        ExprKind::Char(_) => "%u".to_string(),
        ExprKind::Str(_) => "%s".to_string(),

        ExprKind::Cast { ty, .. } => match ty {
            Ty::F32 | Ty::F64 => "%f".to_string(),
            Ty::Char => "%u".to_string(),
            Ty::Str => "%s".to_string(),
            _ => "%lld".to_string(),
        },

        ExprKind::Var(name) => {
            // First try local variables, then global consts/statics.
            if let Some(ty) = ctx.var_types.get(name) {
                return spec_for_ty(Some(ty), ctx);
            }
            spec_for_ty(codegen.const_types.get(name.as_str()), ctx)
        }

        // Arithmetic inherits spec from the left operand.
        ExprKind::BinOp { lhs, .. } => printf_spec(codegen, lhs, ctx),
        ExprKind::UnOp { operand, .. } => printf_spec(codegen, operand, ctx),

        // Field access: try to resolve field type from struct declarations.
        ExprKind::Field { expr: base, field } => {
            if let ExprKind::Var(n) = &base.kind {
                if let Some(struct_name) = ctx.type_env.get(n) {
                    if let Some(sdecl) = codegen.structs.get(struct_name) {
                        if let Some(fd) = sdecl.fields.iter().find(|f| f.name == *field) {
                            return spec_for_ty(Some(&fd.ty), ctx);
                        }
                    }
                }
            }
            printf_spec(codegen, base, ctx)
        }

        // Calls: look up the return type of the function/method.
        ExprKind::Call { name, .. } => spec_for_ty(ctx.fn_ret_tys.get(name.as_str()), ctx),
        ExprKind::AssocCall {
            type_name, method, ..
        } => {
            let mangled = format!("{type_name}_{method}");
            spec_for_ty(ctx.fn_ret_tys.get(mangled.as_str()), ctx)
        }
        ExprKind::MethodCall { expr, method, .. } => {
            if let ExprKind::Var(recv) = &expr.kind {
                // Dyn trait receiver: key is "dyn_TraitName_method".
                if let Some(Ty::DynTrait(trait_name)) = ctx.var_types.get(recv) {
                    let key = format!("dyn_{trait_name}_{method}");
                    return spec_for_ty(ctx.fn_ret_tys.get(key.as_str()), ctx);
                }
                // Concrete type receiver: key is "TypeName_method".
                if let Some(type_name) = ctx.type_env.get(recv) {
                    let mangled = format!("{type_name}_{method}");
                    return spec_for_ty(ctx.fn_ret_tys.get(mangled.as_str()), ctx);
                }
            }
            "%lld".to_string()
        }

        ExprKind::Deref(inner) => printf_spec(codegen, inner, ctx),

        _ => "%lld".to_string(),
    }
}

/// Map an optional `Ty` to a printf format specifier, resolving type aliases.
fn spec_for_ty(ty: Option<&Ty>, ctx: &Ctx) -> String {
    match ty {
        Some(Ty::F32) | Some(Ty::F64) => "%f".to_string(),
        Some(Ty::Char) => "%u".to_string(),
        Some(Ty::Str) => "%s".to_string(),
        // Resolve named type aliases one level (common case: type Meters = f64).
        Some(Ty::Named(name)) => {
            if let Some(underlying) = ctx.type_aliases.get(name) {
                spec_for_ty(Some(underlying), ctx)
            } else {
                "%lld".to_string()
            }
        }
        _ => "%lld".to_string(),
    }
}

fn collect_tuple_types(file: &File) -> Vec<Vec<Ty>> {
    let mut found: Vec<Vec<Ty>> = Vec::new();
    scan_items(&file.items, &mut found);
    found
}

fn scan_items(items: &[Item], found: &mut Vec<Vec<Ty>>) {
    for item in items {
        match item {
            Item::Fn(f) => {
                scan_ty(&f.return_ty, found);
                for p in &f.params {
                    scan_ty(&p.ty, found);
                }
                scan_block(&f.body, found);
            }
            Item::Struct(s) => {
                for f in &s.fields {
                    scan_ty(&f.ty, found);
                }
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    match &v.fields {
                        VariantFields::Unit => {}
                        VariantFields::Tuple(tys) => {
                            for ty in tys {
                                scan_ty(ty, found);
                            }
                        }
                        VariantFields::Named(fields) => {
                            for f in fields {
                                scan_ty(&f.ty, found);
                            }
                        }
                    }
                }
            }
            Item::Impl(imp) => {
                for m in &imp.methods {
                    scan_ty(&m.return_ty, found);
                    for p in &m.params {
                        scan_ty(&p.ty, found);
                    }
                    scan_block(&m.body, found);
                }
            }
            Item::TypeAlias { ty, .. } => scan_ty(ty, found),
            Item::Mod {
                items: mod_items, ..
            } => scan_items(mod_items, found),
            Item::Trait(_)
            | Item::Skip
            | Item::ExternBlock(_)
            | Item::Const { .. }
            | Item::Static { .. } => {}
        }
    }
}

fn scan_ty(ty: &Ty, found: &mut Vec<Vec<Ty>>) {
    match ty {
        Ty::Tuple(tys) => {
            if !found.iter().any(|f| f == tys) {
                found.push(tys.clone());
            }
            for t in tys {
                scan_ty(t, found);
            }
        }
        Ty::Array(inner, _) | Ty::Slice(inner) => scan_ty(inner, found),
        Ty::FnPtr { params, ret } => {
            for p in params {
                scan_ty(p, found);
            }
            scan_ty(ret, found);
        }
        Ty::Ref(inner) | Ty::RefMut(inner) | Ty::RawConst(inner) | Ty::RawMut(inner) => {
            scan_ty(inner, found);
        }
        _ => {}
    }
}

fn scan_block(block: &crate::ast::Block, found: &mut Vec<Vec<Ty>>) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let { ty, expr, .. } => {
                if let Some(t) = ty {
                    scan_ty(t, found);
                }
                scan_expr(expr, ty.as_ref(), found);
            }
            StmtKind::LetPat {
                ty,
                expr,
                else_block,
                ..
            } => {
                if let Some(t) = ty {
                    scan_ty(t, found);
                }
                scan_expr(expr, ty.as_ref(), found);
                if let Some(else_block) = else_block {
                    scan_block(else_block, found);
                }
            }
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                scan_expr(cond, None, found);
                scan_block(then_block, found);
                if let Some(b) = else_block {
                    scan_block(b, found);
                }
            }
            StmtKind::While { cond, body } => {
                scan_expr(cond, None, found);
                scan_block(body, found);
            }
            StmtKind::WhileLet { expr, body, .. } => {
                scan_expr(expr, None, found);
                scan_block(body, found);
            }
            StmtKind::Match { expr, arms, .. } => {
                scan_expr(expr, None, found);
                for arm in arms {
                    scan_block(&arm.body, found);
                }
            }
            StmtKind::Return(Some(e)) => scan_expr(e, None, found),
            StmtKind::Println { args, .. } => {
                for a in args {
                    scan_expr(a, None, found);
                }
            }
            StmtKind::Expr(e) => scan_expr(e, None, found),
            _ => {}
        }
    }
}

fn scan_expr(expr: &Expr, hint: Option<&Ty>, found: &mut Vec<Vec<Ty>>) {
    match &expr.kind {
        ExprKind::Tuple(elems) => {
            let tys: Vec<Ty> = match hint {
                Some(Ty::Tuple(tys)) => tys.clone(),
                _ => elems.iter().map(|_| Ty::I64).collect(),
            };
            if !found.iter().any(|f| f == &tys) {
                found.push(tys);
            }
            for e in elems {
                scan_expr(e, None, found);
            }
        }
        ExprKind::BinOp { lhs, rhs, .. } => {
            scan_expr(lhs, None, found);
            scan_expr(rhs, None, found);
        }
        ExprKind::UnOp { operand, .. } => scan_expr(operand, None, found),
        ExprKind::Call { args, .. } => {
            for a in args {
                scan_expr(a, None, found);
            }
        }
        ExprKind::AssocCall { args, .. } => {
            for a in args {
                scan_expr(a, None, found);
            }
        }
        ExprKind::MethodCall { expr, args, .. } => {
            scan_expr(expr, None, found);
            for a in args {
                scan_expr(a, None, found);
            }
        }
        ExprKind::Field { expr, .. } => scan_expr(expr, None, found),
        ExprKind::StructLit { fields, .. } => {
            for (_, e) in fields {
                scan_expr(e, None, found);
            }
        }
        ExprKind::EnumStructLit { fields, .. } => {
            for (_, e) in fields {
                scan_expr(e, None, found);
            }
        }
        ExprKind::AddrOf { expr, .. } => scan_expr(expr, None, found),
        ExprKind::Deref(expr) => scan_expr(expr, None, found),
        ExprKind::Cast { expr, ty } => {
            scan_expr(expr, None, found);
            scan_ty(ty, found);
        }
        ExprKind::Index { expr, index } => {
            scan_expr(expr, None, found);
            scan_expr(index, None, found);
        }
        ExprKind::Range { start, end } => {
            if let Some(e) = start {
                scan_expr(e, None, found);
            }
            if let Some(e) = end {
                scan_expr(e, None, found);
            }
        }
        ExprKind::Unsafe(block) | ExprKind::Block(block) => scan_block(block, found),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            scan_expr(cond, None, found);
            scan_block(then_block, found);
            if let Some(b) = else_block {
                scan_block(b, found);
            }
        }
        ExprKind::Match { expr, arms, .. } => {
            scan_expr(expr, None, found);
            for arm in arms {
                scan_block(&arm.body, found);
            }
        }
        ExprKind::Loop { body: block, .. } => scan_block(block, found),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers: type strings, signatures, padding
// ---------------------------------------------------------------------------

/// Return a typed C declaration "TYPE name", or fall back to "__auto_type name"
/// for types that cannot be used as a direct r-value in C (arrays/slices/None).
fn typed_or_auto_decl(ty: Option<&Ty>, name: &str) -> String {
    match ty {
        Some(t) if !matches!(t, Ty::Array(_, _) | Ty::Slice(_)) => ty_str_decl(t, name),
        _ => format!("__auto_type {name}"),
    }
}

/// Return the appropriate C zero-initializer literal for a type.
/// Aggregate types need `{0}`; scalars and pointers use `0`.
fn zero_init(ty: &Ty) -> &str {
    match ty {
        Ty::Named(_) | Ty::Tuple(_) | Ty::Array(_, _) => "{0}",
        _ => "0",
    }
}

fn ty_str(ty: &Ty) -> String {
    match ty {
        Ty::I8 => "int8_t".into(),
        Ty::I16 => "int16_t".into(),
        Ty::I32 => "int32_t".into(),
        Ty::I64 => "int64_t".into(),
        Ty::Isize => "intptr_t".into(),
        Ty::U8 => "uint8_t".into(),
        Ty::U16 => "uint16_t".into(),
        Ty::U32 => "uint32_t".into(),
        Ty::U64 => "uint64_t".into(),
        Ty::Usize => "uintptr_t".into(),
        Ty::F32 => "float".into(),
        Ty::F64 => "double".into(),
        Ty::Bool => "bool".into(),
        Ty::Char => "uint32_t".into(),
        Ty::Str => "const char*".into(),
        Ty::Never => "void".into(), // _Noreturn applied at fn level
        Ty::Unit => "void".into(),
        Ty::Array(inner, n) => format!("{}[{n}]", ty_str(inner)),
        Ty::Slice(inner) => format!("{}*", ty_str(inner)),
        Ty::FnPtr { params, ret } => {
            // Anonymous fn ptr for use in cast expressions: `ret (*)(params)`
            let ps = params.iter().map(ty_str).collect::<Vec<_>>().join(", ");
            let ps = if ps.is_empty() {
                "void".to_string()
            } else {
                ps
            };
            format!("{}(*)({ps})", ty_str(ret))
        }
        Ty::Named(n) => n.clone(),
        Ty::DynTrait(t) => format!("dyn_{t}"),
        // SelfTy should be resolved before reaching ty_str; fall back to void* as safe default.
        Ty::SelfTy => "void*".to_string(),
        Ty::Tuple(tys) => tuple_typedef_name(tys),
        Ty::Ref(inner) => format!("const {}*", ty_str(inner)),
        Ty::RefMut(inner) => format!("{}*", ty_str(inner)),
        Ty::RawConst(inner) => format!("const {}*", ty_str(inner)),
        Ty::RawMut(inner) => format!("{}*", ty_str(inner)),
    }
}

fn ty_str_decl(ty: &Ty, name: &str) -> String {
    match ty {
        // C array: size goes after the name
        Ty::Array(inner, n) => format!("{} {name}[{n}]", ty_str(inner)),
        // C function pointer: ret (*name)(params)
        Ty::FnPtr { params, ret } => {
            let ps = params.iter().map(ty_str).collect::<Vec<_>>().join(", ");
            let ps = if ps.is_empty() {
                "void".to_string()
            } else {
                ps
            };
            format!("{}(*{name})({ps})", ty_str(ret))
        }
        _ => format!("{} {name}", ty_str(ty)),
    }
}

fn ty_key(ty: &Ty) -> String {
    match ty {
        Ty::I8 => "i8".into(),
        Ty::I16 => "i16".into(),
        Ty::I32 => "i32".into(),
        Ty::I64 => "i64".into(),
        Ty::Isize => "isize".into(),
        Ty::U8 => "u8".into(),
        Ty::U16 => "u16".into(),
        Ty::U32 => "u32".into(),
        Ty::U64 => "u64".into(),
        Ty::Usize => "usize".into(),
        Ty::F32 => "f32".into(),
        Ty::F64 => "f64".into(),
        Ty::Bool => "bool".into(),
        Ty::Char => "char".into(),
        Ty::Unit => "unit".into(),
        Ty::Str => "str".into(),
        Ty::Never => "never".into(),
        Ty::Array(inner, n) => format!("arr_{}_{n}", ty_key(inner)),
        Ty::Slice(inner) => format!("slice_{}", ty_key(inner)),
        Ty::FnPtr { params, ret } => {
            let ps = params.iter().map(ty_key).collect::<Vec<_>>().join("_");
            format!("fnptr_{}_ret_{}", ps, ty_key(ret))
        }
        Ty::Named(n) => n.clone(),
        Ty::DynTrait(t) => format!("dyn_{t}"),
        Ty::SelfTy => "Self".into(),
        Ty::Tuple(tys) => format!("({})", tys.iter().map(ty_key).collect::<Vec<_>>().join("_")),
        Ty::Ref(inner) => format!("ref_{}", ty_key(inner)),
        Ty::RefMut(inner) => format!("refmut_{}", ty_key(inner)),
        Ty::RawConst(inner) => format!("ptr_{}", ty_key(inner)),
        Ty::RawMut(inner) => format!("ptrm_{}", ty_key(inner)),
    }
}

fn tuple_typedef_name(tys: &[Ty]) -> String {
    format!(
        "Tuple_{}",
        tys.iter().map(ty_key).collect::<Vec<_>>().join("_")
    )
}

fn emit_tuple_typedef(out: &mut String, tys: &[Ty]) {
    let name = tuple_typedef_name(tys);
    out.push_str("typedef struct {\n");
    for (i, ty) in tys.iter().enumerate() {
        out.push_str(&format!("    {} _{};\n", ty_str(ty), i));
    }
    out.push_str(&format!("}} {name};\n"));
}

fn emit_struct(out: &mut String, s: &StructDecl, prefix: &str) {
    let name = format!("{prefix}{}", s.name);
    out.push_str(&format!("typedef struct {name} {{\n"));
    for f in &s.fields {
        out.push_str(&format!("    {};\n", ty_str_decl(&f.ty, &f.name)));
    }
    out.push_str(&format!("}} {name};\n"));
}

/// Emit a C extern declaration for an `extern "C"` fn.
/// The symbol name is the raw C name (never mangled).
fn extern_fn_decl(f: &ExternFnDecl) -> String {
    let noreturn = if f.return_ty == Ty::Never {
        "_Noreturn "
    } else {
        ""
    };
    let ret = ty_str(&f.return_ty);
    let mut param_parts: Vec<String> = f
        .params
        .iter()
        .map(|p| ty_str_decl(&p.ty, &p.name))
        .collect();
    if f.is_variadic {
        param_parts.push("...".to_string());
    }
    let params = if param_parts.is_empty() {
        if f.is_variadic {
            "...".to_string()
        } else {
            "void".to_string()
        }
    } else {
        param_parts.join(", ")
    };
    format!("extern {noreturn}{ret} {}({params});", f.name)
}

fn fn_signature(f: &FnDecl, impl_type: Option<&str>, prefix: &str) -> String {
    if f.name == "main" {
        return "int main(void)".to_string();
    }

    let ret_ty = match impl_type {
        Some(itype) => f.return_ty.resolve_self(itype),
        None => f.return_ty.clone(),
    };
    let noreturn = if ret_ty == Ty::Never {
        "_Noreturn "
    } else {
        ""
    };
    let ret = ty_str(&ret_ty);
    let mut param_parts: Vec<String> = Vec::new();

    if let (Some(recv), Some(itype)) = (&f.receiver, impl_type) {
        let self_param = match recv {
            Receiver::Value => format!("{itype} self"),
            Receiver::Ref => format!("const {itype}* self"),
            Receiver::RefMut => format!("{itype}* self"),
        };
        param_parts.push(self_param);
    }
    for p in &f.params {
        let ty = match impl_type {
            Some(itype) => p.ty.resolve_self(itype),
            None => p.ty.clone(),
        };
        param_parts.push(ty_str_decl(&ty, &p.name));
    }
    let params = if param_parts.is_empty() {
        "void".to_string()
    } else {
        param_parts.join(", ")
    };

    let mangled = match impl_type {
        Some(t) => format!("{t}_{}", f.name),
        None => format!("{prefix}{}", f.name),
    };
    format!("{noreturn}{ret} {mangled}({params})")
}

fn pad(indent: usize) -> String {
    "    ".repeat(indent)
}

// ---------------------------------------------------------------------------
// Tuple typedef collection (pre-scan)
// ---------------------------------------------------------------------------
