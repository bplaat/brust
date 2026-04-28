// out: 1
// out: 2
// out: 3
// out: 10
// out: 30
// out: 40

fn sum_range(lo: i64, hi: i64) -> i64 {
    let mut acc: i64 = 0;
    for i in lo..hi {
        acc = acc + i;
    }
    acc
}

fn main() {
    // for i in lo..hi -- numeric range loop
    for i in 1..4 {
        println!("{}", i);
    }

    // sum 1..5 = 1+2+3+4 = 10
    println!("{}", sum_range(1, 5));

    // slice range indexing: arr[lo..hi] emits (arr + lo) in C
    let arr: [i64; 5] = [10, 20, 30, 40, 50];
    let slice: &[i64] = arr[2..4];
    // slice now points at element 2 (value 30); index [0] gives 30
    println!("{}", slice[0]);

    // zero-based range: arr[0..] is just arr
    let slice2: &[i64] = arr[0..];
    println!("{}", slice2[3]);
}
