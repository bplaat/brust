// out: (0, 0)
// out: (3, 4)
// out: length_sq: 25

struct Vec2 {
    x: i32,
    y: i32,
}

struct Line {
    start: Vec2,
    end: Vec2,
}

impl Vec2 {
    fn zero() -> Vec2 {
        Vec2 { x: 0, y: 0 }
    }

    fn new(x: i32, y: i32) -> Vec2 {
        Vec2 { x: x, y: y }
    }

    fn length_sq(&self) -> i32 {
        self.x * self.x + self.y * self.y
    }

    fn print(&self) {
        println!("({}, {})", self.x, self.y);
    }
}

fn main() {
    let origin: Vec2 = Vec2::zero();
    origin.print();

    let p: Vec2 = Vec2::new(3, 4);
    p.print();
    println!("length_sq: {}", p.length_sq());

    let s: Vec2 = Vec2::zero();
    let e: Vec2 = Vec2::new(1, 1);
    let _line: Line = Line { start: s, end: e };
}
