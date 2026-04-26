// err: if/else branch type mismatch

fn main() {
    let _x: i64 = if true { 1 } else { "hello" };
}
