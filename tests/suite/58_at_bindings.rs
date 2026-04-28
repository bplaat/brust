// out: small: 3
// out: medium: 8
// out: large: 42

fn describe(n: i64) {
    match n {
        v @ 1..=5 => {
            println!("small: {}", v);
        }
        v @ 6..=10 => {
            println!("medium: {}", v);
        }
        v => {
            println!("large: {}", v);
        }
    }
}

fn main() {
    describe(3);
    describe(8);
    describe(42);
}
