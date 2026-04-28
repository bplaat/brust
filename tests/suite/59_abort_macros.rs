// out: 10
// out: 3

fn might_panic(x: i64) -> i64 {
    if x == 0 {
        panic!("x cannot be zero");
    }
    x * 2
}

fn unreachable_branch(x: i64) -> i64 {
    if x > 0 {
        x
    } else {
        unreachable!("should never be negative");
    }
}

fn main() {
    println!("{}", might_panic(5));
    println!("{}", unreachable_branch(3));
}
