// out: (1, 2)
// out: (4, 6)
// out: 10

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Point {
        Point { x: x, y: y }
    }

    fn add(&self, other: Point) -> Point {
        Point {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    fn manhattan(&self) -> i32 {
        self.x + self.y
    }

    fn print(&self) {
        println!("({}, {})", self.x, self.y);
    }
}

fn main() {
    let a: Point = Point::new(1, 2);
    let b: Point = Point::new(3, 4);
    a.print();

    let c: Point = a.add(b);
    c.print();

    println!("{}", c.manhattan());
}
