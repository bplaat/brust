// err: integer pattern on `bool`

fn main() {
    let flag: bool = true;
    while let 1 = flag {
        println!("loop");
    }
}
