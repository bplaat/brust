// err: bool pattern on `i64`

fn main() {
    let mut x: i64 = 0;
    while let true = x {
        x = x + 1;
    }
}
