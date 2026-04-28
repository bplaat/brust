// err: `engine_Obj_internal` is private
// Private method in module A called from sibling module B (not top-level).
// Both modules are at the same nesting level; the call site's cur_mod ("tools_")
// does not match the method's item_module ("engine_"), so it must be rejected.

mod engine {
    pub struct Obj {
        pub v: i32,
    }
    impl Obj {
        fn internal(&self) -> i32 {
            self.v * 2
        }
        pub fn run(&self) -> i32 {
            self.internal()
        }
    }
}

mod tools {
    pub fn probe(o: engine::Obj) -> i32 {
        o.internal()
    }
}

fn main() {
    let o: engine::Obj = engine::Obj { v: 5 };
    println!("{}", tools::probe(o));
}
