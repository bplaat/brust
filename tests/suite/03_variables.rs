// out: 5
// out: 10
// out: 41
// out: hello
// out: world

fn main() {
    let x = 5;
    println!("{}", x);

    let mut y = x;
    y = y * 2;
    println!("{}", y);

    let mut z: i64 = x * y - 1;
    z = z - 8;
    println!("{}", z);

    let s: &str = "hello";
    println!("{}", s);

    let t: &str = "world";
    println!("{}", t);
}
