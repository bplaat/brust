// Showcase: 2D vector math -- modules, structs, methods, and f32 arithmetic.
//
// Demonstrates:
//   - Module namespacing (`mod vec2d { ... }`)
//   - Struct definitions with float fields
//   - Using bare type names inside the module (Vec2 instead of vec2d::Vec2)
//   - Module-qualified calls from outside

mod vec2d {
    pub struct Vec2 {
        pub x: f32,
        pub y: f32,
    }

    pub fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x: x, y: y }
    }

    pub fn add(a: Vec2, b: Vec2) -> Vec2 {
        Vec2 { x: a.x + b.x, y: a.y + b.y }
    }

    pub fn scale(v: Vec2, s: f32) -> Vec2 {
        Vec2 { x: v.x * s, y: v.y * s }
    }

    pub fn dot(a: Vec2, b: Vec2) -> f32 {
        a.x * b.x + a.y * b.y
    }

    pub fn length_sq(v: Vec2) -> f32 {
        v.x * v.x + v.y * v.y
    }
}

fn main() {
    let a: vec2d::Vec2 = vec2d::new(3.0, 4.0);
    let b: vec2d::Vec2 = vec2d::new(1.0, 2.0);

    let c: vec2d::Vec2 = vec2d::add(a, b);
    let cx: f32 = c.x;
    let cy: f32 = c.y;
    println!("add: ({}, {})", cx, cy);

    let d: vec2d::Vec2 = vec2d::new(3.0, 4.0);
    let e: vec2d::Vec2 = vec2d::scale(d, 2.0);
    let ex: f32 = e.x;
    let ey: f32 = e.y;
    println!("scale: ({}, {})", ex, ey);

    let u: vec2d::Vec2 = vec2d::new(1.0, 0.0);
    let v: vec2d::Vec2 = vec2d::new(0.0, 1.0);
    println!("dot: {}", vec2d::dot(u, v));

    let w: vec2d::Vec2 = vec2d::new(3.0, 4.0);
    println!("length_sq: {}", vec2d::length_sq(w));
}
