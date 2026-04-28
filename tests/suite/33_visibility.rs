// out: 5
// out: 42
// Test visibility: pub items are accessible from outside the module
mod shapes {
    pub struct Circle {
        pub radius: i32,
    }

    pub fn make_circle(r: i32) -> Circle {
        Circle { radius: r }
    }

    fn private_helper() -> i32 {
        42
    }

    pub fn get_answer() -> i32 {
        private_helper()
    }
}

use shapes::Circle;

fn main() {
    let c: Circle = shapes::make_circle(5);
    println!("{}", c.radius);
    println!("{}", shapes::get_answer());
}
