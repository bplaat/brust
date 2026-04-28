// out: 3
// out: 7
// out: yes

struct Point(i64, i64);
struct Color(i64, i64, i64);

fn sum(p: Point) -> i64 {
    p.0 + p.1
}

fn main() {
    let p = Point(1, 2);
    println!("{}", sum(p));

    let c = Color(1, 2, 4);
    println!("{}", c.0 + c.1 + c.2);

    let q = Point(3, 4);
    match q {
        Point(x, y) if x < y => println!("yes"),
        _ => println!("no"),
    }
}
