// out: lowercase a
// out: other lowercase
// out: uppercase
// out: digit
// out: other

fn classify(c: char) -> &str {
    match c {
        'a' => "lowercase a",
        'b'..='z' => "other lowercase",
        'A'..='Z' => "uppercase",
        '0'..='9' => "digit",
        _ => "other",
    }
}

fn main() {
    println!("{}", classify('a'));
    println!("{}", classify('m'));
    println!("{}", classify('B'));
    println!("{}", classify('7'));
    println!("{}", classify('!'));
}
