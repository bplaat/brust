/// A complete source file / compilation unit.
pub struct File {
    pub items: Vec<Item>,
}

pub enum Item {
    Fn(FnDecl),
}

pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Ty,
    pub body: Block,
}

pub struct Param {
    pub name: String,
    pub ty: Ty,
}

/// Types supported so far.
pub enum Ty {
    I32,
    I64,
    Bool,
    Unit, // ()
}

pub struct Block {
    pub stmts: Vec<Stmt>,
}

pub enum Stmt {
    /// `println!("format", args...);`
    Println { format: String, args: Vec<Expr> },
    /// `let [mut] name = expr;`
    Let { name: String, mutable: bool, expr: Expr },
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
    Call { name: String, args: Vec<Expr> },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
}

#[derive(Clone, Copy)]
pub enum BinOp {
    // Arithmetic
    Add, Sub, Mul, Div, Rem,
    // Comparison
    Eq, Ne, Lt, Gt, Le, Ge,
    // Logical
    And, Or,
}
