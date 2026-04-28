// out: 55
// out: 120
// out: 7
// out: 3628800

fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn fact(n: i64) -> i64 {
    if n <= 1 {
        return 1;
    }
    n * fact(n - 1)
}

fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {
    println!("{}", fib(10)); // 55
    println!("{}", fact(5)); // 120
    println!("{}", add(3, 4)); // 7
    println!("{}", fact(10)); // 3628800
}
