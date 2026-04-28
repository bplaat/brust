// out: 0
// out: 42
// out: 1000000
// out: 255
// out: 255
// out: 3735928559
// out: 26
// out: 10
// out: 170
// out: 15
// out: 9223372036854775807
// out: 9223372036854775807
// out: 42
// out: 1000
// out: 255

fn main() {
    // Decimal literals
    println!("{}", 0);
    println!("{}", 42);
    println!("{}", 1_000_000);

    // Hex literals -- lowercase and uppercase prefix, underscores
    println!("{}", 0xff);
    println!("{}", 0xFF);
    println!("{}", 0xDEAD_BEEF);
    println!("{}", 0X1A);

    // Binary literals -- lowercase and uppercase prefix, underscores
    println!("{}", 0b1010);
    println!("{}", 0b1010_1010);
    println!("{}", 0B1111);

    // Boundary values
    println!("{}", 9_223_372_036_854_775_807);       // i64::MAX decimal
    println!("{}", 0x7FFF_FFFF_FFFF_FFFF);           // i64::MAX hex

    // Integer type suffixes: consumed without changing the value
    let a: u8 = 42u8;
    println!("{}", a);
    let b: i32 = 1_000i32;
    println!("{}", b);
    let c: u64 = 0xFFu64;
    println!("{}", c);
}
