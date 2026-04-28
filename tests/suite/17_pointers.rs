// out: 42
// out: 100
// out: 255

fn main() {
    let mut x: i32 = 42;
    let p: *mut i32 = &mut x as *mut i32;
    unsafe {
        println!("{}", *p);
        *p = 100;
        println!("{}", *p);
    }

    let byte: u8 = 255;
    let q: *const u8 = &byte as *const u8;
    unsafe {
        println!("{}", *q);
    }
}
