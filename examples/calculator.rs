// Showcase: expression calculator — enums, pattern matching, and recursion.
//
// Demonstrates:
//   - Named and tuple enum variants
//   - Recursive pattern matching via eval
//   - Implicit tail-expression return

enum Expr {
    Num(i64),
    Add { lhs: i64, rhs: i64 },
    Sub { lhs: i64, rhs: i64 },
    Mul { lhs: i64, rhs: i64 },
    Neg { val: i64 },
}

fn eval(e: Expr) -> i64 {
    match e {
        Expr::Num(n)        => n,
        Expr::Add { lhs, rhs } => lhs + rhs,
        Expr::Sub { lhs, rhs } => lhs - rhs,
        Expr::Mul { lhs, rhs } => lhs * rhs,
        Expr::Neg { val }   => -val,
    }
}

fn kind(e: Expr) -> &str {
    match e {
        Expr::Num(_)        => "num",
        Expr::Add { lhs: _, rhs: _ } => "add",
        Expr::Sub { lhs: _, rhs: _ } => "sub",
        Expr::Mul { lhs: _, rhs: _ } => "mul",
        Expr::Neg { val: _ } => "neg",
    }
}

fn main() {
    println!("{}", eval(Expr::Num(42)));
    println!("{}", eval(Expr::Add { lhs: 10, rhs: 32 }));
    println!("{}", eval(Expr::Sub { lhs: 100, rhs: 58 }));
    println!("{}", eval(Expr::Mul { lhs: 6, rhs: 7 }));
    println!("{}", eval(Expr::Neg { val: 99 }));

    println!("{}", kind(Expr::Num(0)));
    println!("{}", kind(Expr::Add { lhs: 1, rhs: 2 }));
}
