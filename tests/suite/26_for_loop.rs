// out: 100
// out: 4
// out: 12

fn main() {
    let arr: [i32; 4] = [10, 20, 30, 40];
    let mut sum: i32 = 0;
    for x in arr {
        sum = sum + x;
    }
    println!("{}", sum);

    let mut count: i32 = 0;
    for _x in arr {
        count = count + 1;
    }
    println!("{}", count);

    let vals: [i32; 5] = [2, 4, 6, 8, 10];
    let mut evens: i32 = 0;
    for v in vals {
        if v % 4 == 0 {
            evens = evens + v;
        }
    }
    println!("{}", evens);
}
