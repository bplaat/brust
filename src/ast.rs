#![allow(dead_code)]

use crate::loc::Loc;

/// A complete source file / compilation unit.
pub struct File {
    pub items: Vec<Item>,
}

pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Impl(ImplBlock),
    Trait(TraitDecl),
    Enum(EnumDecl),
    TypeAlias {
        name: String,
        ty: Ty,
        is_pub: bool,
    },
    Mod {
        name: String,
        items: Vec<Item>,
        is_pub: bool,
    },
    /// `extern "C" { fn name(params) -> RetTy; ... }` -- FFI function declarations.
    ExternBlock(Vec<ExternFnDecl>),
    /// Discarded item: `use ...;`, `extern crate ...;`, etc.
    Skip,
}

#[derive(Clone)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub is_pub: bool,
    pub loc: Loc,
}

#[derive(Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Ty,
    pub is_pub: bool,
}

#[derive(Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub is_pub: bool,
    pub loc: Loc,
}

#[derive(Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: VariantFields,
}

/// Shape of enum variant fields.
#[derive(Clone)]
pub enum VariantFields {
    Unit,                  // Variant
    Tuple(Vec<Ty>),        // Variant(T0, T1)
    Named(Vec<FieldDecl>), // Variant { x: T, y: T }
}

/// A single function declaration inside an `extern "C" { }` block.
/// The `name` is the exact C symbol (never mangled).
/// `is_variadic` means the function accepts additional arguments beyond `params`.
#[derive(Clone)]
pub struct ExternFnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Ty,
    pub is_variadic: bool,
    pub loc: Loc,
}

pub struct ImplBlock {
    pub type_name: String,
    /// `Some("Foo")` for `impl Foo for Bar`, `None` for plain `impl Bar`.
    pub trait_name: Option<String>,
    pub methods: Vec<FnDecl>,
}

/// Trait declaration: `trait Foo { fn method(&self, ...) -> Ty; }`.
#[derive(Clone)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethodSig>,
    pub is_pub: bool,
}

/// A method signature inside a trait declaration (no body).
#[derive(Clone)]
pub struct TraitMethodSig {
    pub name: String,
    pub receiver: Receiver,
    pub params: Vec<Param>,
    pub return_ty: Ty,
}

pub struct FnDecl {
    pub name: String,
    pub receiver: Option<Receiver>,
    pub params: Vec<Param>,
    pub return_ty: Ty,
    pub body: Block,
    pub is_pub: bool,
    pub loc: Loc,
}

/// How `self` is received in a method.
#[derive(Clone)]
pub enum Receiver {
    Value,  // self
    Ref,    // &self
    RefMut, // &mut self
}

#[derive(Clone)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
}

/// All types supported by brust.
#[derive(Clone, PartialEq)]
pub enum Ty {
    // Signed integers
    I8,
    I16,
    I32,
    I64,
    Isize,
    // Unsigned integers
    U8,
    U16,
    U32,
    U64,
    Usize,
    // Floats
    F32,
    F64,
    // Primitives
    Bool,
    Char,   // Unicode scalar value (u32 in C)
    Unit,   // ()
    Str,    // &str (const char* in C)
    Never,  // ! (diverging -- _Noreturn void in C)
    SelfTy, // Self -- the implementing type in an impl/trait context
    // Compound types
    Array(Box<Ty>, usize),                   // [T; N]
    Slice(Box<Ty>),                          // &[T] (T* in C, no length tracking)
    Tuple(Vec<Ty>),                          // (T0, T1, ...)
    FnPtr { params: Vec<Ty>, ret: Box<Ty> }, // fn(T0, T1) -> Ret
    // User-defined and pointer types
    Named(String),     // user-defined struct/enum
    DynTrait(String),  // dyn TraitName -- fat pointer {data, vtable}
    Ref(Box<Ty>),      // &T
    RefMut(Box<Ty>),   // &mut T
    RawConst(Box<Ty>), // *const T
    RawMut(Box<Ty>),   // *mut T
}

impl Ty {
    /// Replace all `SelfTy` occurrences with `Named(self_ty)` recursively.
    pub fn resolve_self(&self, self_ty: &str) -> Self {
        match self {
            Ty::SelfTy => Ty::Named(self_ty.to_string()),
            Ty::Ref(inner) => Ty::Ref(Box::new(inner.resolve_self(self_ty))),
            Ty::RefMut(inner) => Ty::RefMut(Box::new(inner.resolve_self(self_ty))),
            Ty::RawConst(inner) => Ty::RawConst(Box::new(inner.resolve_self(self_ty))),
            Ty::RawMut(inner) => Ty::RawMut(Box::new(inner.resolve_self(self_ty))),
            Ty::Slice(inner) => Ty::Slice(Box::new(inner.resolve_self(self_ty))),
            Ty::Array(inner, n) => Ty::Array(Box::new(inner.resolve_self(self_ty)), *n),
            Ty::Tuple(tys) => Ty::Tuple(tys.iter().map(|t| t.resolve_self(self_ty)).collect()),
            Ty::FnPtr { params, ret } => Ty::FnPtr {
                params: params.iter().map(|t| t.resolve_self(self_ty)).collect(),
                ret: Box::new(ret.resolve_self(self_ty)),
            },
            other => other.clone(),
        }
    }

