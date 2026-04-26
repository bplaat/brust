// out: 15
// out: 5
// out: 50
// out: 2
// out: 3
// out: 7
// out: 4
// out: 6
// out: 3
// out: 100

fn main() {
    let mut x: i64 = 10;
    x += 5;
    println!("{}", x); // 15

    x -= 10;
    println!("{}", x); // 5

    x *= 10;
    println!("{}", x); // 50

    x /= 25;
    println!("{}", x); // 2

    let mut b: i64 = 7;
    b &= 3;
    println!("{}", b); // 3

    b |= 4;
    println!("{}", b); // 7

    b ^= 3;
    println!("{}", b); // 4

    let mut s: i64 = 3;
    s <<= 1;
    println!("{}", s); // 6

    s >>= 1;
    println!("{}", s); // 3

    let mut prod: i64 = 10;
    prod *= 10;
    println!("{}", prod); // 100
}
