//! Type and (minimal) borrow checker for brust.
//!
//! Sits between the parser and the codegen pipeline:
//!   source -> lexer -> parser -> [type_checker] -> codegen -> C
//!
//! Phase 1 - type checker:
//!   Build a global TyEnv from all item declarations, then walk every function
//!   body checking that expressions, calls, assignments, match arms and return
//!   statements are type-consistent. Checks include:
//!   - Named types (structs/enums) must exist at the point of use.
//!   - Struct and enum literals must supply all fields with matching types.
//!   - Function calls must match arity and parameter types.
//!   - Binary/unary operators must receive compatible operand types.
//!   - Conditions must be bool (or a bool alias).
//!   - Functions with a non-unit return type must return on all paths.
//!   - The last expression of a block is checked against the return type
//!     (implicit tail-expression return).
//!   - Module visibility: items inside a `mod` are private by default;
//!     only `pub`-marked items and fields are accessible from outside
//!     their declaring module.
//!
//! Phase 2 - borrow checker (minimal):
//!   Track ownership of non-Copy named types per scope frame. Checks:
//!   - Use after move is rejected.
//!   - Assignment to an immutable binding is rejected.
//!   - &mut of an immutable binding is rejected.
//!   - Intra-statement borrow conflicts: taking &x and &mut x in the same
//!     expression is detected via collect_borrows.
//!   - Cross-statement borrow-overlap tracking (shared vs mut across multiple
//!     statements) is deferred to a future phase.
//!
//! Limitations in v1:
//!   - Integer literals are typed as I64 but accepted for any integer type.
//!   - Float literals are typed as F64 but accepted for any float type.
//!   - Arrays and tuples are treated as Copy (only Named types are non-Copy).
//!   - Match exhaustiveness is not enforced (all-paths-return relies on arm
//!     bodies all containing returns, not on exhaustiveness of patterns).

use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, Block, EnumDecl, Expr, ExprKind, File, FnDecl, Item, MatchArm, Pat, PatBindings,
    Receiver, Stmt, StmtKind, StructDecl, TraitDecl, Ty, UnOp, VariantFields,
};
use crate::error::Error;
use crate::loc::Loc;

// ===========================================================================
// Global type environment
// ===========================================================================

#[derive(Clone)]
pub struct TyEnv {
    pub structs: HashMap<String, StructDecl>,
    pub enums: HashMap<String, EnumDecl>,
    /// mangled name -> (explicit param types, return type)
    /// Note: `self` receiver is NOT included in the param list.
    pub fns: HashMap<String, (Vec<Ty>, Ty)>,
    pub type_aliases: HashMap<String, Ty>,
    /// Trait declarations, for dyn method resolution and impl checking.
    pub traits: HashMap<String, TraitDecl>,
    /// Qualified name -> module prefix it belongs to (only for items inside a mod).
    pub item_module: HashMap<String, String>,
    /// Set of qualified names inside a module that are marked `pub`.
    pub pub_items: HashSet<String>,
    /// Struct qualified name -> set of pub field names.
    pub pub_fields: HashMap<String, HashSet<String>>,
    /// Names of `extern "C"` functions -- calls to these require an `unsafe` block.
    pub extern_fns: HashSet<String>,
    /// Names of variadic extern functions -- allowed to have more args than declared params.
    pub variadic_fns: HashSet<String>,
    /// Global const/static names -> their type.
    pub consts: HashMap<String, Ty>,
    /// Names of tuple structs (for constructor and pattern handling).
    pub tuple_structs: HashSet<String>,
}

impl TyEnv {
    /// Build the global type environment by scanning all top-level items in the file.
    pub fn build(file: &File) -> Self {
        let mut env = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            fns: HashMap::new(),
            type_aliases: HashMap::new(),
            traits: HashMap::new(),
            item_module: HashMap::new(),
            pub_items: HashSet::new(),
            pub_fields: HashMap::new(),
            extern_fns: HashSet::new(),
            variadic_fns: HashSet::new(),
            consts: HashMap::new(),
            tuple_structs: HashSet::new(),
        };
        env.collect_items(&file.items, "");
        env
    }

    /// Recursively collect declarations from `items` into the environment.
    /// `prefix` is the module path so far (e.g. `"math_"` for items inside `mod math`).
    /// Populates `structs`, `enums`, `fns`, `type_aliases`, `traits`,
    /// `item_module`, `pub_items`, and `pub_fields`.
    fn collect_items(&mut self, items: &[Item], prefix: &str) {
        let in_mod = !prefix.is_empty();
        for item in items {
            match item {
                Item::Struct(s) => {
                    let full = format!("{prefix}{}", s.name);
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if s.is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    // Track pub fields for visibility checking.
                    let pub_set: HashSet<String> = s
                        .fields
                        .iter()
                        .filter(|f| f.is_pub)
                        .map(|f| f.name.clone())
                        .collect();
                    self.pub_fields.insert(full.clone(), pub_set);
                    // Register tuple struct constructors as functions.
                    if s.is_tuple {
                        self.tuple_structs.insert(full.clone());
                        let params: Vec<Ty> = s.fields.iter().map(|f| f.ty.clone()).collect();
                        self.fns
                            .insert(full.clone(), (params, Ty::Named(full.clone())));
                    }
                    self.structs.insert(full, s.clone());
                }
                Item::Enum(e) => {
                    let full = format!("{prefix}{}", e.name);
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if e.is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    self.enums.insert(full.clone(), e.clone());
                    // Register variant constructors as functions.
                    for v in &e.variants {
                        let mangled = format!("{full}_{}", v.name);
                        let (params, ret) = match &v.fields {
                            VariantFields::Unit => (vec![], Ty::Named(full.clone())),
                            VariantFields::Tuple(tys) => (tys.clone(), Ty::Named(full.clone())),
                            VariantFields::Named(fs) => (
                                fs.iter().map(|f| f.ty.clone()).collect(),
                                Ty::Named(full.clone()),
                            ),
                        };
                        self.fns.insert(mangled, (params, ret));
                    }
                }
                Item::Fn(f) => {
                    let name = format!("{prefix}{}", f.name);
                    if in_mod {
                        self.item_module.insert(name.clone(), prefix.to_string());
                        if f.is_pub {
                            self.pub_items.insert(name.clone());
                        }
                    }
                    let params = f.params.iter().map(|p| p.ty.clone()).collect();
                    self.fns.insert(name, (params, f.return_ty.clone()));
                }
                Item::Trait(t) => {
                    let full = format!("{prefix}{}", t.name);
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if t.is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    let mut trait_clone = t.clone();
                    trait_clone.name = full.clone();
                    self.traits.insert(full, trait_clone);
                }
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    for m in &imp.methods {
                        let ret_ty = m.return_ty.resolve_self(&type_name);
                        let mangled = format!("{type_name}_{}", m.name);
                        if in_mod {
                            self.item_module.insert(mangled.clone(), prefix.to_string());
                            if m.is_pub {
                                self.pub_items.insert(mangled.clone());
                            }
                        }
                        if imp.trait_name.is_some() {
                            self.fns.entry(mangled).or_insert_with(|| {
                                let params = m
                                    .params
                                    .iter()
                                    .map(|p| p.ty.resolve_self(&type_name))
                                    .collect();
                                (params, ret_ty)
                            });
                        } else {
                            let params = m
                                .params
                                .iter()
                                .map(|p| p.ty.resolve_self(&type_name))
                                .collect();
                            self.fns.insert(mangled, (params, ret_ty));
                        }
                    }
                }
                Item::TypeAlias { name, ty, is_pub } => {
                    let full = format!("{prefix}{name}");
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if *is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    self.type_aliases.insert(full, ty.clone());
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    is_pub,
                } => {
                    let mod_prefix = format!("{prefix}{name}_");
                    if in_mod {
                        // Track the mod itself as an item for visibility of the mod name.
                        let full = format!("{prefix}{name}");
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if *is_pub {
                            self.pub_items.insert(full);
                        }
                    }
                    self.collect_items(mod_items, &mod_prefix);
                }
                Item::ExternBlock(fns) => {
                    // Extern "C" functions use their raw C name (never mangled with prefix).
                    // They are always accessible and always require `unsafe` to call.
                    for f in fns {
                        let params = f.params.iter().map(|p| p.ty.clone()).collect();
                        self.fns
                            .insert(f.name.clone(), (params, f.return_ty.clone()));
                        self.extern_fns.insert(f.name.clone());
                        if f.is_variadic {
                            self.variadic_fns.insert(f.name.clone());
                        }
                    }
                }
                Item::Const {
                    name, ty, is_pub, ..
                } => {
                    let full = format!("{prefix}{name}");
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if *is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    self.consts.insert(full, ty.clone());
                }
                Item::Static {
                    name, ty, is_pub, ..
                } => {
                    let full = format!("{prefix}{name}");
                    if in_mod {
                        self.item_module.insert(full.clone(), prefix.to_string());
                        if *is_pub {
                            self.pub_items.insert(full.clone());
                        }
                    }
                    self.consts.insert(full, ty.clone());
                }
                Item::Skip => {}
            }
        }
    }

    /// Resolve a type alias chain to its concrete type (cycle-safe).
    #[allow(dead_code)]
    pub fn resolve(&self, ty: &Ty) -> Ty {
        self.resolve_inner(ty, &mut HashSet::new())
    }

    fn resolve_inner(&self, ty: &Ty, visited: &mut HashSet<String>) -> Ty {
        if let Ty::Named(n) = ty {
            if visited.contains(n) {
                return ty.clone(); // alias cycle — stop
            }
            if let Some(t) = self.type_aliases.get(n) {
                visited.insert(n.clone());
                return self.resolve_inner(t, visited);
            }
        }
        ty.clone()
    }
}

// ===========================================================================
// Per-function variable scope
// ===========================================================================

/// Information stored for each local variable during type checking.
#[derive(Clone)]
struct VarInfo {
    ty: Ty,
    mutable: bool,
    moved: bool,
}

/// Lexically-scoped variable map: a stack of frames, one per block.
/// Looking up a name searches from the innermost frame outward.
#[derive(Clone)]
struct Scope {
    frames: Vec<HashMap<String, VarInfo>>,
}

impl Scope {
    fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
        }
    }

    fn push(&mut self) {
        self.frames.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.frames.pop();
    }

    /// Define a new variable in the innermost frame.
    fn insert(&mut self, name: String, ty: Ty, mutable: bool) {
        self.frames.last_mut().unwrap().insert(
            name,
            VarInfo {
                ty,
                mutable,
                moved: false,
            },
        );
    }

    /// Look up a variable by name, searching from the innermost frame outward.
    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get(name) {
                return Some(v);
            }
        }
        None
    }
}

// ===========================================================================
// Phase 1 — Type checker
// ===========================================================================

/// Walks the AST and emits type errors. Visibility violations are also reported here.
/// Errors are accumulated in `errors`; `cur_loc` tracks the current source location
/// for error messages. `cur_mod` is the module prefix of the item being checked
/// (e.g. `"math_"` for items in `mod math { }`, or `""` at top level).
struct TypeChecker {
    env: TyEnv,
    errors: Vec<Error>,
    cur_loc: Loc,
    /// Module prefix of the function/item currently being checked (e.g. "math_" or "").
    cur_mod: String,
    /// Whether the current expression context is inside an `unsafe` block.
    in_unsafe: bool,
}

impl TypeChecker {
    fn new(env: TyEnv) -> Self {
        Self {
            env,
            errors: Vec::new(),
            cur_loc: Loc::default(),
            cur_mod: String::new(),
            in_unsafe: false,
        }
    }

