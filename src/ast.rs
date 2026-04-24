/// A complete source file / compilation unit.
pub struct File {
    pub items: Vec<Item>,
}

pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Impl(ImplBlock),
}

pub struct StructDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

pub struct FieldDecl {
    pub name: String,
    pub ty: Ty,
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
pub enum Ty {
    // Signed integers
    I8, I16, I32, I64, Isize,
    // Unsigned integers
    U8, U16, U32, U64, Usize,
    Bool,
    Unit,          // ()
    Named(String), // user-defined struct type
}

pub struct Block {
    pub stmts: Vec<Stmt>,
}

pub enum Stmt {
    /// `println!("format", args...);`
    Println { format: String, args: Vec<Expr> },
    /// `let [mut] name [: ty] = expr;`
    Let { name: String, mutable: bool, ty: Option<Ty>, expr: Expr },
    /// `name = expr;`
    Assign { name: String, expr: Expr },
    /// `return [expr];`
    Return(Option<Expr>),
    /// `if expr { ... } [else { ... }]`
    If { cond: Expr, then_block: Block, else_block: Option<Block> },
    /// `while expr { ... }`
    While { cond: Expr, body: Block },
    /// Expression used as a statement, e.g. a function call.
    Expr(Expr),
}

pub enum Expr {
    Int(i64),
    Bool(bool),
    Var(String),
    /// `Type { field: expr, ... }`
    StructLit { name: String, fields: Vec<(String, Expr)> },
    /// `expr.field`
    Field { expr: Box<Expr>, field: String },
    /// Free function or associated call: `name(args)` or `Type::name(args)`
    Call { name: String, args: Vec<Expr> },
    /// Associated function: `Type::method(args)`
    AssocCall { type_name: String, method: String, args: Vec<Expr> },
    /// Method call: `expr.method(args)`
    MethodCall { expr: Box<Expr>, method: String, args: Vec<Expr> },
    UnOp { op: UnOp, operand: Box<Expr> },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
}

#[derive(Clone, Copy)]
pub enum UnOp {
    Neg,    // -x
    Not,    // !x  logical NOT (bool) / bitwise NOT (int, same as ~)
    BitNot, // ~x  explicit bitwise NOT
}

#[derive(Clone, Copy)]
pub enum BinOp {
    // Arithmetic
    Add, Sub, Mul, Div, Rem,
    // Bitwise
    BitAnd, BitOr, BitXor, Shl, Shr,
    // Comparison
    Eq, Ne, Lt, Gt, Le, Ge,
    // Logical
    And, Or,
}
