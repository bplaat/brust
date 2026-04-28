// out: 7
// out: 9
// out: 8
// out: 2
// out: 2
// out: -3
// out: 100

fn main() {
    println!("{}", 1 + 2 * 3); // 7  (precedence)
    println!("{}", (1 + 2) * 3); // 9
    println!("{}", 10 - 4 / 2); // 8  (10 - 2)
    println!("{}", 17 % 5); // 2
    println!("{}", 10 / 4); // 2  (integer division truncates)
    println!("{}", -3); // -3 (unary negation)
    println!("{}", 10 * 10); // 100
}
