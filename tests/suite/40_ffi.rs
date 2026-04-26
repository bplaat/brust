// out: 5
// out: 123
// out: ok

unsafe extern "C" {
    fn strlen(s: *const u8) -> usize;
    fn atoi(s: *const u8) -> i32;
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
}

fn main() {
    let len: usize = unsafe { strlen("hello\0" as *const u8) };
    println!("{}", len);

    let n: i32 = unsafe { atoi("123\0" as *const u8) };
    println!("{}", n);

    let p: *mut u8 = unsafe { malloc(64) };
    unsafe { free(p) };
    println!("ok");
}
