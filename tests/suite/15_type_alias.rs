// out: 7.300000
// out: 21
// out: hello alias

type Meters = f64;
type Count = i32;
type Message = &str;

fn dist(a: Meters, b: Meters) -> Meters {
    a + b
}

fn times(n: Count, x: Count) -> Count {
    n * x
}

fn greet() -> Message {
    "hello alias"
}

fn main() {
    let d: Meters = dist(3.1, 4.2);
    println!("{}", d);

    let c: Count = times(3, 7);
    println!("{}", c);

    println!("{}", greet());
}
