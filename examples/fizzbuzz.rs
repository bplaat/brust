// Showcase: FizzBuzz — classic loop with branching.

fn classify(n: i64) {
    if n % 15 == 0 {
        println!("FizzBuzz");
    } else if n % 3 == 0 {
        println!("Fizz");
    } else if n % 5 == 0 {
        println!("Buzz");
    } else {
        println!("{}", n);
    }
}

fn main() {
    let mut i: i64 = 1;
    while i <= 20 {
        classify(i);
        i = i + 1;
    }
}
