// out: small
// out: small
// out: medium
// out: large
// out: yes
// out: no
// out: yes

fn classify(n: i64) -> i64 {
    match n {
        1 | 2 | 3 => 1,
        4 | 5 | 6 => 2,
        _ => 3,
    }
}

fn main() {
    // Or-patterns with integer literals
    let x: i64 = 2;
    match x {
        1 | 2 | 3 => println!("small"),
        4 | 5 | 6 => println!("medium"),
        _ => println!("large"),
    }

    let y: i64 = 3;
    match y {
        1 | 2 | 3 => println!("small"),
        4 | 5 | 6 => println!("medium"),
        _ => println!("large"),
    }

    let z: i64 = 5;
    match z {
        1 | 2 | 3 => println!("small"),
        4 | 5 | 6 => println!("medium"),
        _ => println!("large"),
    }

    let w: i64 = 9;
    match w {
        1 | 2 | 3 => println!("small"),
        4 | 5 | 6 => println!("medium"),
        _ => println!("large"),
    }

    // Or-patterns with bool
    let b: bool = true;
    match b {
        true | false => println!("yes"),
    }

    let r: i64 = classify(7);
    match r {
        1 | 2 => println!("yes"),
        _ => println!("no"),
    }

    let s: i64 = classify(1);
    match s {
        1 | 2 => println!("yes"),
        _ => println!("no"),
    }
}
