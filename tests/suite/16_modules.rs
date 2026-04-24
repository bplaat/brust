// out: x=4.000000 y=6.000000
// out: 52.000000
// out: 5.000000

mod math {
    pub struct Vec2 {
        pub x: f32,
        pub y: f32,
    }

    pub fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x: x, y: y }
    }

    pub fn add(a: Vec2, b: Vec2) -> Vec2 {
        Vec2 {
            x: a.x + b.x,
            y: a.y + b.y,
        }
    }

    pub fn length_sq(v: Vec2) -> f32 {
        v.x * v.x + v.y * v.y
    }

    pub fn scale_x(v: Vec2, s: f32) -> f32 {
        v.x * s
    }
}

fn main() {
    let a: math::Vec2 = math::new(1.0, 2.0);
    let b: math::Vec2 = math::new(3.0, 4.0);
    let c: math::Vec2 = math::add(a, b);
    let cx: f32 = c.x;
    let cy: f32 = c.y;
    println!("x={} y={}", cx, cy);

    let d: math::Vec2 = math::new(4.0, 6.0);
    println!("{}", math::length_sq(d));

    let e: math::Vec2 = math::new(1.0, 0.0);
    println!("{}", math::scale_x(e, 5.0));
}
