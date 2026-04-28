// out: 1
// out: 1
// out: 2
// out: 3
// out: 5
// out: 8
// out: 120

fn fib(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    if n == 1 {
        return 1;
    }
    fib(n - 1) + fib(n - 2)
}

fn fact(n: i32) -> i32 {
    if n <= 1 {
        return 1;
    }
    n * fact(n - 1)
}

fn main() {
    println!("{}", fib(1));
    println!("{}", fib(2));
    println!("{}", fib(3));
    println!("{}", fib(4));
    println!("{}", fib(5));
    println!("{}", fib(6));
    println!("{}", fact(5));
}
