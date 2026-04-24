// out: 5
// out: 15
// out: done

fn main() {
    let mut n: i32 = 0;
    loop {
        n = n + 1;
        if n == 5 {
            break;
        }
    }
    println!("{}", n);

    let mut sum: i32 = 0;
    let mut i: i32 = 1;
    loop {
        if i > 5 {
            break;
        }
        sum = sum + i;
        i = i + 1;
    }
    println!("{}", sum);

    println!("done");
}
