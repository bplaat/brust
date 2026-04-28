// err: byte literal value must be in the range 0..=255

fn main() {
    let _x: u32 = b'\u{100}';
}
