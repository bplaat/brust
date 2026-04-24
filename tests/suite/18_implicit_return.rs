// out: 5
// out: 9
// out: 7
// out: found

fn max(a: i64, b: i64) -> i64 {
    if a > b { a } else { b }
}

fn abs(n: i64) -> i64 {
    if n < 0 { -n } else { n }
}

fn find(arr: [i64; 5], target: i64) -> &str {
    if arr[0] == target {
        return "found";
    }
    if arr[1] == target {
        return "found";
    }
    if arr[2] == target {
        return "found";
    }
    if arr[3] == target {
        return "found";
    }
    if arr[4] == target {
        return "found";
    }
    "not found"
}

fn main() {
    println!("{}", max(3, 5)); // 5
    println!("{}", max(9, 2)); // 9
    println!("{}", abs(-7)); // 7

    let arr: [i64; 5] = [1, 2, 3, 4, 5];
    println!("{}", find(arr, 3));
}
