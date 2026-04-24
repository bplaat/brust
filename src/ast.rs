/// A complete source file / compilation unit.
pub struct File {
    pub items: Vec<Item>,
}

pub enum Item {
    Fn(FnDecl),
}

pub struct FnDecl {
    pub name: String,
    pub body: Block,
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
}

pub enum Expr {
    Int(i64),
    Var(String),
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
}

#[derive(Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
