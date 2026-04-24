// err: missing field `y` in `Point`

struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p: Point = Point { x: 1 };
}
