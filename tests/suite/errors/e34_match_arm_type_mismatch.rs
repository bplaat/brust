// err: match arms have incompatible types

fn main() {
    let n: i64 = 1;
    let _x: i64 = match n {
        1 => 42,
        _ => "hello",
    };
}
