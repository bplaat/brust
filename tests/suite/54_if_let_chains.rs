// out: positive: 5
// out: skipped

enum Opt {
    Some(i64),
    None,
}

fn main() {
    let a: Opt = Opt::Some(5);
    if let Opt::Some(x) = a && x > 0 {
        println!("positive: {}", x);
    } else {
        println!("skipped");
    }

    let b: Opt = Opt::Some(-1);
    if let Opt::Some(x) = b && x > 0 {
        println!("positive: {}", x);
    } else {
        println!("skipped");
    }
}
