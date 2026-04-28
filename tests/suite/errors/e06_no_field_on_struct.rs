// err: no field `z` on `Point`

struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p: Point = Point { x: 1, y: 2 };
    println!("{}", p.z);
}
