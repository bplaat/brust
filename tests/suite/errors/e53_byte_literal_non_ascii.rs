// err: non-ASCII characters are not allowed in byte literals

fn main() {
    let _: u8 = b'Ñ';
}
