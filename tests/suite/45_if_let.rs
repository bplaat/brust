// out: got 42
// out: nothing
// out: x=10 y=20
// out: fallback
// out: true branch

enum Opt {
    Some(i64),
    None,
}

enum Point {
    Pos { x: i64, y: i64 },
}

fn main() {
    let a: Opt = Opt::Some(42);
    if let Opt::Some(v) = a {
        println!("got {}", v);
    } else {
        println!("nothing");
    }

    let b: Opt = Opt::None;
    if let Opt::Some(_v) = b {
        println!("got it");
    } else {
        println!("nothing");
    }

    let p: Point = Point::Pos { x: 10, y: 20 };
    if let Point::Pos { x, y } = p {
        println!("x={} y={}", x, y);
    }

    // if let with no else
    let c: Opt = Opt::None;
    if let Opt::Some(_x) = c {
        println!("some");
    } else {
        println!("fallback");
    }

    // if let with bool pattern
    let flag: bool = true;
    if let true = flag {
        println!("true branch");
    } else {
        println!("false branch");
    }
}
