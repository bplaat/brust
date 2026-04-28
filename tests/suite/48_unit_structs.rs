// out: Marker used
// out: done

struct Marker;
struct Empty;

fn use_marker(m: Marker) -> i64 {
    let _ = m;
    1
}

fn main() {
    let m = Marker;
    if use_marker(m) == 1 {
        println!("Marker used");
    }
    let _e = Empty;
    println!("done");
}
