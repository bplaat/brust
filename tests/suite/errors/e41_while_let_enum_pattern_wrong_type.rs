// err: pattern `Opt::Some` used on `i64`

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let n: i64 = 0;
    while let Opt::Some(x) = n {
        println!("{}", x);
    }
}
