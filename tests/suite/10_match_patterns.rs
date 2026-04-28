// out: zero
// out: one
// out: small
// out: other

fn classify(n: i64) -> &str {
    match n {
        0 => "zero",
        1 => "one",
        2 => "small",
        _ => "other",
    }
}

fn main() {
    println!("{}", classify(0));
    println!("{}", classify(1));
    println!("{}", classify(2));
    println!("{}", classify(99));
}
