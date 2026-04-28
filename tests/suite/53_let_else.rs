// out: got 5
// out: none case

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let a: Opt = Opt::Some(5);
    let Opt::Some(x) = a else {
        println!("none case");
        return;
    };
    println!("got {}", x);

    let b: Opt = Opt::None;
    let Opt::Some(_y) = b else {
        println!("none case");
        return;
    };
    println!("unreachable");
}
