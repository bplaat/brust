// out: 42
// out: hello
// out: 3
// out: 42 hello

fn main() {
    let t: (i32, &str) = (42, "hello");
    let n: i32 = t.0;
    let s: &str = t.1;
    println!("{}", n);
    println!("{}", s);

    let pair: (i32, i32) = (1, 2);
    println!("{}", pair.0 + pair.1);

    println!("{} {}", n, s);
}
