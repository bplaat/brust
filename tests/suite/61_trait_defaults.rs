// out: Hello, Alice!
// out: Hola, Carlos!

trait Greeter {
    fn name(&self) -> &str;

    fn greet(&self) {
        println!("Hello, {}!", self.name());
    }
}

struct English {
    pub name: &str,
}

struct Spanish {
    pub name: &str,
}

impl Greeter for English {
    fn name(&self) -> &str {
        self.name
    }
}

impl Greeter for Spanish {
    fn name(&self) -> &str {
        self.name
    }

    fn greet(&self) {
        println!("Hola, {}!", self.name());
    }
}

fn do_greet(g: dyn Greeter) {
    g.greet();
}

fn main() {
    let e: English = English { name: "Alice" };
    let s: Spanish = Spanish { name: "Carlos" };
    do_greet(&e as dyn Greeter);
    do_greet(&s as dyn Greeter);
}
