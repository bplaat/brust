// out: true
// out: false
// out: true
// out: true
// out: false

enum Dir {
    North,
    South,
    East,
    West,
}

fn main() {
    let x: i64 = 5;
    println!("{}", matches!(x, 1 | 2 | 5));
    println!("{}", matches!(x, 10 | 20));

    let d = Dir::North;
    println!("{}", matches!(d, Dir::North | Dir::South));
    println!("{}", matches!(d, Dir::North));
    println!("{}", matches!(d, Dir::East));
}
