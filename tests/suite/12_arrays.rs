// out: 10
// out: 20
// out: 30
// out: 60

fn sum3(a: i32, b: i32, c: i32) -> i32 {
    a + b + c
}

fn main() {
    let arr: [i32; 3] = [10, 20, 30];
    println!("{}", arr[0]);
    println!("{}", arr[1]);
    println!("{}", arr[2]);
    println!("{}", sum3(arr[0], arr[1], arr[2]));
}
