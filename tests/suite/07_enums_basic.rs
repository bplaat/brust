// out: north
// out: south
// out: east
// out: west
// out: unknown

enum Dir {
    North,
    South,
    East,
    West,
}

fn name(d: Dir) -> &str {
    match d {
        Dir::North => "north",
        Dir::South => "south",
        Dir::East => "east",
        Dir::West => "west",
    }
}

fn opposite(d: Dir) -> Dir {
    match d {
        Dir::North => Dir::South,
        Dir::South => Dir::North,
        Dir::East => Dir::West,
        Dir::West => Dir::East,
    }
}

fn main() {
    println!("{}", name(Dir::North));
    println!("{}", name(opposite(Dir::North)));
    println!("{}", name(Dir::East));
    println!("{}", name(opposite(Dir::East)));

    let d: Dir = Dir::North;
    match d {
        Dir::North => println!("unknown"),
        _ => println!("known"),
    }
}
