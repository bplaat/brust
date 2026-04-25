// out: 100
// out: 200

mod shapes {
    pub struct Rect {
        pub width: i32,
        pub height: i32,
    }

    impl Rect {
        pub fn area(&self) -> i32 {
            self.width * self.height
        }
    }
}

use shapes::Rect;

fn main() {
    let r: Rect = Rect { width: 10, height: 10 };
    println!("{}", r.area());
    let r2: Rect = Rect { width: 10, height: 20 };
    println!("{}", r2.area());
}
