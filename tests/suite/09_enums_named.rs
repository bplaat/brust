// out: 5
// out: 7
// out: -10
// out: 5

enum Expr {
    Lit(i32),
    Add { lhs: i32, rhs: i32 },
    Neg { val: i32 },
}

fn eval(e: Expr) -> i32 {
    match e {
        Expr::Lit(n) => n,
        Expr::Add { lhs, rhs } => lhs + rhs,
        Expr::Neg { val } => -val,
    }
}

fn main() {
    println!("{}", eval(Expr::Lit(5)));
    println!("{}", eval(Expr::Add { lhs: 3, rhs: 4 }));
    println!("{}", eval(Expr::Neg { val: 10 }));
    println!(
        "{}",
        eval(Expr::Add {
            lhs: eval(Expr::Lit(2)),
            rhs: eval(Expr::Lit(3))
        })
    );
}
