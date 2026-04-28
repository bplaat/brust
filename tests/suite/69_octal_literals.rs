// out: 8
// out: 63
// out: 15
// out: 63
// out: 342391
// out: 255

fn main() {
    // Octal literals -- lowercase and uppercase prefix, underscores
    println!("{}", 0o10);
    println!("{}", 0o77);
    println!("{}", 0O17);
    println!("{}", 0o7_7);

    // Larger octal value
    println!("{}", 0o1234567);

    // Octal with integer type suffix
    let a: u8 = 0o377u8;
    println!("{}", a);
}
