// err: this form of character escape may only be used with characters in the range [\x00-\x7f]

fn main() {
    let _: char = '\x80';
}