    fn err(&mut self, msg: impl Into<String>) {
        self.errors.push(Error::new(self.cur_loc, msg));
    }

    /// Check whether `name` (a fully-qualified item) is accessible from `cur_mod`.
    /// Items NOT inside any module are always accessible.
    /// Items inside a module are accessible only if:
    ///   (a) the caller is in the same module or a descendant, OR
    ///   (b) the item is marked pub.
    fn check_item_visibility(&mut self, name: &str) {
        if let Some(item_mod) = self.env.item_module.get(name).cloned() {
            let accessible =
                self.cur_mod.starts_with(item_mod.as_str()) || self.env.pub_items.contains(name);
            if !accessible {
                self.err(format!("`{name}` is private"));
            }
        }
    }

    /// Check that `field` of struct `struct_name` is accessible from `cur_mod`.
    fn check_field_visibility(&mut self, struct_name: &str, field: &str) {
        // Only enforce if the struct is inside a module.
        if let Some(struct_mod) = self.env.item_module.get(struct_name).cloned() {
            if !self.cur_mod.starts_with(struct_mod.as_str()) {
                // We're outside the struct's module: field must be pub.
                let is_pub = self
                    .env
                    .pub_fields
                    .get(struct_name)
                    .is_some_and(|s| s.contains(field));
                if !is_pub {
                    self.err(format!("field `{field}` of `{struct_name}` is private"));
                }
            }
        }
    }

    /// Resolve type alias chain (cycle-safe).
    fn resolve(&self, ty: &Ty) -> Ty {
        self.env.resolve(ty)
    }

    /// Type compatibility with alias resolution on both sides.
    fn compat(&self, got: &Ty, expected: &Ty) -> bool {
        ty_compat(&self.resolve(got), &self.resolve(expected))
    }

    // -----------------------------------------------------------------------
    // Item traversal
    // -----------------------------------------------------------------------

    /// Entry point: validate item declarations then type-check all function bodies.
    fn check_file(&mut self, file: &mut File) {
        self.validate_items(&file.items, "");
        self.check_items(&mut file.items, "");
    }

    /// Walk all item declarations and verify that every named type reference
    /// resolves to a known struct, enum, or alias, and that impl targets exist.
    fn validate_items(&mut self, items: &[Item], prefix: &str) {
        let saved_cur_mod = std::mem::replace(&mut self.cur_mod, prefix.to_string());
        for item in items {
            match item {
                Item::Struct(s) => {
                    self.cur_loc = s.loc;
                    for f in &s.fields {
                        self.validate_ty(&f.ty);
                    }
                }
                Item::Enum(e) => {
                    self.cur_loc = e.loc;
                    for v in &e.variants {
                        match &v.fields {
                            VariantFields::Tuple(tys) => {
                                for t in tys {
                                    self.validate_ty(t);
                                }
                            }
                            VariantFields::Named(fs) => {
                                for f in fs {
                                    self.validate_ty(&f.ty);
                                }
                            }
                            VariantFields::Unit => {}
                        }
                    }
                }
                Item::TypeAlias { ty, .. } => {
                    self.validate_ty(ty);
                }
                Item::Fn(f) => {
                    self.cur_loc = f.loc;
                    for p in &f.params {
                        self.validate_ty(&p.ty);
                    }
                    self.validate_ty(&f.return_ty);
                }
                Item::Impl(imp) => {
                    // Validate the impl target type exists.
                    let mangled = format!("{prefix}{}", imp.type_name);
                    if !self.env.structs.contains_key(&mangled)
                        && !self.env.enums.contains_key(&mangled)
                    {
                        self.err(format!("impl for unknown type `{}`", imp.type_name));
                    }
                    // Validate trait exists and all required methods are provided.
                    if let Some(trait_name) = &imp.trait_name {
                        if let Some(tr) = self.env.traits.get(trait_name).cloned() {
                            for sig in &tr.methods {
                                // Methods with a default body don't need to be overridden.
                                if sig.body.is_none()
                                    && !imp.methods.iter().any(|m| m.name == sig.name)
                                {
                                    self.err(format!(
                                        "`impl {trait_name} for {}` missing method `{}`",
                                        imp.type_name, sig.name
                                    ));
                                }
                            }
                        } else {
                            self.err(format!("unknown trait `{trait_name}`"));
                        }
                    }
                    for m in &imp.methods {
                        self.cur_loc = m.loc;
                        for p in &m.params {
                            self.validate_ty(&p.ty);
                        }
                        self.validate_ty(&m.return_ty);
                    }
                }
                Item::Trait(t) => {
                    // Validate method signatures in the trait definition.
                    for m in &t.methods {
                        for p in &m.params {
                            self.validate_ty(&p.ty);
                        }
                        self.validate_ty(&m.return_ty);
                    }
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.validate_items(mod_items, &format!("{prefix}{name}_"));
                    self.cur_mod = prefix.to_string();
                }
                Item::ExternBlock(fns) => {
                    // Validate param and return types of every extern fn declaration.
                    for f in fns {
                        self.cur_loc = f.loc;
                        for p in &f.params {
                            self.validate_ty(&p.ty);
                        }
                        self.validate_ty(&f.return_ty);
                    }
                }
                Item::Skip => {}
                Item::Const { ty, .. } | Item::Static { ty, .. } => {
                    self.validate_ty(ty);
                }
            }
        }
        self.cur_mod = saved_cur_mod;
    }

    /// Check that a type's Named variants refer to declared types, and are accessible.
    fn validate_ty(&mut self, ty: &Ty) {
        match ty {
            Ty::Named(name) => {
                if !self.env.structs.contains_key(name)
                    && !self.env.enums.contains_key(name)
                    && !self.env.type_aliases.contains_key(name)
                {
                    self.err(format!("unknown type `{name}`"));
                } else {
                    self.check_item_visibility(name);
                }
            }
            Ty::DynTrait(name) => {
                if !self.env.traits.contains_key(name) {
                    self.err(format!("unknown trait `{name}`"));
                } else {
                    self.check_item_visibility(name);
                }
            }
            Ty::Array(inner, _) | Ty::Slice(inner) => self.validate_ty(inner),
            Ty::Ref(inner) | Ty::RefMut(inner) => self.validate_ty(inner),
            Ty::RawConst(inner) | Ty::RawMut(inner) => self.validate_ty(inner),
            Ty::Tuple(tys) => {
                for t in tys {
                    self.validate_ty(t);
                }
            }
            Ty::FnPtr { params, ret } => {
                for p in params {
                    self.validate_ty(p);
                }
                self.validate_ty(ret);
            }
            _ => {} // primitives and SelfTy are always valid in context
        }
    }

