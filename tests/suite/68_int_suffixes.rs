// out: 3
// out: 4
// out: 255
// out: 100
// out: 65535
// out: 4294967295
// out: 9223372036854775807
// out: 0

fn main() {
    let a: u32 = 3u32;
    println!("{}", a);

    let b: u64 = 4u64;
    println!("{}", b);

    let c: u8 = 255u8;
    println!("{}", c);

    let d: i32 = 100i32;
    println!("{}", d);

    let e: u16 = 65535u16;
    println!("{}", e);

    let f: u32 = 4294967295u32;
    println!("{}", f);

    let g: i64 = 9223372036854775807i64;
    println!("{}", g);

    let h: usize = 0usize;
    println!("{}", h);
}
