// err: unicode escapes are not allowed in byte literals

fn main() {
    let _x: u8 = b'\u{100}';
}