    /// Recursively type-check all function bodies in `items`.
    /// `prefix` is the current module path (e.g. `"math_"`), stored in `cur_mod`
    /// so that visibility checks know what module is doing the calling.
    fn check_items(&mut self, items: &mut [Item], prefix: &str) {
        for item in items.iter_mut() {
            match item {
                Item::Fn(f) => {
                    let saved = std::mem::replace(&mut self.cur_mod, prefix.to_string());
                    self.check_fn(f, None, prefix);
                    self.cur_mod = saved;
                }
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    let saved = std::mem::replace(&mut self.cur_mod, prefix.to_string());
                    for m in imp.methods.iter_mut() {
                        self.check_fn(m, Some(&type_name), "");
                    }
                    self.cur_mod = saved;
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.check_items(mod_items, &format!("{prefix}{name}_"));
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function and block checking
    // -----------------------------------------------------------------------

    /// Type-check a single function or method body.
    /// `impl_type` is the mangled name of the implementing type for methods (used to
    /// resolve `Self` and insert the `self` receiver into scope).
    /// `prefix` is the module prefix used for name resolution within the body.
    fn check_fn(&mut self, f: &mut FnDecl, impl_type: Option<&str>, prefix: &str) {
        let mut scope = Scope::new();
        // Each function starts outside any unsafe block.
        self.in_unsafe = false;

        // Resolve SelfTy to the concrete implementing type.
        let return_ty = match impl_type {
            Some(itype) => f.return_ty.resolve_self(itype),
            None => f.return_ty.clone(),
        };

        // Insert `self` for methods.
        if let (Some(recv), Some(itype)) = (&f.receiver, impl_type) {
            let self_ty = match recv {
                Receiver::Value => Ty::Named(itype.to_string()),
                Receiver::Ref => Ty::Ref(Box::new(Ty::Named(itype.to_string()))),
                Receiver::RefMut => Ty::RefMut(Box::new(Ty::Named(itype.to_string()))),
            };
            scope.insert("self".to_string(), self_ty, false);
        }
        for p in &f.params {
            let ty = match impl_type {
                Some(itype) => p.ty.resolve_self(itype),
                None => p.ty.clone(),
            };
            scope.insert(p.name.clone(), ty, false);
        }

        let _ = prefix;
        self.check_block_stmts(&mut f.body.stmts, &mut scope, &return_ty);

        // All-paths-return check for non-void functions.
        if return_ty != Ty::Unit && return_ty != Ty::Never && !block_definitely_returns(&f.body) {
            self.cur_loc = f.loc;
            self.err(format!(
                "function `{}` may not return a value (expected `{}`)",
                f.name,
                ty_display(&return_ty)
            ));
        }
    }

    /// Type-check a block, pushing and popping a scope frame around it.
    fn check_block(&mut self, block: &mut Block, scope: &mut Scope, return_ty: &Ty) {
        scope.push();
        self.check_block_stmts(&mut block.stmts, scope, return_ty);
        scope.pop();
    }

    /// Type-check a list of statements.
    /// The final bare `Expr` statement is treated as an implicit return and is
    /// checked against `return_ty` directly.
    fn check_block_stmts(&mut self, stmts: &mut [Stmt], scope: &mut Scope, return_ty: &Ty) {
        let len = stmts.len();
        for (i, stmt) in stmts.iter_mut().enumerate() {
            let is_last = i + 1 == len;
            // Last bare Expr is an implicit return — type-check it against return_ty.
            if is_last
                && return_ty != &Ty::Unit
                && return_ty != &Ty::Never
                && let StmtKind::Expr(expr) = &mut stmt.kind
            {
                self.cur_loc = stmt.loc;
                let ty = self.infer_expr(expr, scope);
                if !self.compat(&ty, return_ty) {
                    self.err(format!(
                        "implicit return: expected `{}`, found `{}`",
                        ty_display(return_ty),
                        ty_display(&ty)
                    ));
                }
                continue;
            }
            self.check_stmt(stmt, scope, return_ty);
        }
    }

    // -----------------------------------------------------------------------
    // Statement checking
    // -----------------------------------------------------------------------

    fn check_stmt(&mut self, stmt: &mut Stmt, scope: &mut Scope, return_ty: &Ty) {
        self.cur_loc = stmt.loc;
        match &mut stmt.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                expr,
            } => {
                let inferred = self.infer_expr(expr, scope);
                let final_ty = if let Some(ann) = ty.as_ref() {
                    // Validate the annotation refers to a real type.
                    self.validate_ty(ann);
                    if !self.compat(&inferred, ann) {
                        self.err(format!(
                            "let `{name}`: expected `{}`, found `{}`",
                            ty_display(ann),
                            ty_display(&inferred)
                        ));
                    }
                    ann.clone()
                } else {
                    inferred
                };
                if ty.is_none() {
                    *ty = Some(final_ty.clone());
                }
                scope.insert(name.clone(), final_ty, *mutable);
            }

            StmtKind::Assign { name, expr } => {
                let rhs = self.infer_expr(expr, scope);
                match scope.lookup(name.as_str()) {
                    None => self.err(format!("undefined variable `{name}`")),
                    Some(vi) => {
                        if !vi.mutable {
                            self.err(format!("cannot assign to immutable variable `{name}`"));
                        }
                        let lhs = vi.ty.clone();
                        if !self.compat(&rhs, &lhs) {
                            self.err(format!(
                                "assign `{name}`: expected `{}`, found `{}`",
                                ty_display(&lhs),
                                ty_display(&rhs)
                            ));
                        }
                    }
                }
            }

            StmtKind::Return(Some(expr)) => {
                let ty = self.infer_expr(expr, scope);
                if !self.compat(&ty, return_ty) {
                    self.err(format!(
                        "return type mismatch: expected `{}`, found `{}`",
                        ty_display(return_ty),
                        ty_display(&ty)
                    ));
                }
            }

            StmtKind::Return(None) => {
                if !self.compat(&Ty::Unit, return_ty) && return_ty != &Ty::Never {
                    self.err(format!(
                        "empty return in function returning `{}`",
                        ty_display(return_ty)
                    ));
                }
            }

            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let ct = self.infer_expr(cond, scope);
                if !self.compat(&ct, &Ty::Bool) {
                    self.err(format!(
                        "if condition must be `bool`, found `{}`",
                        ty_display(&ct)
                    ));
                }
                self.check_block(then_block, scope, return_ty);
                if let Some(blk) = else_block {
                    self.check_block(blk, scope, return_ty);
                }
            }

            StmtKind::While { cond, body } => {
                let ct = self.infer_expr(cond, scope);
                if !self.compat(&ct, &Ty::Bool) {
                    self.err(format!(
                        "while condition must be `bool`, found `{}`",
                        ty_display(&ct)
                    ));
                }
                self.check_block(body, scope, return_ty);
            }

            StmtKind::Loop(body) => {
                self.check_block(body, scope, return_ty);
            }

            StmtKind::For {
                var,
                iter,
                body,
                elem_ty,
                iter_ty,
            } => {
                let it = self.infer_expr(iter, scope);
                *iter_ty = Some(it.clone());
                let inferred_elem_ty = match &it {
                    Ty::Array(elem, _) => *elem.clone(),
                    _ => Ty::I64,
                };
                *elem_ty = Some(inferred_elem_ty.clone());
                scope.push();
                scope.insert(var.clone(), inferred_elem_ty, false);
                self.check_block_stmts(&mut body.stmts, scope, return_ty);
                scope.pop();
            }

            StmtKind::Break(val) => {
                if let Some(e) = val {
                    self.infer_expr(e, scope);
                }
            }

            StmtKind::Continue => {}

            StmtKind::Match {
                expr,
                arms,
                scrutinee_ty,
            } => {
                let scrutinee = self.infer_expr(expr, scope);
                *scrutinee_ty = Some(scrutinee.clone());
                for arm in arms.iter_mut() {
                    self.check_arm(arm, &scrutinee, scope, return_ty);
                }
            }

            StmtKind::Println { args, .. } => {
                for a in args.iter_mut() {
                    self.infer_expr(a, scope);
                }
            }

            StmtKind::CompoundAssign { lhs, rhs, .. } => {
                let rhs_ty = self.infer_expr(rhs, scope);
                if let Some(lhs_ty) = self.infer_lvalue(lhs, scope)
                    && !ty_compat(&rhs_ty, &lhs_ty)
                {
                    self.err(format!(
                        "compound assignment: expected `{}`, found `{}`",
                        ty_display(&lhs_ty),
                        ty_display(&rhs_ty)
                    ));
                }
            }

            StmtKind::IfLet {
                pat,
                expr,
                expr_ty,
                and_cond,
                then_block,
                else_block,
            } => {
                let inferred_expr_ty = self.infer_expr(expr, scope);
                *expr_ty = Some(inferred_expr_ty.clone());
                scope.push();
                self.bind_pat_vars(pat, &inferred_expr_ty, scope);
                if let Some(cond) = and_cond {
                    let ct = self.infer_expr(cond, scope);
                    if !self.compat(&ct, &Ty::Bool) {
                        self.err(format!(
                            "if-let chain condition must be `bool`, found `{}`",
                            ty_display(&ct)
                        ));
                    }
                }
                self.check_block_stmts(&mut then_block.stmts, scope, return_ty);
                scope.pop();
                if let Some(eb) = else_block {
                    self.check_block(eb, scope, return_ty);
                }
            }

            StmtKind::WhileLet {
                pat,
                expr,
                expr_ty,
                body,
            } => {
                let inferred_expr_ty = self.infer_expr(expr, scope);
                *expr_ty = Some(inferred_expr_ty.clone());
                scope.push();
                self.bind_pat_vars(pat, &inferred_expr_ty, scope);
                self.check_block_stmts(&mut body.stmts, scope, return_ty);
                scope.pop();
            }

            StmtKind::Expr(expr) => {
                let is_assign = matches!(&expr.kind, ExprKind::BinOp { op: BinOp::Eq, .. });
                if is_assign {
                    if let ExprKind::BinOp { lhs, rhs, .. } = &mut expr.kind {
                        let rhs_ty = self.infer_expr(rhs, scope);
                        if let Some(lhs_ty) = self.infer_lvalue(lhs, scope)
                            && !ty_compat(&rhs_ty, &lhs_ty)
                        {
                            self.err(format!(
                                "assignment: expected `{}`, found `{}`",
                                ty_display(&lhs_ty),
                                ty_display(&rhs_ty)
                            ));
                        }
                    }
                } else {
                    self.infer_expr(expr, scope);
                }
            }

            StmtKind::LetPat {
                pat,
                ty,
                expr,
                else_block,
            } => {
                let inferred = self.infer_expr(expr, scope);
                let final_ty = if let Some(ann) = ty.as_ref() {
                    self.validate_ty(ann);
                    ann.clone()
                } else {
                    inferred.clone()
                };
                if ty.is_none() {
                    *ty = Some(final_ty.clone());
                }
                // If there's an else block, check it (must diverge).
                if let Some(else_block) = else_block {
                    self.check_block(else_block, scope, return_ty);
                }
                self.bind_pat_vars(pat, &inferred, scope);
            }
        }
    }

    /// Type-check a single match arm: bind pattern variables into a new scope frame
    /// and check the arm body's statements against `return_ty`.
    fn check_arm(
        &mut self,
        arm: &mut MatchArm,
        scrutinee_ty: &Ty,
        scope: &mut Scope,
        return_ty: &Ty,
    ) {
        scope.push();
        self.bind_pat_vars(&arm.pat, scrutinee_ty, scope);
        // Type-check optional guard as bool.
        if let Some(guard) = &mut arm.guard {
            let gt = self.infer_expr(guard, scope);
            if !self.compat(&gt, &Ty::Bool) {
                self.err(format!(
                    "match guard must be `bool`, found `{}`",
                    ty_display(&gt)
                ));
            }
        }
        self.check_block_stmts(&mut arm.body.stmts, scope, return_ty);
        scope.pop();
    }

