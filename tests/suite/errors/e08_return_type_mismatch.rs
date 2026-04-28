// err: expected `i32`, found `bool`

fn answer() -> i32 {
    true
}

fn main() {
    println!("{}", answer());
}
