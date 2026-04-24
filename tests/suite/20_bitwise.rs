// out: 12
// out: 14
// out: 6
// out: 40
// out: 6
// out: 0
// out: 15

fn main() {
    println!("{}", 12 & 14); // 8
    println!("{}", 12 | 6); // 14
    println!("{}", 12 ^ 10); // 6
    println!("{}", 5 << 3); // 40
    println!("{}", 48 >> 3); // 6
    println!("{}", 0 & 255); // 0
    let x: i32 = 10;
    let y: i32 = 5;
    println!("{}", x | y); // 15
}
