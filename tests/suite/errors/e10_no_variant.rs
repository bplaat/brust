// err: unknown associated function or variant `Dir::West`

enum Dir {
    North,
    South,
    East,
}

fn main() {
    let d: Dir = Dir::West;
    println!("{}", 0);
}
