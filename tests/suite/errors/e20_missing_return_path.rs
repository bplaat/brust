// err: function `maybe` may not return a value

fn maybe(cond: bool) -> i32 {
    if cond {
        1
    }
}

fn main() {
    println!("{}", maybe(true));
}
