// err: `extern "C"` block must be declared `unsafe extern "C"` to acknowledge that calling C functions is unsafe

extern "C" {
    fn strlen(s: *const u8) -> usize;
}

fn main() {
    let _len: usize = unsafe { strlen("hello\0" as *const u8) };
}
