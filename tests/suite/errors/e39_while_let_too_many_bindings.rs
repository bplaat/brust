// err: too many bindings for `Opt::Some`

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let o: Opt = Opt::Some(42);
    while let Opt::Some(x, y) = o {
        println!("{} {}", x, y);
    }
}
