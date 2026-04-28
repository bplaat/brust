// err: no field `z` in `Shape::Rect`

enum Shape {
    Rect { x: i64, y: i64 },
}

fn main() {
    let s: Shape = Shape::Rect { x: 1, y: 2 };
    while let Shape::Rect { x, z } = s {
        println!("{} {}", x, z);
    }
}
