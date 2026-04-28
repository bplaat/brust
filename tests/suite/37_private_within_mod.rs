// out: ok
// Comprehensive test: private items (fn, struct, field, method) are all
// fully accessible within the module that declares them.

mod lib {
    struct Handle {
        value: i32,
    }

    fn new_handle(v: i32) -> Handle {
        Handle { value: v }
    }

    fn read_handle(h: Handle) -> i32 {
        h.value
    }

    pub struct Wrapper {
        inner: i32,
    }

    pub fn wrap(x: i32) -> Wrapper {
        let h: Handle = new_handle(x);
        Wrapper {
            inner: read_handle(h),
        }
    }

    pub fn unwrap(w: Wrapper) -> i32 {
        w.inner
    }

    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn new() -> Counter {
            Counter { count: 0 }
        }

        fn increment(&self) -> Counter {
            Counter {
                count: self.count + 1,
            }
        }

        pub fn tick(&self) -> Counter {
            // public method calling a private method within the same impl
            self.increment()
        }

        pub fn value(&self) -> i32 {
            self.count
        }
    }
}

fn main() {
    let w: lib::Wrapper = lib::wrap(42);
    let v: i32 = lib::unwrap(w);

    let c0: lib::Counter = lib::Counter::new();
    let c1: lib::Counter = c0.tick();
    let c2: lib::Counter = c1.tick();

    if v == 42 && c2.value() == 2 {
        println!("ok");
    }
}
