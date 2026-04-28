// out: 5
// out: 3

fn main() {
    let mut evens: i32 = 0;
    let mut i: i32 = 0;
    while i < 10 {
        i = i + 1;
        if i % 2 != 0 {
            continue;
        }
        evens = evens + 1;
    }
    println!("{}", evens);

    let mut count: i32 = 0;
    let arr: [i32; 6] = [1, 2, 3, 4, 5, 6];
    for x in arr {
        if x % 2 == 0 {
            continue;
        }
        count = count + 1;
    }
    println!("{}", count);
}
