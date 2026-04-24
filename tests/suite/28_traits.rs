// out: Rex says woof
// out: Whiskers says meow
// out: 3

trait Animal {
    fn speak(&self) -> &str;
    fn name(&self) -> &str;
}

trait Counter {
    fn count(&self) -> i32;
}

struct Dog {
    pub name: &str,
}

struct Cat {
    pub name: &str,
}

struct Pack {
    pub size: i32,
}

impl Animal for Dog {
    fn speak(&self) -> &str {
        "woof"
    }
    fn name(&self) -> &str {
        self.name
    }
}

impl Animal for Cat {
    fn speak(&self) -> &str {
        "meow"
    }
    fn name(&self) -> &str {
        self.name
    }
}

impl Counter for Pack {
    fn count(&self) -> i32 {
        self.size
    }
}

fn greet(a: dyn Animal) {
    println!("{} says {}", a.name(), a.speak());
}

fn main() {
    let dog: Dog = Dog { name: "Rex" };
    let cat: Cat = Cat { name: "Whiskers" };
    let pack: Pack = Pack { size: 3 };

    greet(&dog as dyn Animal);
    greet(&cat as dyn Animal);

    let c: dyn Counter = &pack as dyn Counter;
    println!("{}", c.count());
}
