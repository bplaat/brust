// out: 10
// out: 16
// out: 9
// out: 30

fn apply(f: fn(i32) -> i32, x: i32) -> i32 {
    f(x)
}

fn dbl(x: i32) -> i32 {
    x * 2
}

fn square(x: i32) -> i32 {
    x * x
}

fn add_then_print(a: i32, b: i32, print: fn(i32)) {
    print(a + b);
}

fn print_i32(x: i32) {
    println!("{}", x);
}

fn main() {
    println!("{}", apply(dbl, 5)); // 10
    println!("{}", apply(square, 4)); // 16

    let f: fn(i32) -> i32 = square;
    println!("{}", apply(f, 3)); // 9

    add_then_print(10, 20, print_i32); // 30
}