    /// Returns true if this type contains `SelfTy` anywhere.
    pub fn contains_self(&self) -> bool {
        match self {
            Ty::SelfTy => true,
            Ty::Ref(inner)
            | Ty::RefMut(inner)
            | Ty::RawConst(inner)
            | Ty::RawMut(inner)
            | Ty::Slice(inner) => inner.contains_self(),
            Ty::Array(inner, _) => inner.contains_self(),
            Ty::Tuple(tys) => tys.iter().any(|t| t.contains_self()),
            Ty::FnPtr { params, ret } => {
                params.iter().any(|t| t.contains_self()) || ret.contains_self()
            }
            _ => false,
        }
    }
}

pub struct Block {
    pub stmts: Vec<Stmt>,
}

/// A statement node with its source location.
pub struct Stmt {
    pub kind: StmtKind,
    pub loc: Loc,
}

/// All statement forms.
pub enum StmtKind {
    /// `println!("fmt", args...)` / `print!("fmt", args...)` / `eprintln!` / `eprint!`
    Println {
        format: String,
        args: Vec<Expr>,
        newline: bool,
        stderr: bool,
    },
    /// `let [mut] name [: ty] = expr;`
    Let {
        name: String,
        mutable: bool,
        ty: Option<Ty>,
        expr: Expr,
    },
    /// `name = expr;` (also used for field/index assignment via BinOp::Eq in lvalue position)
    Assign { name: String, expr: Expr },
    /// `lhs op= rhs;` — compound assignment for complex lvalue targets (arrays, fields)
    CompoundAssign { op: BinOp, lhs: Expr, rhs: Expr },
    /// `return [expr];` or implicit tail-expression return.
    Return(Option<Expr>),
    /// `if expr { ... } [else { ... }]`
    If {
        cond: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// `if let pat = expr { ... } [else { ... }]`
    IfLet {
        pat: Pat,
        expr: Expr,
        expr_ty: Option<Ty>,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// `while expr { ... }`
    While { cond: Expr, body: Block },
    /// `loop { ... }` — infinite loop, exit via `break`
    Loop(Block),
    /// `for <var> in <expr> { ... }` — iterate over an array or range
    For {
        var: String,
        iter: Expr,
        body: Block,
        elem_ty: Option<Ty>,
    },
    /// `break [expr]` — exit the nearest enclosing loop, optionally with a value
    Break(Option<Expr>),
    /// `continue` — skip to the next iteration of the nearest enclosing loop
    Continue,
    /// `match expr { pat => { ... }, ... }`
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        scrutinee_ty: Option<Ty>,
    },
    /// Expression used as a statement (calls, assignments via BinOp::Eq, unsafe blocks).
    Expr(Expr),
}

pub struct MatchArm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Block,
    pub loc: Loc,
}

/// Match patterns supported in `match` arms.
pub enum Pat {
    Wildcard,
    Bool(bool),
    Int(i64),
    /// Binding pattern: `x` — binds the matched value to a variable name.
    Binding(String),
    /// `Pat1 | Pat2 | ...` — or-pattern (each alternative must be the same kind)
    Or(Vec<Pat>),
    EnumVariant {
        type_name: String,
        variant: String,
        bindings: PatBindings,
    },
}

/// Bindings introduced by a match pattern.
pub enum PatBindings {
    None,                         // unit variant: no bindings
    Tuple(Vec<String>),           // Variant(a, b, _)
    Named(Vec<(String, String)>), // Variant { field: binding } or shorthand { x }
}

/// An expression node with its source location.
pub struct Expr {
    pub kind: ExprKind,
    pub loc: Loc,
}

/// All expression forms.
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(String),
    Var(String),
    /// `[expr, expr, ...]` -- array literal
    ArrayLit(Vec<Expr>),
    /// `expr[index]`
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
    },
    /// `lo..hi`, `lo..`, `..hi`, or `..` -- range expression
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
    },
    /// `(expr0, expr1, ...)`
    Tuple(Vec<Expr>),
    /// `Type { field: expr, ... }` -- struct literal
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// `Type::Variant { field: expr, ... }` -- named enum variant literal
    EnumStructLit {
        type_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
    /// `expr.field` or `expr.0` (tuple index via numeric field name)
    Field {
        expr: Box<Expr>,
        field: String,
    },
    /// Free function call: `name(args)`
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// Associated function or enum variant construction: `Type::name(args)`
    AssocCall {
        type_name: String,
        method: String,
        args: Vec<Expr>,
    },
    /// Method call: `expr.method(args)`
    MethodCall {
        expr: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    UnOp {
        op: UnOp,
        operand: Box<Expr>,
    },
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `&expr` or `&mut expr` -- address-of
    AddrOf {
        mutable: bool,
        expr: Box<Expr>,
    },
    /// `*expr` -- dereference a raw pointer or reference
    Deref(Box<Expr>),
    /// `expr as Ty` -- type cast
    Cast {
        expr: Box<Expr>,
        ty: Ty,
    },
    /// `unsafe { stmts }` -- unsafe block (emitted as a plain block in C)
    Unsafe(Block),
    /// `{ stmts; expr }` -- block used as a value-producing expression
    Block(Block),
    /// `if cond { then } [else { else }]` used in expression position
    If {
        cond: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// `match expr { arms }` used in expression position
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        scrutinee_ty: Option<Ty>,
    },
    /// `loop { stmts }` used in expression position (value via `break val`)
    Loop {
        body: Block,
        result_ty: Option<Ty>,
    },
}

#[derive(Clone, Copy)]
pub enum UnOp {
    Neg,    // -x
    Not,    // !x  logical NOT
    BitNot, // ~x  bitwise NOT (^x in Rust notation)
}

#[derive(Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}
