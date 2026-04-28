// out: positive
// out: negative
// out: zero
// out: 1
// out: 2
// out: 3
// out: done

fn sign(n: i64) -> &str {
    if n > 0 {
        "positive"
    } else if n < 0 {
        "negative"
    } else {
        "zero"
    }
}

fn main() {
    println!("{}", sign(10));
    println!("{}", sign(-5));
    println!("{}", sign(0));

    let mut i: i64 = 1;
    while i <= 3 {
        println!("{}", i);
        i = i + 1;
    }

    println!("done");
}
