// err: argument 1 of `greet`: expected `i32`, found `bool`

fn greet(n: i32) {
    println!("{}", n);
}

fn main() {
    greet(true);
}