    /// Bind pattern variables into the current scope frame. Also type-checks the pattern.
    fn bind_pat_vars(&mut self, pat: &Pat, scrutinee_ty: &Ty, scope: &mut Scope) {
        match pat {
            Pat::Wildcard => {}

            Pat::Binding(name) => {
                // Bind the whole scrutinee value to `name`.
                scope.insert(name.clone(), scrutinee_ty.clone(), false);
            }

            Pat::Bool(_) => {
                if scrutinee_ty != &Ty::Bool {
                    self.err(format!("bool pattern on `{}`", ty_display(scrutinee_ty)));
                }
            }

            Pat::Int(_) => {
                if !is_integer(scrutinee_ty) {
                    self.err(format!("integer pattern on `{}`", ty_display(scrutinee_ty)));
                }
            }

            Pat::Char(_) => {
                if scrutinee_ty != &Ty::Char {
                    self.err(format!("char pattern on `{}`", ty_display(scrutinee_ty)));
                }
            }

            Pat::CharRange { .. } => {
                if scrutinee_ty != &Ty::Char {
                    self.err(format!("char range pattern on `{}`", ty_display(scrutinee_ty)));
                }
            }

            Pat::At { name, pat } => {
                scope.insert(name.clone(), scrutinee_ty.clone(), false);
                self.bind_pat_vars(pat, scrutinee_ty, scope);
            }

            Pat::Range { .. } => {
                if !is_integer(scrutinee_ty) {
                    self.err(format!(
                        "range pattern on non-integer `{}`",
                        ty_display(scrutinee_ty)
                    ));
                }
            }

            Pat::Tuple(pats) => {
                if let Ty::Tuple(tys) = scrutinee_ty {
                    for (p, ty) in pats.iter().zip(tys.iter()) {
                        self.bind_pat_vars(p, ty, scope);
                    }
                }
                // Accept even without a known tuple type (let destructuring with inference).
            }

            Pat::TupleStruct { type_name, fields } => {
                if let Some(sdecl) = self.env.structs.get(type_name).cloned() {
                    for (i, pat) in fields.iter().enumerate() {
                        let field_ty = sdecl
                            .fields
                            .get(i)
                            .map(|f| f.ty.clone())
                            .unwrap_or(Ty::Unit);
                        self.bind_pat_vars(pat, &field_ty, scope);
                    }
                } else {
                    self.err(format!("unknown struct `{type_name}` in pattern"));
                }
            }

            Pat::Or(alternatives) => {
                for alt in alternatives {
                    self.bind_pat_vars(alt, scrutinee_ty, scope);
                }
            }

            Pat::EnumVariant {
                type_name,
                variant,
                bindings,
            } => {
                // Scrutinee must be Named(type_name).
                let ok = matches!(scrutinee_ty, Ty::Named(n) if n == type_name);
                if !ok {
                    self.err(format!(
                        "pattern `{type_name}::{variant}` used on `{}`",
                        ty_display(scrutinee_ty)
                    ));
                }
                if let Some(edecl) = self.env.enums.get(type_name).cloned() {
                    if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                        match (bindings, &ev.fields) {
                            (PatBindings::None, _) => {} // unit or ignored
                            (PatBindings::Tuple(pats), VariantFields::Tuple(tys)) => {
                                if pats.len() > tys.len() {
                                    self.err(format!(
                                        "too many bindings for `{type_name}::{variant}`: \
                                         expected {}, found {}",
                                        tys.len(),
                                        pats.len()
                                    ));
                                }
                                for (sub_pat, ty) in pats.iter().zip(tys.iter()) {
                                    self.bind_pat_vars(sub_pat, ty, scope);
                                }
                            }
                            (
                                PatBindings::Named(fields, _has_rest),
                                VariantFields::Named(decl_fields),
                            ) => {
                                for (field_name, binding) in fields {
                                    if let Some(df) =
                                        decl_fields.iter().find(|f| f.name == *field_name)
                                    {
                                        scope.insert(binding.clone(), df.ty.clone(), false);
                                    } else {
                                        self.err(format!(
                                            "no field `{field_name}` in `{type_name}::{variant}`"
                                        ));
                                    }
                                }
                            }
                            _ => {
                                self.err(format!(
                                    "binding shape mismatch for `{type_name}::{variant}`"
                                ));
                            }
                        }
                    } else {
                        self.err(format!("no variant `{variant}` in enum `{type_name}`"));
                    }
                } else if type_name == variant
                    && let Some(sdecl) = self.env.structs.get(type_name).cloned()
                {
                    // Struct pattern: `Point { x, y, .. }` -- type_name and variant are the same.
                    if let PatBindings::Named(fields, _has_rest) = bindings {
                        for (field_name, binding) in fields {
                            if let Some(f) = sdecl.fields.iter().find(|f| f.name == *field_name) {
                                scope.insert(binding.clone(), f.ty.clone(), false);
                            } else {
                                self.err(format!(
                                    "no field `{field_name}` in struct `{type_name}`"
                                ));
                            }
                        }
                    }
                }
                // If the enum/struct isn't found, skip -- error already reported by validate_items.
            }
        }
    }

    // -----------------------------------------------------------------------
    // Block / if / match expression helpers
    // -----------------------------------------------------------------------

    /// Infer the value type of a block used in expression position.
    /// Checks all non-tail statements, then infers the tail expression type.
    fn infer_block_value_ty(&mut self, block: &mut Block, scope: &Scope) -> Ty {
        let mut block_scope = scope.clone();
        if let Some((last, rest)) = block.stmts.split_last_mut() {
            for stmt in rest.iter_mut() {
                self.check_stmt(stmt, &mut block_scope, &Ty::Unit);
            }
            match &mut last.kind {
                StmtKind::Expr(e) | StmtKind::Return(Some(e)) => self.infer_expr(e, &block_scope),
                // A tail if/match statement also produces a value.
                StmtKind::If {
                    cond,
                    then_block,
                    else_block,
                } => {
                    let ct = self.infer_expr(cond, &block_scope);
                    if !self.compat(&ct, &Ty::Bool) {
                        self.err(format!(
                            "if condition must be `bool`, found `{}`",
                            ty_display(&ct)
                        ));
                    }
                    let then_ty = self.infer_block_value_ty(then_block, &block_scope);
                    let else_ty = else_block
                        .as_mut()
                        .map(|b| self.infer_block_value_ty(b, &block_scope))
                        .unwrap_or(Ty::Unit);
                    if !ty_compat(&then_ty, &else_ty) && !ty_compat(&else_ty, &then_ty) {
                        self.err(format!(
                            "if/else branch type mismatch: `{}` vs `{}`",
                            ty_display(&then_ty),
                            ty_display(&else_ty)
                        ));
                    }
                    then_ty
                }
                StmtKind::Match {
                    expr,
                    arms,
                    scrutinee_ty,
                } => {
                    let sc_ty = self.infer_expr(expr, &block_scope);
                    *scrutinee_ty = Some(sc_ty.clone());
                    let mut result_ty: Option<Ty> = None;
                    for arm in arms.iter_mut() {
                        let arm_ty = self.infer_arm_value_ty(arm, &sc_ty, &block_scope);
                        if let Some(ref rt) = result_ty {
                            if !ty_compat(&arm_ty, rt) && !ty_compat(rt, &arm_ty) {
                                self.err(format!(
                                    "match arms have incompatible types: `{}` vs `{}`",
                                    ty_display(rt),
                                    ty_display(&arm_ty)
                                ));
                            }
                        } else {
                            result_ty = Some(arm_ty);
                        }
                    }
                    result_ty.unwrap_or(Ty::Unit)
                }
                StmtKind::Loop(body) => {
                    fn find_break_ty(
                        tc: &mut TypeChecker,
                        stmts: &mut [Stmt],
                        scope: &Scope,
                    ) -> Option<Ty> {
                        for s in stmts {
                            match &mut s.kind {
                                StmtKind::Break(Some(v)) => return Some(tc.infer_expr(v, scope)),
                                StmtKind::If {
                                    then_block,
                                    else_block,
                                    ..
                                } => {
                                    if let Some(t) = find_break_ty(tc, &mut then_block.stmts, scope)
                                    {
                                        return Some(t);
                                    }
                                    if let Some(eb) = else_block {
                                        if let Some(t) = find_break_ty(tc, &mut eb.stmts, scope) {
                                            return Some(t);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        None
                    }
                    find_break_ty(self, &mut body.stmts, &block_scope).unwrap_or(Ty::Unit)
                }
                _ => {
                    self.check_stmt(last, &mut block_scope, &Ty::Unit);
                    Ty::Unit
                }
            }
        } else {
            Ty::Unit
        }
    }

    /// Infer the value type of a match arm body, adding pattern bindings to scope.
    fn infer_arm_value_ty(&mut self, arm: &mut MatchArm, scrutinee_ty: &Ty, scope: &Scope) -> Ty {
        let mut arm_scope = scope.clone();
        arm_scope.push();
        if let Pat::EnumVariant {
            type_name,
            variant,
            bindings,
        } = &arm.pat
        {
            if let Some(edecl) = self.env.enums.get(type_name).cloned() {
                if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                    match (bindings, &ev.fields) {
                        (PatBindings::Tuple(pats), VariantFields::Tuple(tys)) => {
                            for (sub_pat, ty) in pats.iter().zip(tys.iter()) {
                                if let Pat::Binding(name) = sub_pat {
                                    if name != "_" {
                                        arm_scope.insert(name.clone(), ty.clone(), false);
                                    }
                                }
                            }
                        }
                        (PatBindings::Named(fields, _), VariantFields::Named(decl_fields)) => {
                            for (field_name, binding) in fields {
                                if let Some(df) = decl_fields.iter().find(|f| f.name == *field_name)
                                {
                                    arm_scope.insert(binding.clone(), df.ty.clone(), false);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let _ = scrutinee_ty;
        let ty = self.infer_block_value_ty(&mut arm.body, &arm_scope);
        arm_scope.pop();
        ty
    }

    // -----------------------------------------------------------------------
    // Expression type inference
    // -----------------------------------------------------------------------

    /// Infer the type of `expr` and return it. Also enforces visibility on every
    /// named item referenced (struct, function, field). Errors are accumulated via
    /// `self.err()`; on error, a best-effort fallback type (`Ty::Unit`) is returned
    /// so checking can continue and collect multiple errors in one pass.
    fn infer_expr(&mut self, expr: &mut Expr, scope: &Scope) -> Ty {
        self.cur_loc = expr.loc;
        match &mut expr.kind {
            // Literals: suffixed integers infer as their explicit type; unsuffixed
            // integer/float literals use canonical types widened by ty_compat.
            ExprKind::Int(_, Some(ty)) => ty.clone(),
            ExprKind::Int(_, None) => Ty::I64,
            ExprKind::Float(_, Some(ty)) => ty.clone(),
            ExprKind::Float(_, None) => Ty::F64,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Str(_) => Ty::Str,

            ExprKind::Var(name) => {
                match scope.lookup(name) {
                    Some(vi) => {
                        if vi.moved {
                            self.err(format!("use of moved value `{name}`"));
                        }
                        vi.ty.clone()
                    }
                    None => {
                        // Allow using a function name as a first-class value.
                        if let Some((param_tys, ret_ty)) = self.env.fns.get(name) {
                            return Ty::FnPtr {
                                params: param_tys.clone(),
                                ret: Box::new(ret_ty.clone()),
                            };
                        }
                        // Allow referencing global const/static.
                        if let Some(ty) = self.env.consts.get(name) {
                            return ty.clone();
                        }
                        // Unit struct used as a value: `Marker` or `Marker {}`
                        if let Some(s) = self.env.structs.get(name) {
                            if s.fields.is_empty() && !s.is_tuple {
                                return Ty::Named(name.clone());
                            }
                        }
                        self.err(format!("undefined variable `{name}`"));
                        Ty::Unit
                    }
                }
            }

            ExprKind::Tuple(elems) => Ty::Tuple(
                elems
                    .iter_mut()
                    .map(|e| self.infer_expr(e, scope))
                    .collect(),
            ),

            ExprKind::ArrayLit(elems) => {
                if elems.is_empty() {
                    return Ty::Array(Box::new(Ty::Unit), 0);
                }
                let first = self.infer_expr(&mut elems[0], scope);
                for e in &mut elems[1..] {
                    let t = self.infer_expr(e, scope);
                    if !ty_compat(&t, &first) {
                        self.err(format!(
                            "array element type mismatch: expected `{}`, found `{}`",
                            ty_display(&first),
                            ty_display(&t)
                        ));
                    }
                }
                Ty::Array(Box::new(first), elems.len())
            }

            ExprKind::Index { expr, index } => {
                // Range index `arr[lo..hi]` produces a slice.
                if let ExprKind::Range { start, end } = &mut index.kind {
                    if let Some(e) = start {
                        self.infer_expr(e, scope);
                    }
                    if let Some(e) = end {
                        self.infer_expr(e, scope);
                    }
                    let arr = self.infer_expr(expr, scope);
                    return match arr {
                        Ty::Array(inner, _) | Ty::Slice(inner) => Ty::Slice(inner),
                        _ => {
                            self.err(format!("cannot slice type `{}`", ty_display(&arr)));
                            Ty::Unit
                        }
                    };
                }
                let idx = self.infer_expr(index, scope);
                if !is_integer(&idx) {
                    self.err(format!(
                        "array index must be an integer, found `{}`",
                        ty_display(&idx)
                    ));
                }
                let arr = self.infer_expr(expr, scope);
                match arr {
                    Ty::Array(inner, _) | Ty::Slice(inner) => *inner,
                    _ => {
                        self.err(format!("cannot index type `{}`", ty_display(&arr)));
                        Ty::Unit
                    }
                }
            }

            ExprKind::Range { start, end } => {
                if let Some(e) = start {
                    self.infer_expr(e, scope);
                }
                if let Some(e) = end {
                    self.infer_expr(e, scope);
                }
                Ty::I64 // ranges are integer-valued by default
            }

            ExprKind::StructLit { name, fields, rest } => {
                self.check_item_visibility(name);
                if let Some(s) = self.env.structs.get(name.as_str()).cloned() {
                    let provided: HashSet<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                    for (fname, fexpr) in fields.iter_mut() {
                        let got = self.infer_expr(fexpr, scope);
                        // Check field visibility on construction.
                        self.check_field_visibility(name, fname);
                        if let Some(fd) = s.fields.iter().find(|f| f.name == *fname) {
                            if !self.compat(&got, &fd.ty) {
                                self.err(format!(
                                    "field `{fname}` of `{name}`: expected `{}`, found `{}`",
                                    ty_display(&fd.ty),
                                    ty_display(&got)
                                ));
                            }
                        } else {
                            self.err(format!("no field `{fname}` in struct `{name}`"));
                        }
                    }
                    if let Some(base) = rest {
                        let base_ty = self.infer_expr(base, scope);
                        if !self.compat(&base_ty, &Ty::Named(name.clone())) {
                            self.err(format!(
                                "struct update base has type `{}`, expected `{name}`",
                                ty_display(&base_ty)
                            ));
                        }
                    } else {
                        // Check for missing required fields only when no rest expression.
                        for df in &s.fields {
                            if !provided.contains(&df.name) {
                                self.err(format!("missing field `{}` in `{name}`", df.name));
                            }
                        }
                    }
                } else {
                    for (_, e) in fields.iter_mut() {
                        self.infer_expr(e, scope);
                    }
                    self.err(format!("unknown struct `{name}`"));
                }
                Ty::Named(name.clone())
            }

            ExprKind::EnumStructLit {
                type_name,
                variant,
                fields,
            } => {
                let mangled = format!("{type_name}_{variant}");
                if let Some(edecl) = self.env.enums.get(type_name.as_str()).cloned() {
                    // Typed enum variant: Type::Variant { ... }
                    if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                        if let VariantFields::Named(decl_fields) = &ev.fields {
                            let provided: HashSet<String> =
                                fields.iter().map(|(n, _)| n.clone()).collect();
                            for (fname, fexpr) in fields.iter_mut() {
                                let got = self.infer_expr(fexpr, scope);
                                if let Some(df) = decl_fields.iter().find(|f| f.name == *fname) {
                                    if !self.compat(&got, &df.ty) {
                                        self.err(format!(
                                            "field `{fname}` of `{type_name}::{variant}`: \
                                             expected `{}`, found `{}`",
                                            ty_display(&df.ty),
                                            ty_display(&got)
                                        ));
                                    }
                                } else {
                                    self.err(format!(
                                        "no field `{fname}` in `{type_name}::{variant}`"
                                    ));
                                }
                            }
                            // Missing fields check.
                            for df in decl_fields.clone().iter() {
                                if !provided.contains(&df.name) {
                                    self.err(format!(
                                        "missing field `{}` in `{type_name}::{variant}`",
                                        df.name
                                    ));
                                }
                            }
                        } else {
                            for (_, e) in fields.iter_mut() {
                                self.infer_expr(e, scope);
                            }
                        }
                    } else {
                        for (_, e) in fields.iter_mut() {
                            self.infer_expr(e, scope);
                        }
                        self.err(format!("no variant `{variant}` in enum `{type_name}`"));
                    }
                    Ty::Named(type_name.clone())
                } else if let Some(s) = self.env.structs.get(&mangled).cloned() {
                    // Module-qualified struct literal: mod::Struct { ... }
                    self.check_item_visibility(&mangled);
                    let provided: HashSet<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                    for (fname, fexpr) in fields.iter_mut() {
                        let got = self.infer_expr(fexpr, scope);
                        self.check_field_visibility(&mangled, fname);
                        if let Some(fd) = s.fields.iter().find(|f| f.name == *fname) {
                            if !self.compat(&got, &fd.ty) {
                                self.err(format!(
                                    "field `{fname}` of `{mangled}`: expected `{}`, found `{}`",
                                    ty_display(&fd.ty),
                                    ty_display(&got)
                                ));
                            }
                        } else {
                            self.err(format!("no field `{fname}` in `{mangled}`"));
                        }
                    }
                    for df in &s.fields {
                        if !provided.contains(&df.name) {
                            self.err(format!("missing field `{}` in `{mangled}`", df.name));
                        }
                    }
                    Ty::Named(mangled)
                } else {
                    for (_, e) in fields.iter_mut() {
                        self.infer_expr(e, scope);
                    }
                    self.err(format!("unknown enum `{type_name}` or struct `{mangled}`"));
                    Ty::Named(type_name.clone())
                }
            }

            ExprKind::Field { expr, field } => {
                let ty = self.infer_expr(expr, scope);
                if field.chars().all(|c| c.is_ascii_digit()) {
                    // Tuple field access: expr.0, expr.1 ...
                    let idx: usize = field.parse().unwrap_or(usize::MAX);
                    match &ty {
                        Ty::Tuple(tys) => tys.iter().nth(idx).cloned().unwrap_or_else(|| {
                            self.err(format!("tuple index {idx} out of range"));
                            Ty::Unit
                        }),
                        Ty::Named(n) if self.env.tuple_structs.contains(n) => {
                            // Tuple struct field access: Point.0 -> type of field _0
                            if let Some(s) = self.env.structs.get(n) {
                                let internal_name = format!("_{idx}");
                                s.fields
                                    .iter()
                                    .find(|f| f.name == internal_name)
                                    .map(|f| f.ty.clone())
                                    .unwrap_or_else(|| {
                                        self.err(format!("tuple index {idx} out of range"));
                                        Ty::Unit
                                    })
                            } else {
                                Ty::Unit
                            }
                        }
                        _ => {
                            self.err(format!("tuple index on non-tuple `{}`", ty_display(&ty)));
                            Ty::Unit
                        }
                    }
                } else {
                    // Check field visibility.
                    let sname = match &ty {
                        Ty::Named(n) => Some(n.clone()),
                        Ty::Ref(inner) => {
                            if let Ty::Named(n) = inner.as_ref() {
                                Some(n.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(sname) = sname {
                        self.check_field_visibility(&sname, field);
                    }
                    self.lookup_field_ty(&ty, field)
                }
            }

            ExprKind::Call { name, args } => {
                if let Some((param_tys, ret_ty)) = self.env.fns.get(name).cloned() {
                    self.check_item_visibility(name);
                    // Extern "C" functions require an unsafe block at the call site.
                    if self.env.extern_fns.contains(name) && !self.in_unsafe {
                        self.err(format!(
                            "call to unsafe extern fn `{name}` must be inside an `unsafe` block"
                        ));
                    }
                    self.check_call_args(name, args, &param_tys, scope);
                    ret_ty
                } else if let Some(vi) = scope.lookup(name) {
                    // Call through a local function-pointer variable.
                    let fptr = self.resolve(&vi.ty);
                    if let Ty::FnPtr { params, ret } = fptr {
                        self.check_call_args(name, args, &params, scope);
                        *ret
                    } else {
                        for a in args.iter_mut() {
                            self.infer_expr(a, scope);
                        }
                        self.err(format!(
                            "`{name}` is not callable (type: `{}`)",
                            ty_display(&vi.ty)
                        ));
                        Ty::Unit
                    }
                } else {
                    for a in args.iter_mut() {
                        self.infer_expr(a, scope);
                    }
                    self.err(format!("undefined function `{name}`"));
                    Ty::Unit
                }
            }

            ExprKind::AssocCall {
                type_name,
                method,
                args,
            } => {
                let mangled = format!("{type_name}_{method}");
                if let Some((param_tys, ret_ty)) = self.env.fns.get(&mangled).cloned() {
                    self.check_item_visibility(&mangled);
                    self.check_call_args(&mangled, args, &param_tys, scope);
                    ret_ty
                } else if self.env.enums.get(type_name.as_str()).is_some_and(|e| {
                    e.variants
                        .iter()
                        .any(|v| v.name == *method && matches!(v.fields, VariantFields::Unit))
                }) {
                    // Unit enum variant used as a value: MyEnum::Variant
                    Ty::Named(type_name.clone())
                } else {
                    for a in args.iter_mut() {
                        self.infer_expr(a, scope);
                    }
                    self.err(format!(
                        "unknown associated function or variant `{type_name}::{method}`"
                    ));
                    Ty::Unit
                }
            }

            ExprKind::MethodCall { expr, method, args } => {
                let recv = self.infer_expr(expr, scope);
                let recv_r = self.resolve(&recv);
                // Dyn trait dispatch: look up method in the trait definition.
                if let Ty::DynTrait(trait_name) = &recv_r {
                    let trait_name = trait_name.clone();
                    if let Some(tr) = self.env.traits.get(&trait_name).cloned()
                        && let Some(sig) = tr.methods.iter().find(|m| m.name == *method)
                    {
                        for a in args.iter_mut() {
                            self.infer_expr(a, scope);
                        }
                        return sig.return_ty.clone();
                    }
                    for a in args.iter_mut() {
                        self.infer_expr(a, scope);
                    }
                    self.err(format!("no method `{method}` on `dyn {trait_name}`"));
                    return Ty::Unit;
                }
                if let Some(type_name) = base_type_name(&recv_r) {
                    let mangled = format!("{type_name}_{method}");
                    if let Some((param_tys, ret_ty)) = self.env.fns.get(&mangled).cloned() {
                        self.check_item_visibility(&mangled);
                        self.check_call_args(&mangled, args, &param_tys, scope);
                        ret_ty
                    } else {
                        for a in args.iter_mut() {
                            self.infer_expr(a, scope);
                        }
                        self.err(format!("no method `{method}` on `{}`", ty_display(&recv)));
                        Ty::Unit
                    }
                } else {
                    for a in args.iter_mut() {
                        self.infer_expr(a, scope);
                    }
                    self.err(format!(
                        "method call `.{method}()` on primitive type `{}`",
                        ty_display(&recv)
                    ));
                    Ty::Unit
                }
            }

            ExprKind::UnOp { op, operand } => {
                let ty = self.infer_expr(operand, scope);
                match op {
                    UnOp::Neg => {
                        if !is_numeric(&ty) {
                            self.err(format!("negation on non-numeric `{}`", ty_display(&ty)));
                        }
                        ty
                    }
                    UnOp::Not => {
                        // In Rust, `!` on bool is logical NOT, on integers is bitwise NOT.
                        if ty == Ty::Bool {
                            Ty::Bool
                        } else if is_integer(&ty) {
                            ty
                        } else {
                            self.err(format!(
                                "NOT operator on non-bool/integer `{}`",
                                ty_display(&ty)
                            ));
                            Ty::Bool
                        }
                    }
                    UnOp::BitNot => {
                        if !is_integer(&ty) {
                            self.err(format!("bitwise NOT on non-integer `{}`", ty_display(&ty)));
                        }
                        ty
                    }
                }
            }

            ExprKind::BinOp { op, lhs, rhs } => {
                let lty = self.infer_expr(lhs, scope);
                let rty = self.infer_expr(rhs, scope);
                self.check_binop(*op, &lty, &rty)
            }

            ExprKind::AddrOf { mutable, expr } => {
                // &mut x requires x to be a mutable binding.
                if *mutable {
                    if let ExprKind::Var(name) = &expr.kind
                        && let Some(vi) = scope.lookup(name)
                        && !vi.mutable
                    {
                        self.err(format!("cannot take `&mut` of immutable `{name}`"));
                    }
                    Ty::RefMut(Box::new(self.infer_expr(expr, scope)))
                } else {
                    Ty::Ref(Box::new(self.infer_expr(expr, scope)))
                }
            }

            ExprKind::Deref(inner) => {
                let ty = self.infer_expr(inner, scope);
                match ty {
                    Ty::Ref(t) | Ty::RefMut(t) | Ty::RawConst(t) | Ty::RawMut(t) => *t,
                    _ => {
                        self.err(format!("cannot dereference `{}`", ty_display(&ty)));
                        Ty::Unit
                    }
                }
            }

            // Cast always produces the target type (no further checking).
            ExprKind::Cast { ty, .. } => ty.clone(),

            ExprKind::Unsafe(block) => {
                // Type-check inside unsafe blocks using a copy of the current scope.
                // Save and restore `in_unsafe` to handle nested unsafe blocks correctly.
                let saved_unsafe = std::mem::replace(&mut self.in_unsafe, true);
                let mut inner_scope = scope.clone();
                let result_ty = if let Some((last, rest)) = block.stmts.split_last_mut() {
                    for stmt in rest.iter_mut() {
                        self.check_stmt(stmt, &mut inner_scope, &Ty::Unit);
                    }
                    // Tail expressions appear as either bare Expr or implicit Return.
                    let tail_expr = match &mut last.kind {
                        StmtKind::Expr(expr) => Some(expr as &mut Expr),
                        StmtKind::Return(Some(expr)) => Some(expr as &mut Expr),
                        _ => None,
                    };
                    if let Some(expr) = tail_expr {
                        self.cur_loc = last.loc;
                        self.infer_expr(expr, &inner_scope)
                    } else {
                        self.check_stmt(last, &mut inner_scope, &Ty::Unit);
                        Ty::Unit
                    }
                } else {
                    Ty::Unit
                };
                self.in_unsafe = saved_unsafe;
                result_ty
            }

            ExprKind::Block(block) => self.infer_block_value_ty(block, scope),

            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let ct = self.infer_expr(cond, scope);
                if !self.compat(&ct, &Ty::Bool) {
                    self.err(format!(
                        "if condition must be `bool`, found `{}`",
                        ty_display(&ct)
                    ));
                }
                let then_ty = self.infer_block_value_ty(then_block, scope);
                let else_ty = else_block
                    .as_mut()
                    .map(|b| self.infer_block_value_ty(b, scope))
                    .unwrap_or(Ty::Unit);
                if !ty_compat(&then_ty, &else_ty) && !ty_compat(&else_ty, &then_ty) {
                    self.err(format!(
                        "if/else branch type mismatch: `{}` vs `{}`",
                        ty_display(&then_ty),
                        ty_display(&else_ty)
                    ));
                }
                then_ty
            }

            ExprKind::Match {
                expr,
                arms,
                scrutinee_ty,
            } => {
                let sc_ty = self.infer_expr(expr, scope);
                *scrutinee_ty = Some(sc_ty.clone());
                let mut result_ty: Option<Ty> = None;
                for arm in arms.iter_mut() {
                    let arm_ty = self.infer_arm_value_ty(arm, &sc_ty, scope);
                    if let Some(ref rt) = result_ty {
                        if !ty_compat(&arm_ty, rt) && !ty_compat(rt, &arm_ty) {
                            self.err(format!(
                                "match arms have incompatible types: `{}` vs `{}`",
                                ty_display(rt),
                                ty_display(&arm_ty)
                            ));
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                }
                result_ty.unwrap_or(Ty::Unit)
            }

            ExprKind::Loop { body, result_ty } => {
                // Infer loop type from `break val` — scan body for first `break val`.
                fn find_break_expr(stmts: &mut [Stmt]) -> Option<&mut Expr> {
                    for s in stmts {
                        match &mut s.kind {
                            StmtKind::Break(Some(v)) => return Some(v),
                            StmtKind::If {
                                then_block,
                                else_block,
                                ..
                            } => {
                                if let Some(e) = find_break_expr(&mut then_block.stmts) {
                                    return Some(e);
                                }
                                if let Some(eb) = else_block {
                                    if let Some(e) = find_break_expr(&mut eb.stmts) {
                                        return Some(e);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    None
                }
                // Check the body in a temporary scope so bindings don't leak.
                let mut inner_scope = scope.clone();
                self.check_block(body, &mut inner_scope, &Ty::Unit);
                let ty = find_break_expr(&mut body.stmts)
                    .map(|v| self.infer_expr(v, scope))
                    .unwrap_or(Ty::Unit);
                *result_ty = Some(ty.clone());
                ty
            }

            ExprKind::IfLet {
                pat,
                expr,
                expr_ty,
                then_block,
                else_block,
            } => {
                let inferred_expr_ty = self.infer_expr(expr, scope);
                *expr_ty = Some(inferred_expr_ty.clone());
                let mut inner = scope.clone();
                inner.push();
                self.bind_pat_vars(pat, &inferred_expr_ty, &mut inner);
                let then_ty = self.infer_block_value_ty(then_block, &inner);
                inner.pop();
                let else_ty = else_block
                    .as_mut()
                    .map(|b| self.infer_block_value_ty(b, scope))
                    .unwrap_or(Ty::Unit);
                if !ty_compat(&then_ty, &else_ty) && !ty_compat(&else_ty, &then_ty) {
                    self.err(format!(
                        "if-let branch type mismatch: `{}` vs `{}`",
                        ty_display(&then_ty),
                        ty_display(&else_ty)
                    ));
                }
                then_ty
            }

            ExprKind::Abort { message } => {
                if let Some(msg) = message {
                    self.infer_expr(msg, scope);
                }
                // Abort diverges; treat as the expected type (Never approximation).
                Ty::Never
            }
        }
    }

    /// Verify argument count and types for a function call.
    /// For variadic extern fns, at least `param_tys.len()` args must be provided;
    /// extra arguments beyond that are accepted without type checking.
    /// Arguments are inferred even on arity mismatch so subsequent errors are still caught.
    fn check_call_args(&mut self, name: &str, args: &mut [Expr], param_tys: &[Ty], scope: &Scope) {
        let is_variadic = self.env.variadic_fns.contains(name);
        let arity_ok = if is_variadic {
            args.len() >= param_tys.len()
        } else {
            args.len() == param_tys.len()
        };
        if !arity_ok {
            let expected = if is_variadic {
                format!("at least {}", param_tys.len())
            } else {
                param_tys.len().to_string()
            };
            self.err(format!(
                "`{name}` expects {expected} argument(s), found {}",
                args.len()
            ));
            for a in args.iter_mut() {
                self.infer_expr(a, scope);
            }
            return;
        }
        // Type-check the fixed parameters; variadic extra args are inferred without checking.
        for (i, (arg, param_ty)) in args.iter_mut().zip(param_tys.iter()).enumerate() {
            let got = self.infer_expr(arg, scope);
            if !self.compat(&got, param_ty) {
                self.err(format!(
                    "argument {} of `{name}`: expected `{}`, found `{}`",
                    i + 1,
                    ty_display(param_ty),
                    ty_display(&got)
                ));
            }
        }
        // Infer types of extra variadic args (for side-effects, e.g. nested calls).
        for arg in args.iter_mut().skip(param_tys.len()) {
            self.infer_expr(arg, scope);
        }
    }

    /// Return the type of `field` on `ty`, or emit an error and return `Ty::Unit`.
    /// Resolves type aliases before looking up the struct declaration.
    fn lookup_field_ty(&mut self, ty: &Ty, field: &str) -> Ty {
        let ty_r = self.resolve(ty);
        if let Some(name) = base_type_name(&ty_r)
            && let Some(s) = self.env.structs.get(&name)
        {
            return s
                .fields
                .iter()
                .find(|f| f.name == field)
                .map(|f| f.ty.clone())
                .unwrap_or_else(|| {
                    self.err(format!("no field `{field}` on `{name}`"));
                    Ty::Unit
                });
        }
        if base_type_name(&ty_r).is_none() {
            // Primitive type -- field access is always invalid.
            self.err(format!("no field `{field}` on `{}`", ty_display(&ty_r)));
        }
        // Named type that is an enum -- field access happens through match bindings.
        Ty::Unit
    }

    /// Infer the type of an lvalue expression (for compound assignment type checking).
    /// Returns `None` for expressions that are not valid lvalues.
    fn infer_lvalue(&mut self, expr: &mut Expr, scope: &Scope) -> Option<Ty> {
        match &mut expr.kind {
            ExprKind::Var(name) => scope.lookup(name).map(|vi| vi.ty.clone()),
            ExprKind::Field { expr, field } => {
                let ty = self.infer_expr(expr, scope);
                if field.chars().all(|c| c.is_ascii_digit()) {
                    let idx: usize = field.parse().unwrap_or(usize::MAX);
                    if let Ty::Tuple(tys) = ty {
                        tys.into_iter().nth(idx)
                    } else {
                        None
                    }
                } else {
                    Some(self.lookup_field_ty(&ty, field))
                }
            }
            ExprKind::Index { expr, .. } => {
                let ty = self.infer_expr(expr, scope);
                match ty {
                    Ty::Array(inner, _) | Ty::Slice(inner) => Some(*inner),
                    _ => None,
                }
            }
            ExprKind::Deref(inner) => {
                let ty = self.infer_expr(inner, scope);
                match ty {
                    Ty::RawMut(inner) | Ty::RefMut(inner) => Some(*inner),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Check operand types for a binary operator and return the result type.
    /// Resolves aliases on both sides before checking. Returns the LHS type for
    /// arithmetic/bitwise ops and `Ty::Bool` for comparisons and logical ops.
    fn check_binop(&mut self, op: BinOp, lty: &Ty, rty: &Ty) -> Ty {
        let lty_r = self.resolve(lty);
        let rty_r = self.resolve(rty);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if !is_numeric(&lty_r) {
                    self.err(format!("arithmetic on non-numeric `{}`", ty_display(lty)));
                } else if !ty_compat(&rty_r, &lty_r) && !ty_compat(&lty_r, &rty_r) {
                    self.err(format!(
                        "arithmetic type mismatch: `{}` vs `{}`",
                        ty_display(lty),
                        ty_display(rty)
                    ));
                }
                lty.clone()
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                if !is_integer(&lty_r) {
                    self.err(format!("bitwise op on non-integer `{}`", ty_display(lty)));
                } else if !is_integer(&rty_r) {
                    self.err(format!(
                        "bitwise op RHS must be integer, found `{}`",
                        ty_display(rty)
                    ));
                }
                lty.clone()
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if !ty_compat(&lty_r, &rty_r) && !ty_compat(&rty_r, &lty_r) {
                    self.err(format!(
                        "comparison type mismatch: `{}` vs `{}`",
                        ty_display(lty),
                        ty_display(rty)
                    ));
                }
                Ty::Bool
            }
            BinOp::And | BinOp::Or => {
                if !self.compat(lty, &Ty::Bool) {
                    self.err(format!(
                        "logical op requires `bool`, found `{}`",
                        ty_display(lty)
                    ));
                }
                if !self.compat(rty, &Ty::Bool) {
                    self.err(format!(
                        "logical op requires `bool`, found `{}`",
                        ty_display(rty)
                    ));
                }
                Ty::Bool
            }
        }
    }
}

// ===========================================================================
// All-paths return analysis
// ===========================================================================

/// Returns true if the block definitely returns a value on all paths
/// (so the caller doesn't need to emit a "missing return" error).
fn block_definitely_returns(block: &Block) -> bool {
    match block.stmts.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_)) => true,
        // Last bare expression is an implicit return.
        Some(StmtKind::Expr(_)) => true,
        // if + else where both branches return.
        Some(StmtKind::If {
            else_block: Some(else_block),
            then_block,
            ..
        }) => block_definitely_returns(then_block) && block_definitely_returns(else_block),
        // match where every arm body returns (exhaustiveness not enforced here).
        Some(StmtKind::Match { arms, .. }) => {
            !arms.is_empty() && arms.iter().all(|arm| block_definitely_returns(&arm.body))
        }
        // A `loop` always terminates (via break or infinitely), so it definitely
        // produces a value for any function that ends with one.
        Some(StmtKind::Loop(_)) => true,
        _ => false,
    }
}

// ===========================================================================
// Phase 2 — Borrow checker (minimal)
// ===========================================================================
// Tracks per-variable:
//   ty      — declared type (from let annotation or param)
//   mutable — whether the binding is `let mut`
//   moved   — whether a non-Copy value has been moved out
//
// Rules enforced:
//   - Assign to immutable variable  ->  error
//   - &mut x where x is not `let mut`  ->  error
//   - Use of a moved Named-type variable  ->  error
//   - Passing a Named-type variable by value to a function  ->  marks it moved
//
// NOT yet enforced:
//   - Shared-vs-mutable borrow conflicts across statements (future phase).
//   - Move of partially borrowed data.

/// Per-variable information used by the borrow checker.
#[derive(Clone)]
struct BVar {
    ty: Ty,
    mutable: bool,
    moved: bool,
}

/// Lexically-scoped variable map for the borrow checker.
#[derive(Clone)]
struct BScope {
    frames: Vec<HashMap<String, BVar>>,
}

impl BScope {
    fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
        }
    }

    fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn insert(&mut self, name: String, var: BVar) {
        self.frames.last_mut().unwrap().insert(name, var);
    }

    fn get(&self, name: &str) -> Option<&BVar> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut BVar> {
        for frame in self.frames.iter_mut().rev() {
            if frame.contains_key(name) {
                return frame.get_mut(name);
            }
        }
        None
    }

    fn iter_all(&self) -> impl Iterator<Item = (&String, &BVar)> {
        self.frames.iter().flat_map(|f| f.iter())
    }
}

/// Minimal borrow checker. Runs after the type checker (only when there are no
/// type errors) and enforces move semantics, mutability, and intra-statement
/// borrow conflicts. See module-level doc for the full list of rules.
struct BorrowChecker {
    env: TyEnv,
    errors: Vec<Error>,
    cur_loc: Loc,
}

impl BorrowChecker {
    fn new(env: TyEnv) -> Self {
        Self {
            env,
            errors: Vec::new(),
            cur_loc: Loc::default(),
        }
    }

    fn err(&mut self, msg: impl Into<String>) {
        self.errors.push(Error::new(self.cur_loc, msg));
    }

    fn check_file(&mut self, file: &File) {
        self.check_items(&file.items, "");
    }

    fn check_items(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Fn(f) => self.check_fn(f, None, prefix),
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    for m in &imp.methods {
                        self.check_fn(m, Some(&type_name.clone()), "");
                    }
                }
                Item::Mod {
                    name,
                    items: mod_items,
                    ..
                } => {
                    self.check_items(mod_items, &format!("{prefix}{name}_"));
                }
                _ => {}
            }
        }
    }

    fn check_fn(&mut self, f: &FnDecl, impl_type: Option<&str>, _prefix: &str) {
        let mut scope = BScope::new();
        if let (Some(recv), Some(itype)) = (&f.receiver, impl_type) {
            let (self_ty, self_mut) = match recv {
                Receiver::Value => (Ty::Named(itype.to_string()), false),
                Receiver::Ref => (Ty::Ref(Box::new(Ty::Named(itype.to_string()))), false),
                // &mut self: binding is mutable so field assignments work.
                Receiver::RefMut => (Ty::RefMut(Box::new(Ty::Named(itype.to_string()))), true),
            };
            scope.insert(
                "self".to_string(),
                BVar {
                    ty: self_ty,
                    mutable: self_mut,
                    moved: false,
                },
            );
        }
        for p in &f.params {
            scope.insert(
                p.name.clone(),
                BVar {
                    ty: p.ty.clone(),
                    mutable: false,
                    moved: false,
                },
            );
        }
        self.check_stmts(&f.body.stmts, &mut scope);
    }

    fn check_block(&mut self, block: &Block, scope: &mut BScope) {
        scope.push();
        self.check_stmts(&block.stmts, scope);
        scope.pop();
    }

    /// Walk statements, running `collect_borrows` before each statement to detect
    /// intra-statement borrow conflicts (e.g. `foo(&x, &mut x)`).
    fn check_stmts(&mut self, stmts: &[Stmt], scope: &mut BScope) {
        for stmt in stmts {
            // Detect intra-statement borrow conflicts before full check.
            let mut shared = HashSet::new();
            let mut muts = HashSet::new();
            match &stmt.kind {
                StmtKind::Let { expr, .. } => {
                    self.collect_borrows(expr, &mut shared, &mut muts);
                }
                StmtKind::Expr(expr) | StmtKind::Return(Some(expr)) => {
                    self.collect_borrows(expr, &mut shared, &mut muts);
                }
                _ => {}
            }
            self.check_stmt(stmt, scope);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt, scope: &mut BScope) {
        self.cur_loc = stmt.loc;
        match &stmt.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                expr,
            } => {
                // Move from source variable if directly assigned.
                if let ExprKind::Var(src) = &expr.kind
                    && let Some(sv) = scope.get(src)
                {
                    if sv.moved {
                        self.err(format!("use of moved value `{src}`"));
                    } else if is_non_copy(&sv.ty) {
                        let src_ty = sv.ty.clone();
                        scope.get_mut(src).unwrap().moved = true;
                        let final_ty = ty.clone().unwrap_or(src_ty);
                        scope.insert(
                            name.clone(),
                            BVar {
                                ty: final_ty,
                                mutable: *mutable,
                                moved: false,
                            },
                        );
                        return;
                    }
                }
                self.check_expr(expr, scope, false);
                let final_ty = ty.clone().unwrap_or(Ty::Unit);
                scope.insert(
                    name.clone(),
                    BVar {
                        ty: final_ty,
                        mutable: *mutable,
                        moved: false,
                    },
                );
            }

            StmtKind::Assign { name, expr } => {
                self.check_expr(expr, scope, false);
                if let Some(bv) = scope.get(name)
                    && !bv.mutable
                {
                    self.err(format!("cannot assign to immutable variable `{name}`"));
                }
            }

            StmtKind::Return(Some(expr)) => {
                self.check_expr(expr, scope, false);
            }
            StmtKind::Return(None) => {}

            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                self.check_expr(cond, scope, true);
                let mut then_scope = scope.clone();
                self.check_block(then_block, &mut then_scope);
                let mut else_scope = scope.clone();
                if let Some(blk) = else_block {
                    self.check_block(blk, &mut else_scope);
                }
                // Propagate moves from branches conservatively.
                for (name, bv) in then_scope.iter_all() {
                    if bv.moved
                        && let Some(v) = scope.get_mut(name)
                    {
                        v.moved = true;
                    }
                }
                for (name, bv) in else_scope.iter_all() {
                    if bv.moved
                        && let Some(v) = scope.get_mut(name)
                    {
                        v.moved = true;
                    }
                }
            }

            StmtKind::While { cond, body } => {
                self.check_expr(cond, scope, true);
                self.check_block(body, scope);
            }

            StmtKind::Loop(body) => {
                self.check_block(body, scope);
            }

            StmtKind::For {
                var, iter: _, body, ..
            } => {
                scope.insert(
                    var.clone(),
                    BVar {
                        ty: Ty::Unit,
                        mutable: false,
                        moved: false,
                    },
                );
                self.check_block(body, scope);
            }

            StmtKind::Break(val) => {
                if let Some(e) = val {
                    self.check_expr(e, scope, false);
                }
            }

            StmtKind::Continue => {}

            StmtKind::CompoundAssign { lhs, rhs, .. } => {
                self.check_expr(rhs, scope, false);
                self.check_lvalue_mut(lhs, scope);
            }

            StmtKind::IfLet {
                pat,
                expr,
                then_block,
                else_block,
                ..
            } => {
                self.check_expr(expr, scope, false);
                let mut then_scope = scope.clone();
                // Add pattern bindings with placeholder types.
                if let Pat::EnumVariant { bindings, .. } = pat {
                    match bindings {
                        PatBindings::Tuple(pats) => {
                            for sub_pat in pats {
                                if let Pat::Binding(n) = sub_pat {
                                    then_scope.insert(
                                        n.clone(),
                                        BVar {
                                            ty: Ty::Unit,
                                            mutable: false,
                                            moved: false,
                                        },
                                    );
                                }
                            }
                        }
                        PatBindings::Named(fields, _) => {
                            for (_, binding) in fields {
                                then_scope.insert(
                                    binding.clone(),
                                    BVar {
                                        ty: Ty::Unit,
                                        mutable: false,
                                        moved: false,
                                    },
                                );
                            }
                        }
                        PatBindings::None => {}
                    }
                }
                self.check_block(then_block, &mut then_scope);
                if let Some(eb) = else_block {
                    self.check_block(eb, scope);
                }
            }

            StmtKind::WhileLet {
                pat, expr, body, ..
            } => {
                self.check_expr(expr, scope, false);
                let mut body_scope = scope.clone();
                if let Pat::EnumVariant { bindings, .. } = pat {
                    match bindings {
                        PatBindings::Tuple(pats) => {
                            for sub_pat in pats {
                                if let Pat::Binding(n) = sub_pat {
                                    body_scope.insert(
                                        n.clone(),
                                        BVar {
                                            ty: Ty::Unit,
                                            mutable: false,
                                            moved: false,
                                        },
                                    );
                                }
                            }
                        }
                        PatBindings::Named(fields, _) => {
                            for (_, binding) in fields {
                                body_scope.insert(
                                    binding.clone(),
                                    BVar {
                                        ty: Ty::Unit,
                                        mutable: false,
                                        moved: false,
                                    },
                                );
                            }
                        }
                        PatBindings::None => {}
                    }
                }
                self.check_block(body, &mut body_scope);
            }

            StmtKind::Match { expr: _, arms, .. } => {
                for arm in arms {
                    let mut arm_scope = scope.clone();
                    // Add pattern bindings (type unknown here -- use Unit).
                    if let Pat::EnumVariant { bindings, .. } = &arm.pat {
                        match bindings {
                            PatBindings::Tuple(pats) => {
                                for sub_pat in pats {
                                    if let Pat::Binding(n) = sub_pat {
                                        arm_scope.insert(
                                            n.clone(),
                                            BVar {
                                                ty: Ty::Unit,
                                                mutable: false,
                                                moved: false,
                                            },
                                        );
                                    }
                                }
                            }
                            PatBindings::Named(fields, _) => {
                                for (_, binding) in fields {
                                    arm_scope.insert(
                                        binding.clone(),
                                        BVar {
                                            ty: Ty::Unit,
                                            mutable: false,
                                            moved: false,
                                        },
                                    );
                                }
                            }
                            PatBindings::None => {}
                        }
                    }
                    self.check_stmts(&arm.body.stmts, &mut arm_scope);
                }
            }

            StmtKind::Println { args, .. } => {
                for a in args {
                    self.check_expr(a, scope, true);
                }
            }

            StmtKind::Expr(expr) => {
                if let ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs,
                    rhs,
                } = &expr.kind
                {
                    self.check_expr(rhs, scope, false);
                    self.check_lvalue_mut(lhs, scope);
                } else {
                    self.check_expr(expr, scope, false);
                }
            }

            StmtKind::LetPat {
                expr, else_block, ..
            } => {
                self.check_expr(expr, scope, false);
                if let Some(else_block) = else_block {
                    self.check_block(else_block, scope);
                }
            }
        }
    }

    /// Check that an lvalue is mutable before assignment.
    fn check_lvalue_mut(&mut self, expr: &Expr, scope: &BScope) {
        self.cur_loc = expr.loc;
        match &expr.kind {
            ExprKind::Var(name) => {
                if let Some(bv) = scope.get(name)
                    && !bv.mutable
                {
                    self.err(format!("cannot assign to immutable variable `{name}`"));
                }
            }
            ExprKind::Index { expr, .. } => {
                // arr[i] = ... requires arr to be mutable.
                self.check_lvalue_mut(expr, scope);
            }
            ExprKind::Deref(_) => {
                // *p = ... : pointer mutability checked by type checker.
            }
            ExprKind::Field { .. } => {
                // obj.field = ... : mutability via reference can't be checked
                // without type info; type checker already validates this.
            }
            _ => {}
        }
    }

    /// Walk an expression for borrow checking.
    /// `by_ref`: true means this use is in a reference/borrow context — don't move.
    fn check_expr(&mut self, expr: &Expr, scope: &mut BScope, by_ref: bool) {
        self.cur_loc = expr.loc;
        match &expr.kind {
            ExprKind::Var(name) => {
                if let Some(bv) = scope.get_mut(name) {
                    if bv.moved {
                        self.err(format!("use of moved value `{name}`"));
                        return;
                    }
                    if !by_ref && is_non_copy(&bv.ty) {
                        bv.moved = true;
                    }
                }
            }

            ExprKind::Call { name, args } => {
                let param_tys = self
                    .env
                    .fns
                    .get(name)
                    .map(|(p, _)| p.clone())
                    .unwrap_or_default();
                self.check_args(args, &param_tys, scope);
            }

            ExprKind::AssocCall {
                type_name,
                method,
                args,
            } => {
                let mangled = format!("{type_name}_{method}");
                let param_tys = self
                    .env
                    .fns
                    .get(&mangled)
                    .map(|(p, _)| p.clone())
                    .unwrap_or_default();
                self.check_args(args, &param_tys, scope);
            }

            ExprKind::MethodCall { expr, args, .. } => {
                // Receiver is always by-reference (conceptually).
                self.check_expr(expr, scope, true);
                for a in args {
                    self.check_expr(a, scope, false);
                }
            }

            ExprKind::AddrOf { mutable, expr } => {
                if *mutable
                    && let ExprKind::Var(name) = &expr.kind
                    && let Some(bv) = scope.get(name)
                    && !bv.mutable
                {
                    self.err(format!("cannot take `&mut` of immutable binding `{name}`"));
                }
                self.check_expr(expr, scope, true);
            }

            ExprKind::Tuple(elems) => {
                // Check for borrow conflicts within tuple construction: (&x, &mut x).
                let mut shared: HashSet<String> = HashSet::new();
                let mut muts: HashSet<String> = HashSet::new();
                for e in elems {
                    self.collect_borrows(e, &mut shared, &mut muts);
                    self.check_expr(e, scope, false);
                }
            }

            ExprKind::BinOp { lhs, rhs, .. } => {
                self.check_expr(lhs, scope, false);
                self.check_expr(rhs, scope, false);
            }
            ExprKind::UnOp { operand, .. } => self.check_expr(operand, scope, false),
            ExprKind::Deref(inner) => self.check_expr(inner, scope, true),
            ExprKind::Field { expr, .. } => self.check_expr(expr, scope, true),
            ExprKind::Index { expr, index } => {
                self.check_expr(expr, scope, true);
                self.check_expr(index, scope, false);
            }
            ExprKind::Range { start, end } => {
                if let Some(e) = start {
                    self.check_expr(e, scope, false);
                }
                if let Some(e) = end {
                    self.check_expr(e, scope, false);
                }
            }
            ExprKind::Cast { expr, .. } => self.check_expr(expr, scope, false),
            ExprKind::StructLit { fields, .. } => {
                for (_, e) in fields {
                    self.check_expr(e, scope, false);
                }
            }
            ExprKind::EnumStructLit { fields, .. } => {
                for (_, e) in fields {
                    self.check_expr(e, scope, false);
                }
            }

            ExprKind::ArrayLit(elems) => {
                for e in elems {
                    self.check_expr(e, scope, false);
                }
            }
            ExprKind::Unsafe(block) => {
                self.check_stmts(&block.stmts, scope);
            }
            ExprKind::Block(block) => {
                self.check_stmts(&block.stmts, scope);
            }
            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                self.check_expr(cond, scope, false);
                self.check_stmts(&then_block.stmts, scope);
                if let Some(b) = else_block {
                    self.check_stmts(&b.stmts, scope);
                }
            }
            ExprKind::Match { expr: _, arms, .. } => {
                for arm in arms {
                    self.check_stmts(&arm.body.stmts, scope);
                }
            }
            ExprKind::Loop { body, .. } => {
                self.check_stmts(&body.stmts, scope);
            }
            _ => {} // literals have no sub-expressions
        }
    }

    fn check_args(&mut self, args: &[Expr], param_tys: &[Ty], scope: &mut BScope) {
        for (i, arg) in args.iter().enumerate() {
            let by_ref = param_tys.get(i).is_some_and(is_ref_ty);
            self.check_expr(arg, scope, by_ref);
        }
    }

    /// Collect all direct-variable borrows in an expression tree.
    /// Detects same-expression conflicts: `foo(&x, &mut x)`.
    fn collect_borrows(
        &mut self,
        expr: &Expr,
        shared: &mut HashSet<String>,
        muts: &mut HashSet<String>,
    ) {
        self.cur_loc = expr.loc;
        match &expr.kind {
            ExprKind::AddrOf {
                mutable: false,
                expr: inner,
            } => {
                if let ExprKind::Var(name) = &inner.kind {
                    if muts.contains(name) {
                        self.err(format!(
                            "cannot borrow `{name}` as immutable: also borrowed as mutable"
                        ));
                    }
                    shared.insert(name.clone());
                }
                self.collect_borrows(inner, shared, muts);
            }
            ExprKind::AddrOf {
                mutable: true,
                expr: inner,
            } => {
                if let ExprKind::Var(name) = &inner.kind {
                    if shared.contains(name) {
                        self.err(format!(
                            "cannot borrow `{name}` as mutable: also borrowed as immutable"
                        ));
                    } else if muts.contains(name) {
                        self.err(format!("cannot borrow `{name}` as mutable more than once"));
                    }
                    muts.insert(name.clone());
                }
                self.collect_borrows(inner, shared, muts);
            }
            ExprKind::BinOp { lhs, rhs, .. } => {
                self.collect_borrows(lhs, shared, muts);
                self.collect_borrows(rhs, shared, muts);
            }
            ExprKind::UnOp { operand, .. } => self.collect_borrows(operand, shared, muts),
            ExprKind::Call { args, .. } | ExprKind::AssocCall { args, .. } => {
                for a in args {
                    self.collect_borrows(a, shared, muts);
                }
            }
            ExprKind::MethodCall { expr, args, .. } => {
                self.collect_borrows(expr, shared, muts);
                for a in args {
                    self.collect_borrows(a, shared, muts);
                }
            }
            ExprKind::Tuple(elems) | ExprKind::ArrayLit(elems) => {
                for e in elems {
                    self.collect_borrows(e, shared, muts);
                }
            }
            ExprKind::Field { expr, .. } | ExprKind::Deref(expr) | ExprKind::Cast { expr, .. } => {
                self.collect_borrows(expr, shared, muts);
            }
            ExprKind::Index { expr, index } => {
                self.collect_borrows(expr, shared, muts);
                self.collect_borrows(index, shared, muts);
            }
            ExprKind::Range { start, end } => {
                if let Some(e) = start {
                    self.collect_borrows(e, shared, muts);
                }
                if let Some(e) = end {
                    self.collect_borrows(e, shared, muts);
                }
            }
            _ => {}
        }
    }
}

// ===========================================================================
// Helper predicates and formatters
// ===========================================================================

/// True if `ty` is an integer type (signed or unsigned, any width).
fn is_integer(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::I8
            | Ty::I16
            | Ty::I32
            | Ty::I64
            | Ty::Isize
            | Ty::U8
            | Ty::U16
            | Ty::U32
            | Ty::U64
            | Ty::Usize
    )
}

/// True if `ty` is a floating-point type.
fn is_float(ty: &Ty) -> bool {
    matches!(ty, Ty::F32 | Ty::F64)
}

/// True if `ty` is any numeric type (integer or float).
fn is_numeric(ty: &Ty) -> bool {
    is_integer(ty) || is_float(ty)
}

/// True if `ty` is a reference or raw pointer (used to decide pass-by-ref in borrow check).
fn is_ref_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Ref(_) | Ty::RefMut(_) | Ty::RawConst(_) | Ty::RawMut(_)
    )
}

/// Named types (structs and enums) are non-Copy in v1.
/// All primitive types, references, raw pointers, arrays, and tuples are Copy.
/// Currently returns false for everything because the C backend copies structs
/// by value, so treating them as Copy is safe for now.
fn is_non_copy(_ty: &Ty) -> bool {
    // In v1 everything is treated as Copy: the C backend copies structs by value anyway.
    false
}

/// Type compatibility: `got` is acceptable where `expected` is required.
/// Handles widening coercions (integer/float literals, Never, &mut -> &, etc.)
/// and structural compat for arrays, tuples, and function pointers.
fn ty_compat(got: &Ty, expected: &Ty) -> bool {
    if got == expected {
        return true;
    }
    // SelfTy in annotations: compatible with any Named type (resolved in impl context).
    if matches!(expected, Ty::SelfTy) || matches!(got, Ty::SelfTy) {
        return true;
    }
    // Integer literal (I64) is compatible with any integer type.
    if got == &Ty::I64 && is_integer(expected) {
        return true;
    }
    // Float literal (F64) is compatible with any float type.
    if got == &Ty::F64 && is_float(expected) {
        return true;
    }
    // Never diverges — compatible with any type.
    if got == &Ty::Never {
        return true;
    }
    // &mut T coerces to &T.
    if let (Ty::RefMut(a), Ty::Ref(b)) = (got, expected) {
        return ty_compat(a, b);
    }
    // &mut T / &T coerce to raw pointer *mut T / *const T (C doesn't distinguish).
    if let (Ty::RefMut(a), Ty::RawMut(b)) = (got, expected) {
        return ty_compat(a, b);
    }
    if let (Ty::Ref(a), Ty::RawConst(b)) = (got, expected) {
        return ty_compat(a, b);
    }
    // Recursive array compat: [I64; N] compat [i32; N].
    if let (Ty::Array(ga, gn), Ty::Array(ea, en)) = (got, expected) {
        return gn == en && ty_compat(ga, ea);
    }
    // Recursive tuple compat: (I64, I64) compat (i32, i64).
    if let (Ty::Tuple(gs), Ty::Tuple(es)) = (got, expected) {
        return gs.len() == es.len() && gs.iter().zip(es.iter()).all(|(g, e)| ty_compat(g, e));
    }
    // Function pointer structural compat.
    if let (
        Ty::FnPtr {
            params: gp,
            ret: gr,
        },
        Ty::FnPtr {
            params: ep,
            ret: er,
        },
    ) = (got, expected)
    {
        return gp.len() == ep.len()
            && gp.iter().zip(ep.iter()).all(|(g, e)| ty_compat(g, e))
            && ty_compat(gr, er);
    }
    false
}

/// Extract the base struct/enum name from a type, peeling through references.
/// Used for field and method resolution (e.g. `&Point` -> `"Point"`).
fn base_type_name(ty: &Ty) -> Option<String> {
    match ty {
        Ty::Named(n) => Some(n.clone()),
        Ty::Ref(inner) | Ty::RefMut(inner) => base_type_name(inner),
        _ => None,
    }
}

/// Human-readable type representation for error messages.
pub fn ty_display(ty: &Ty) -> String {
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
        Ty::Unit => "()".into(),
        Ty::Str => "&str".into(),
        Ty::Never => "!".into(),
        Ty::Array(inner, n) => format!("[{}; {n}]", ty_display(inner)),
        Ty::Slice(inner) => format!("&[{}]", ty_display(inner)),
        Ty::Tuple(tys) => format!(
            "({})",
            tys.iter().map(ty_display).collect::<Vec<_>>().join(", ")
        ),
        Ty::FnPtr { params, ret } => {
            let ps = params.iter().map(ty_display).collect::<Vec<_>>().join(", ");
            format!("fn({ps}) -> {}", ty_display(ret))
        }
        Ty::Named(n) => n.clone(),
        Ty::DynTrait(t) => format!("dyn {t}"),
        Ty::SelfTy => "Self".into(),
        Ty::Ref(inner) => format!("&{}", ty_display(inner)),
        Ty::RefMut(inner) => format!("&mut {}", ty_display(inner)),
        Ty::RawConst(inner) => format!("*const {}", ty_display(inner)),
        Ty::RawMut(inner) => format!("*mut {}", ty_display(inner)),
    }
}

// ===========================================================================
// Public entry point
// ===========================================================================

pub fn check(file: &mut File) -> Vec<Error> {
    let env = TyEnv::build(file);
    let mut errors = Vec::new();

    // Phase 1: type checking
    {
        let mut tc = TypeChecker::new(env.clone());
        tc.check_file(file);
        errors.extend(tc.errors);
    }

    // Phase 2: borrow checking (only if type checking was clean, to avoid cascading noise)
    if errors.is_empty() {
        let mut bc = BorrowChecker::new(env);
        bc.check_file(file);
        errors.extend(bc.errors);
    }

    errors
}
