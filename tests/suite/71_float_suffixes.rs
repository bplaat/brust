// out: 0.100000
// out: 0.100000
// out: 5.000000
// out: 12.000000
// out: 3.140000

fn main() {
    // Float literals with f64 suffix
    let a: f64 = 0.1f64;
    println!("{}", a);

    // Float literals with f32 suffix
    let b: f32 = 0.1f32;
    println!("{}", b);

    // Integer form with float suffix (5f32 is a float literal)
    let c: f32 = 5f32;
    println!("{}", c);

    // Float with exponent and suffix
    let d: f64 = 12E+0_f64;
    println!("{}", d);

    // Float suffix underscore separator
    let e: f64 = 3.14_f64;
    println!("{}", e);
}
