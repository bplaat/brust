// out: 3.141593
// out: 6.283186
// out: 78.539825

fn square(x: f64) -> f64 {
    x * x
}

fn circle_area(r: f64) -> f64 {
    let pi: f64 = 3.141593;
    pi * square(r)
}

fn main() {
    let pi: f64 = 3.141593;
    println!("{}", pi);
    println!("{}", pi * 2.0);
    println!("{}", circle_area(5.0));
}
