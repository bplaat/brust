// err: if/else branch type mismatch

fn main() {
    // if without else used as a non-unit value is a type mismatch
    let _x: i64 = if true { 1 };
}
