// err: call to unsafe extern fn `strlen` must be inside an `unsafe` block

unsafe extern "C" {
    fn strlen(s: *const u8) -> usize;
}

fn main() {
    let _ = strlen("hello\0" as *const u8);
}
