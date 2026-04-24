// err: cannot take `&mut` of immutable

fn bump(p: &mut i32) {
    *p = *p + 1;
}

fn main() {
    let x: i32 = 5;
    bump(&mut x);
}
