// out: 42
// out: 10
// out: done

fn find_first_even(arr: [i64; 5]) -> i64 {
    let mut i: i64 = 0;
    loop {
        if arr[i as usize] % 2 == 0 {
            break arr[i as usize];
        }
        i += 1;
    }
}

fn main() {
    // Simple loop expression
    let x: i64 = loop {
        break 42;
    };
    println!("{}", x);

    // Loop expression with computation
    let arr: [i64; 5] = [1, 3, 5, 10, 12];
    let first_even = find_first_even(arr);
    println!("{}", first_even);

    // Loop used as statement (no value captured)
    let mut count: i64 = 0;
    loop {
        count += 1;
        if count >= 3 {
            break;
        }
    }
    println!("done");
}
