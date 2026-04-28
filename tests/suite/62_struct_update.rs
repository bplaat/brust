// out: 10 2 3
// out: 100 200 3

struct Point {
    x: i64,
    y: i64,
    z: i64,
}

fn main() {
    let base: Point = Point { x: 1, y: 2, z: 3 };
    let p: Point = Point { x: 10, ..base };
    println!("{} {} {}", p.x, p.y, p.z);

    let q: Point = Point { x: 100, y: 200, ..base };
    println!("{} {} {}", q.x, q.y, q.z);
}
