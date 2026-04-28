// out: 42
// out: 100
// out: hello

const ANSWER: i64 = 42;
static LIMIT: i64 = 100;
const GREETING: &str = "hello";

fn main() {
    println!("{}", ANSWER);
    println!("{}", LIMIT);
    println!("{}", GREETING);
}
