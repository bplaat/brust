// out: negative
// out: zero
// out: small positive
// out: large positive
// out: even
// out: odd
// out: even

fn main() {
    // Basic match guard
    let nums: [i64; 4] = [-5, 0, 3, 100];
    let mut i: i64 = 0;
    while i < 4 {
        let n: i64 = nums[i as usize];
        match n {
            x if x < 0 => println!("negative"),
            x if x == 0 => println!("zero"),
            x if x < 10 => println!("small positive"),
            _ => println!("large positive"),
        }
        i += 1;
    }

    // Guard with binding
    let values: [i64; 3] = [4, 7, 8];
    let mut j: i64 = 0;
    while j < 3 {
        let v: i64 = values[j as usize];
        match v {
            x if x % 2 == 0 => println!("even"),
            _ => println!("odd"),
        }
        j += 1;
    }
}
