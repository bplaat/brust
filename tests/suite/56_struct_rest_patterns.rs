// out: x=1
// out: x=3 extra=4

struct Point {
    x: i64,
    y: i64,
    z: i64,
}

struct Named {
    x: i64,
    extra: i64,
}

fn main() {
    let p = Point { x: 1, y: 2, z: 3 };
    match p {
        Point { x, .. } => println!("x={}", x),
    }

    let n = Named { x: 3, extra: 4 };
    match n {
        Named { x, extra, .. } => println!("x={} extra={}", x, extra),
    }
}
