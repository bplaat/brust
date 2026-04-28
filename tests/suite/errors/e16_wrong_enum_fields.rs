// err: missing field

enum Shape {
    Rect { w: f32, h: f32 },
}

fn main() {
    let s: Shape = Shape::Rect { w: 1.0 };
}
