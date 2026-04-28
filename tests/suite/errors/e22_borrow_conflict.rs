// err: cannot borrow `x` as mutable: also borrowed as immutable

fn take(a: &i32, b: &mut i32) {}

fn main() {
    let mut x: i32 = 5;
    take(&x, &mut x);
}
