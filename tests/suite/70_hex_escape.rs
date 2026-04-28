// out: 82
// out: Rango
// out: 82
// out: hello

fn main() {
    // \x escape in char literal (7-bit, 0x00-0x7F)
    let a: char = '\x52';
    println!("{}", a);

    // \x escape in string literal
    let s: &str = "\x52\x61\x6E\x67\x6F";
    println!("{}", s);

    // \x escape in byte literal (8-bit, 0x00-0xFF)
    let b: u8 = b'\x52';
    println!("{}", b);

    // Multiple \x escapes in string
    let hello: &str = "\x68\x65\x6c\x6c\x6f";
    println!("{}", hello);
}
