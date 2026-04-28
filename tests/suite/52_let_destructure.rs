// out: 1 2
// out: 3 4 5

fn main() {
    let (a, b) = (1_i64, 2_i64);
    println!("{} {}", a, b);

    let (x, y, z) = (3_i64, 4_i64, 5_i64);
    println!("{} {} {}", x, y, z);
}
