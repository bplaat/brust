// out: 42
// out: -1

enum Option2 {
    Some2(i64),
    None2,
}

fn main() {
    let a: Option2 = Option2::Some2(42);
    let b: Option2 = Option2::None2;

    let x: i64 = if let Option2::Some2(v) = a { v } else { 0 };
    let y: i64 = if let Option2::Some2(v) = b { v } else { -1 };

    println!("{}", x);
    println!("{}", y);
}
