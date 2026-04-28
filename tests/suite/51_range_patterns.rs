// out: small
// out: medium
// out: large

fn classify(n: i64) -> &str {
    match n {
        1..=5 => "small",
        6..=10 => "medium",
        _ => "large",
    }
}

fn main() {
    println!("{}", classify(3));
    println!("{}", classify(7));
    println!("{}", classify(99));
}
