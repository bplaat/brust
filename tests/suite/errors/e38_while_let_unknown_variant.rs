// err: no variant `Missing` in enum `Opt`

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let o: Opt = Opt::None;
    while let Opt::Missing(x) = o {
        println!("{}", x);
    }
}
