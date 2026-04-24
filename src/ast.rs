/// A complete source file / compilation unit.
pub struct File {
    pub items: Vec<Item>,
}

pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Impl(ImplBlock),
    Enum(EnumDecl),
    TypeAlias { name: String, ty: Ty },
}

pub struct StructDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Ty,
}

#[derive(Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
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

pub struct ImplBlock {
    pub type_name: String,
    pub methods: Vec<FnDecl>,
}

pub struct FnDecl {
    pub name: String,
    pub receiver: Option<Receiver>,
    pub params: Vec<Param>,
    pub return_ty: Ty,
    pub body: Block,
}

/// How `self` is received in a method.
pub enum Receiver {
    Value,  // self
    Ref,    // &self
    RefMut, // &mut self
}

pub struct Param {
    pub name: String,
    pub ty: Ty,
}

/// Types supported.
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
    // Other primitives
    Bool,
    Char,              // Unicode scalar value (u32)
    Unit,              // ()
    Str,               // &str (const char*)
    Never,             // ! (diverging — _Noreturn void in C)
    Array(Box<Ty>, usize), // [T; N]
    Slice(Box<Ty>),    // &[T] (v1: T*, no length)
    Tuple(Vec<Ty>),    // (T0, T1, ...)
    FnPtr {            // fn(T0, T1) -> Ret
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
    Named(String),     // user-defined struct/enum type
    Ref(Box<Ty>),      // &T
    RefMut(Box<Ty>),   // &mut T
    RawConst(Box<Ty>), // *const T
    RawMut(Box<Ty>),   // *mut T
}

pub struct Block {
    pub stmts: Vec<Stmt>,
}

pub enum Stmt {
    /// `println!("format", args...);`
    Println { format: String, args: Vec<Expr> },
    /// `let [mut] name [: ty] = expr;`
    Let {
        name: String,
        mutable: bool,
        ty: Option<Ty>,
        expr: Expr,
    },
    /// `name = expr;`
    Assign { name: String, expr: Expr },
    /// `return [expr];` or implicit tail return
    Return(Option<Expr>),
    /// `if expr { ... } [else { ... }]`
    If {
        cond: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// `while expr { ... }`
    While { cond: Expr, body: Block },
    /// `match expr { pat => { ... }, ... }`
    Match { expr: Expr, arms: Vec<MatchArm> },
    /// Expression used as a statement (function call, method call, field assignment).
    Expr(Expr),
}

pub struct MatchArm {
    pub pat: Pat,
    pub body: Block,
}

/// Match patterns.
pub enum Pat {
    Wildcard,
    Bool(bool),
    Int(i64),
    EnumVariant {
        type_name: String,
        variant: String,
        bindings: PatBindings,
    },
}

/// Bindings extracted in a match arm.
pub enum PatBindings {
    None,                         // unit variant: no bindings
    Tuple(Vec<String>),           // Variant(a, b, _)
    Named(Vec<(String, String)>), // Variant { field: binding, ... } or shorthand { x, y }
}

pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(String),
    Var(String),
    /// `[expr, expr, ...]` — array literal
    ArrayLit(Vec<Expr>),
    /// `expr[index]`
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
    },
    /// `(expr0, expr1, ...)`
    Tuple(Vec<Expr>),
    /// `Type { field: expr, ... }` — struct literal
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// `Type::Variant { field: expr, ... }` — struct-like enum variant literal
    EnumStructLit {
        type_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
    /// `expr.field` or `expr.0` (tuple index via numeric field name "0", "1", ...)
    Field {
        expr: Box<Expr>,
        field: String,
    },
    /// Free function call: `name(args)`
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// Associated function or enum variant: `Type::name(args)` or `Type::Variant`
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
    /// `&expr` — address-of
    AddrOf {
        mutable: bool,
        expr: Box<Expr>,
    },
    /// `*expr` — raw pointer / reference dereference
    Deref(Box<Expr>),
    /// `expr as Ty` — type cast
    Cast {
        expr: Box<Expr>,
        ty: Ty,
    },
    /// `unsafe { stmts }` — unsafe block
    Unsafe(Block),
}

#[derive(Clone, Copy)]
pub enum UnOp {
    Neg,    // -x
    Not,    // !x  logical NOT
    BitNot, // ~x  bitwise NOT
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
