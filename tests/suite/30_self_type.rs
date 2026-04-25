// out: Point { x: 3, y: 7 }
// out: 3
// out: 7

trait Clone {
    fn clone(&self) -> Self;
}

struct Point {
    pub x: i32,
    pub y: i32,
}

impl Clone for Point {
    fn clone(&self) -> Self {
        Point { x: self.x, y: self.y }
    }
}

fn main() {
    let p: Point = Point { x: 3, y: 7 };
    let q: Point = p.clone();
    println!("Point { x: {}, y: {} }", q.x, q.y);
    println!("{}", q.x);
    println!("{}", q.y);
}
