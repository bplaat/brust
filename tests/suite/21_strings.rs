// out: hello
// out: world
// out: Hello, brust!
// out: brust

fn greet(name: &str) -> &str {
    name
}

fn main() {
    let a: &str = "hello";
    let b: &str = "world";
    println!("{}", a);
    println!("{}", b);
    println!("Hello, brust!");
    println!("{}", greet("brust"));
}
