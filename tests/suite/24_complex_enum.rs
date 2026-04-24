// out: lit: 42
// out: add: 7
// out: mul: 12
// out: neg: -5
// out: nested: 9

enum Expr {
    Lit(i64),
    Add(i64, i64),
    Mul(i64, i64),
    Neg(i64),
}

fn eval(e: Expr) -> i64 {
    match e {
        Expr::Lit(n) => n,
        Expr::Add(a, b) => a + b,
        Expr::Mul(a, b) => a * b,
        Expr::Neg(n) => -n,
    }
}

fn main() {
    println!("lit: {}", eval(Expr::Lit(42)));
    println!("add: {}", eval(Expr::Add(3, 4)));
    println!("mul: {}", eval(Expr::Mul(3, 4)));
    println!("neg: {}", eval(Expr::Neg(5)));

    let a: i64 = eval(Expr::Add(4, 5));
    let b: i64 = eval(Expr::Lit(a));
    println!("nested: {}", b);
}
