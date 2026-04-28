// out: 65
// out: 104
// out: 72 101 108 108 111

fn main() {
    let a: u8 = b'A';
    println!("{}", a);

    let h: u8 = b'h';
    println!("{}", h);

    let bytes: [u8; 5] = [b'H', b'e', b'l', b'l', b'o'];
    println!("{} {} {} {} {}", bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]);
}
