// out: negative
// out: zero
// out: positive
// out: 10
// out: two
// out: 3
// out: 30
// out: 2
// out: -1
// out: 0
// out: 1
// out: 1
// out: 30
// out: 42
// out: 6
// out: all expression-as-value tests passed

fn classify(n: i64) -> &str {
    if n < 0 {
        "negative"
    } else if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

fn abs(n: i64) -> i64 {
    if n < 0 { -n } else { n }
}

fn sign(n: i64) -> i64 {
    match n {
        0 => 0,
        _ => {
            if n > 0 {
                1
            } else {
                -1
            }
        }
    }
}

fn main() {
    // if as let binding
    let x: i64 = if true { 42 } else { 0 };
    // x == 42 verified below via classify

    // if-else chain as let binding
    let label1 = classify(-5);
    println!("{}", label1);
    let label2 = classify(0);
    println!("{}", label2);
    let label3 = classify(7);
    println!("{}", label3);

    // if as function argument
    let result = abs(if true { -10 } else { 10 });
    println!("{}", result);

    // match as let binding
    let n: i64 = 2;
    let name: &str = match n {
        1 => "one",
        2 => "two",
        _ => "other",
    };
    println!("{}", name);

    // block as let binding (single expr)
    let y: i64 = { 1 + 2 };
    println!("{}", y);

    // block with multiple stmts
    let z: i64 = {
        let a: i64 = 10;
        let b: i64 = 20;
        a + b
    };
    println!("{}", z);

    // nested if expression
    let v: i64 = if true { if false { 1 } else { 2 } } else { 3 };
    println!("{}", v);

    // match with computed scrutinee as expression
    let s1 = sign(-5);
    println!("{}", s1);
    let s2 = sign(0);
    println!("{}", s2);
    let s3 = sign(3);
    println!("{}", s3);

    // block with if as tail statement (tests infer_block_value_ty for StmtKind::If)
    let bi: i64 = { if true { 1 } else { 2 } };
    println!("{}", bi);

    // block with match as tail statement (tests infer_block_value_ty for StmtKind::Match)
    let bm: i64 = {
        let n2: i64 = 3;
        match n2 {
            1 => 10,
            2 => 20,
            _ => 30,
        }
    };
    println!("{}", bm);

    // if as function arg (x is 42, so abs(42) = 42)
    let fa: i64 = abs(if x > 0 { 42 } else { -42 });
    println!("{}", fa);

    // multi-stmt block computing value (3*3 + 4*4 - 19 = 6)
    let pyth: i64 = {
        let a2: i64 = 3;
        let b2: i64 = 4;
        a2 * a2 + b2 * b2 - 19
    };
    println!("{}", pyth);

    println!("all expression-as-value tests passed");
}
