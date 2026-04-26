// out: hello
// out: hello world

fn main() {
    print!("hello");
    println!("");
    print!("hello ");
    println!("world");
    eprintln!("this goes to stderr");
    eprint!("also stderr: ");
    eprintln!("{}", 42);
}
