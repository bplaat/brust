// out: 0 0
// out: 3 4
// out: 10 20
// Test self:: and super:: paths in expressions
mod geom {
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    pub fn origin() -> self::Point {
        self::Point { x: 0, y: 0 }
    }

    pub fn make(x: i32, y: i32) -> Point {
        self::Point { x: x, y: y }
    }

    pub mod transform {
        pub fn shift(dx: i32, dy: i32) -> super::Point {
            super::Point { x: dx, y: dy }
        }
    }
}

fn main() {
    let o: geom::Point = geom::origin();
    println!("{} {}", o.x, o.y);

    let p: geom::Point = geom::make(3, 4);
    println!("{} {}", p.x, p.y);

    let q: geom::Point = geom::transform::shift(10, 20);
    println!("{} {}", q.x, q.y);
}
