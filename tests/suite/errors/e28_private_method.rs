// err: `inner_Obj_secret` is private
// Bug E: private impl method must not be callable from outside the module.

mod inner {
    pub struct Obj {
        pub x: i32,
    }
    impl Obj {
        fn secret(&self) -> i32 {
            self.x
        }
    }
}

fn main() {
    let o: inner::Obj = inner::Obj { x: 5 };
    println!("{}", o.secret());
}
