// out: 7
// out: 0

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let a: Opt = Opt::Some(7);
    let val = match a {
        Opt::Some(n) => n,
        Opt::None => 0,
    };
    println!("{}", val);

    let b: Opt = Opt::None;
    let val2 = match b {
        Opt::Some(n) => n,
        Opt::None => 0,
    };
    println!("{}", val2);
}
