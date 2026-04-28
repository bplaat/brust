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
}

impl TyEnv {
    pub fn build(file: &File) -> Self {
        let mut env = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            fns: HashMap::new(),
            type_aliases: HashMap::new(),
            traits: HashMap::new(),
        };
        env.collect_items(&file.items, "");
        env
    }

    fn collect_items(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Struct(s) => {
                    self.structs
                        .insert(format!("{prefix}{}", s.name), s.clone());
                }
                Item::Enum(e) => {
                    let full = format!("{prefix}{}", e.name);
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
                    let params = f.params.iter().map(|p| p.ty.clone()).collect();
                    self.fns.insert(name, (params, f.return_ty.clone()));
                }
                Item::Trait(t) => {
                    let full = format!("{prefix}{}", t.name);
                    let mut trait_clone = t.clone();
                    trait_clone.name = full.clone();
                    self.traits.insert(full, trait_clone);
                }
                Item::Impl(imp) => {
                    let type_name = format!("{prefix}{}", imp.type_name);
                    for m in &imp.methods {
                        if imp.trait_name.is_some() {
                            // Trait impl methods: also register under TypeName_method for
                            // unambiguous resolution when calling on concrete types.
                            let mangled = format!("{type_name}_{}", m.name);
                            // Only insert if there's no inherent method with the same name.
                            self.fns.entry(mangled).or_insert_with(|| {
                                let params = m.params.iter().map(|p| p.ty.clone()).collect();
                                (params, m.return_ty.clone())
                            });
                        } else {
                            let mangled = format!("{type_name}_{}", m.name);
                            let params = m.params.iter().map(|p| p.ty.clone()).collect();
                            self.fns.insert(mangled, (params, m.return_ty.clone()));
                        }
                    }
                }
                Item::TypeAlias { name, ty } => {
                    self.type_aliases
                        .insert(format!("{prefix}{name}"), ty.clone());
                }
                Item::Mod {
                    name,
                    items: mod_items,
                } => {
                    self.collect_items(mod_items, &format!("{prefix}{name}_"));
                }
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

#[derive(Clone)]
struct VarInfo {
    ty: Ty,
    mutable: bool,
    moved: bool,
}

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

struct TypeChecker {
    env: TyEnv,
    errors: Vec<Error>,
    cur_loc: Loc,
}

impl TypeChecker {
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

    /// Resolve type alias chain (cycle-safe).
    fn resolve(&self, ty: &Ty) -> Ty {
        self.env.resolve_inner(ty, &mut HashSet::new())
    }

    /// Type compatibility with alias resolution on both sides.
    fn compat(&self, got: &Ty, expected: &Ty) -> bool {
        ty_compat(&self.resolve(got), &self.resolve(expected))
    }

    // -----------------------------------------------------------------------
    // Item traversal
    // -----------------------------------------------------------------------

    fn check_file(&mut self, file: &File) {
        self.validate_items(&file.items, "");
        self.check_items(&file.items, "");
    }

    /// Validate that all named types in declarations exist.
    fn validate_items(&mut self, items: &[Item], prefix: &str) {
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
                                if !imp.methods.iter().any(|m| m.name == sig.name) {
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
                } => {
                    self.validate_items(mod_items, &format!("{prefix}{name}_"));
                }
            }
        }
    }

    /// Check that a type's Named variants refer to declared types.
    fn validate_ty(&mut self, ty: &Ty) {
        match ty {
            Ty::Named(name) => {
                if !self.env.structs.contains_key(name)
                    && !self.env.enums.contains_key(name)
                    && !self.env.type_aliases.contains_key(name)
                {
                    self.err(format!("unknown type `{name}`"));
                }
            }
            Ty::DynTrait(name) => {
                if !self.env.traits.contains_key(name) {
                    self.err(format!("unknown trait `{name}`"));
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
            _ => {} // primitives are always valid
        }
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
                } => {
                    self.check_items(mod_items, &format!("{prefix}{name}_"));
                }
                _ => {} // traits, structs, enums, type aliases are pure declarations
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function and block checking
    // -----------------------------------------------------------------------

    fn check_fn(&mut self, f: &FnDecl, impl_type: Option<&str>, prefix: &str) {
        let mut scope = Scope::new();

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
            scope.insert(p.name.clone(), p.ty.clone(), false);
        }

        let _ = prefix; // name mangling handled by TyEnv
        self.check_block_stmts(&f.body.stmts, &mut scope, &f.return_ty);

        // All-paths-return check for non-void functions.
        if f.return_ty != Ty::Unit && f.return_ty != Ty::Never && !block_definitely_returns(&f.body)
        {
            self.cur_loc = f.loc;
            self.err(format!(
                "function `{}` may not return a value (expected `{}`)",
                f.name,
                ty_display(&f.return_ty)
            ));
        }
    }

    fn check_block(&mut self, block: &Block, scope: &mut Scope, return_ty: &Ty) {
        scope.push();
        self.check_block_stmts(&block.stmts, scope, return_ty);
        scope.pop();
    }

    fn check_block_stmts(&mut self, stmts: &[Stmt], scope: &mut Scope, return_ty: &Ty) {
        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i + 1 == stmts.len();
            // Last bare Expr is an implicit return — type-check it against return_ty.
            if is_last
                && return_ty != &Ty::Unit
                && return_ty != &Ty::Never
                && let StmtKind::Expr(expr) = &stmt.kind
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

    fn check_stmt(&mut self, stmt: &Stmt, scope: &mut Scope, return_ty: &Ty) {
        self.cur_loc = stmt.loc;
        match &stmt.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                expr,
            } => {
                let inferred = self.infer_expr(expr, scope);
                let final_ty = if let Some(ann) = ty {
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
                scope.insert(name.clone(), final_ty, *mutable);
            }

            StmtKind::Assign { name, expr } => {
                let rhs = self.infer_expr(expr, scope);
                match scope.lookup(name) {
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

            StmtKind::For { var, iter, body } => {
                // Infer element type from the array/slice expression.
                let iter_ty = self.infer_expr(iter, scope);
                let elem_ty = match &iter_ty {
                    Ty::Array(elem, _) => *elem.clone(),
                    _ => Ty::I64, // fallback for unknown iterables
                };
                scope.push();
                scope.insert(var.clone(), elem_ty, false);
                self.check_block_stmts(&body.stmts, scope, return_ty);
                scope.pop();
            }

            StmtKind::Break | StmtKind::Continue => {
                // Valid inside loops; no further type checking needed here.
            }

            StmtKind::Match { expr, arms } => {
                let scrutinee = self.infer_expr(expr, scope);
                for arm in arms {
                    self.check_arm(arm, &scrutinee, scope, return_ty);
                }
            }

            StmtKind::Println { args, .. } => {
                for a in args {
                    self.infer_expr(a, scope);
                }
            }

            StmtKind::Expr(expr) => {
                // Assignments are encoded as `BinOp { Eq, lhs, rhs }` for complex lvalues.
                if let ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs,
                    rhs,
                } = &expr.kind
                {
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
                } else {
                    self.infer_expr(expr, scope);
                }
            }
        }
    }

    fn check_arm(&mut self, arm: &MatchArm, scrutinee_ty: &Ty, scope: &mut Scope, return_ty: &Ty) {
        scope.push();

        match &arm.pat {
            Pat::Wildcard => {}

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
                            (PatBindings::Tuple(names), VariantFields::Tuple(tys)) => {
                                if names.len() > tys.len() {
                                    self.err(format!(
                                        "too many bindings for `{type_name}::{variant}`: \
                                         expected {}, found {}",
                                        tys.len(),
                                        names.len()
                                    ));
                                }
                                for (name, ty) in names.iter().zip(tys.iter()) {
                                    if name != "_" {
                                        scope.insert(name.clone(), ty.clone(), false);
                                    }
                                }
                            }
                            (PatBindings::Named(fields), VariantFields::Named(decl_fields)) => {
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
                }
                // If the enum isn't found, it may be a plain (non-data) enum — skip.
            }
        }

        self.check_block_stmts(&arm.body.stmts, scope, return_ty);
        scope.pop();
    }

    // -----------------------------------------------------------------------
    // Expression type inference
    // -----------------------------------------------------------------------

    fn infer_expr(&mut self, expr: &Expr, scope: &Scope) -> Ty {
        self.cur_loc = expr.loc;
        match &expr.kind {
            // Literals: integer/float literals use canonical types that are
            // widened by ty_compat to any compatible integer/float type.
            ExprKind::Int(_) => Ty::I64,
            ExprKind::Float(_) => Ty::F64,
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
                        self.err(format!("undefined variable `{name}`"));
                        Ty::Unit
                    }
                }
            }

            ExprKind::Tuple(elems) => {
                Ty::Tuple(elems.iter().map(|e| self.infer_expr(e, scope)).collect())
            }

            ExprKind::ArrayLit(elems) => {
                if elems.is_empty() {
                    return Ty::Array(Box::new(Ty::Unit), 0);
                }
                let first = self.infer_expr(&elems[0], scope);
                for e in &elems[1..] {
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
                if let ExprKind::Range { start, end } = &index.kind {
                    if let Some(e) = start { self.infer_expr(e, scope); }
                    if let Some(e) = end   { self.infer_expr(e, scope); }
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
                if let Some(e) = start { self.infer_expr(e, scope); }
                if let Some(e) = end   { self.infer_expr(e, scope); }
                Ty::I64 // ranges are integer-valued by default
            }

            ExprKind::StructLit { name, fields } => {
                if let Some(s) = self.env.structs.get(name).cloned() {
                    let provided: HashSet<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                    for (fname, fexpr) in fields {
                        let got = self.infer_expr(fexpr, scope);
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
                    // Check for missing required fields.
                    for df in &s.fields {
                        if !provided.contains(df.name.as_str()) {
                            self.err(format!("missing field `{}` in `{name}`", df.name));
                        }
                    }
                } else {
                    for (_, e) in fields {
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
                if let Some(edecl) = self.env.enums.get(type_name).cloned() {
                    // Typed enum variant: Type::Variant { ... }
                    if let Some(ev) = edecl.variants.iter().find(|v| v.name == *variant) {
                        if let VariantFields::Named(decl_fields) = &ev.fields {
                            let provided: HashSet<&str> =
                                fields.iter().map(|(n, _)| n.as_str()).collect();
                            for (fname, fexpr) in fields {
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
                                if !provided.contains(df.name.as_str()) {
                                    self.err(format!(
                                        "missing field `{}` in `{type_name}::{variant}`",
                                        df.name
                                    ));
                                }
                            }
                        } else {
                            for (_, e) in fields {
                                self.infer_expr(e, scope);
                            }
                        }
                    } else {
                        for (_, e) in fields {
                            self.infer_expr(e, scope);
                        }
                        self.err(format!("no variant `{variant}` in enum `{type_name}`"));
                    }
                    Ty::Named(type_name.clone())
                } else if let Some(s) = self.env.structs.get(&mangled).cloned() {
                    // Module-qualified struct literal: mod::Struct { ... }
                    let provided: HashSet<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                    for (fname, fexpr) in fields {
                        let got = self.infer_expr(fexpr, scope);
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
                        if !provided.contains(df.name.as_str()) {
                            self.err(format!("missing field `{}` in `{mangled}`", df.name));
                        }
                    }
                    Ty::Named(mangled)
                } else {
                    for (_, e) in fields {
                        self.infer_expr(e, scope);
                    }
                    self.err(format!("unknown enum `{type_name}` or struct `{mangled}`"));
                    Ty::Named(type_name.clone())
                }
            }

            ExprKind::Field { expr, field } => {
                let ty = self.infer_expr(expr, scope);
                if field.chars().all(|c| c.is_ascii_digit()) {
                    // Tuple field access: expr.0, expr.1 …
                    let idx: usize = field.parse().unwrap_or(usize::MAX);
                    match ty {
                        Ty::Tuple(tys) => tys.into_iter().nth(idx).unwrap_or_else(|| {
                            self.err(format!("tuple index {idx} out of range"));
                            Ty::Unit
                        }),
                        _ => {
                            self.err(format!("tuple index on non-tuple `{}`", ty_display(&ty)));
                            Ty::Unit
                        }
                    }
                } else {
                    self.lookup_field_ty(&ty, field)
                }
            }

            ExprKind::Call { name, args } => {
                if let Some((param_tys, ret_ty)) = self.env.fns.get(name).cloned() {
                    self.check_call_args(name, args, &param_tys, scope);
                    ret_ty
                } else if let Some(vi) = scope.lookup(name) {
                    // Call through a local function-pointer variable.
                    let fptr = self.resolve(&vi.ty);
                    if let Ty::FnPtr { params, ret } = fptr {
                        self.check_call_args(name, args, &params, scope);
                        *ret
                    } else {
                        for a in args {
                            self.infer_expr(a, scope);
                        }
                        self.err(format!(
                            "`{name}` is not callable (type: `{}`)",
                            ty_display(&vi.ty)
                        ));
                        Ty::Unit
                    }
                } else {
                    for a in args {
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
                    for a in args {
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
                        for a in args {
                            self.infer_expr(a, scope);
                        }
                        return sig.return_ty.clone();
                    }
                    for a in args {
                        self.infer_expr(a, scope);
                    }
                    self.err(format!("no method `{method}` on `dyn {trait_name}`"));
                    return Ty::Unit;
                }
                if let Some(type_name) = base_type_name(&recv_r) {
                    let mangled = format!("{type_name}_{method}");
                    if let Some((param_tys, ret_ty)) = self.env.fns.get(&mangled).cloned() {
                        self.check_call_args(&mangled, args, &param_tys, scope);
                        ret_ty
                    } else {
                        for a in args {
                            self.infer_expr(a, scope);
                        }
                        self.err(format!("no method `{method}` on `{}`", ty_display(&recv)));
                        Ty::Unit
                    }
                } else {
                    for a in args {
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
                let mut inner_scope = scope.clone();
                for stmt in &block.stmts {
                    self.check_stmt(stmt, &mut inner_scope, &Ty::Unit);
                }
                Ty::Unit
            }
        }
    }

    fn check_call_args(&mut self, name: &str, args: &[Expr], param_tys: &[Ty], scope: &Scope) {
        if args.len() != param_tys.len() {
            self.err(format!(
                "`{name}` expects {} argument(s), found {}",
                param_tys.len(),
                args.len()
            ));
            for a in args {
                self.infer_expr(a, scope);
            }
            return;
        }
        for (i, (arg, param_ty)) in args.iter().zip(param_tys.iter()).enumerate() {
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
    }

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

    fn infer_lvalue(&mut self, expr: &Expr, scope: &Scope) -> Option<Ty> {
        match &expr.kind {
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
//   - Assign to immutable variable  →  error
//   - &mut x where x is not `let mut`  →  error
//   - Use of a moved Named-type variable  →  error
//   - Passing a Named-type variable by value to a function  →  marks it moved
//
// NOT yet enforced:
//   - Shared-vs-mutable borrow conflicts across statements (future phase).
//   - Move of partially borrowed data.

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

            StmtKind::For { var, iter, body } => {
                self.check_expr(iter, scope, false);
                scope.insert(
                    var.clone(),
                    BVar { ty: Ty::Unit, mutable: false, moved: false },
                );
                self.check_block(body, scope);
            }

            StmtKind::Break | StmtKind::Continue => {}

            StmtKind::Match { expr, arms } => {
                self.check_expr(expr, scope, true);
                for arm in arms {
                    let mut arm_scope = scope.clone();
                    // Add pattern bindings (type unknown here — use Unit).
                    if let Pat::EnumVariant { bindings, .. } = &arm.pat {
                        match bindings {
                            PatBindings::Tuple(names) => {
                                for n in names {
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
                            PatBindings::Named(fields) => {
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
                if let Some(e) = start { self.check_expr(e, scope, false); }
                if let Some(e) = end   { self.check_expr(e, scope, false); }
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
                if let Some(e) = start { self.collect_borrows(e, shared, muts); }
                if let Some(e) = end   { self.collect_borrows(e, shared, muts); }
            }
            _ => {}
        }
    }
}

// ===========================================================================
// Helper predicates and formatters
// ===========================================================================

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

fn is_float(ty: &Ty) -> bool {
    matches!(ty, Ty::F32 | Ty::F64)
}

fn is_numeric(ty: &Ty) -> bool {
    is_integer(ty) || is_float(ty)
}

fn is_ref_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Ref(_) | Ty::RefMut(_) | Ty::RawConst(_) | Ty::RawMut(_)
    )
}

/// Named types (structs and enums) are non-Copy in v1.
/// All primitive types, references, raw pointers, arrays, tuples are Copy.
fn is_non_copy(_ty: &Ty) -> bool {
    // In v1 everything is treated as Copy: the C backend copies structs by value anyway.
    false
}

/// Type compatibility: `got` is acceptable where `expected` is required.
fn ty_compat(got: &Ty, expected: &Ty) -> bool {
    if got == expected {
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

/// Extract the base struct/enum name from a type (for field and method lookup).
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
        Ty::Ref(inner) => format!("&{}", ty_display(inner)),
        Ty::RefMut(inner) => format!("&mut {}", ty_display(inner)),
        Ty::RawConst(inner) => format!("*const {}", ty_display(inner)),
        Ty::RawMut(inner) => format!("*mut {}", ty_display(inner)),
    }
}

// ===========================================================================
// Public entry point
// ===========================================================================

pub fn check(file: &File) -> Vec<Error> {
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
