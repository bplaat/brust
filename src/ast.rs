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
}

pub enum Expr {
    Int(i64),
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
