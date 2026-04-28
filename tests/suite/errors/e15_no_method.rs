// err: no method `fly` on

struct Bird {
    name: i32,
}

fn main() {
    let b: Bird = Bird { name: 1 };
    b.fly();
}
