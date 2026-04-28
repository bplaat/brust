// err: missing field `secret` in `store_Box`
// mod::Struct {} literal through the EnumStructLit path must enforce that
// all required fields are provided, including private ones the caller cannot set.
// The caller can't supply 'secret' (it's private) but can't omit it either.

mod store {
    pub struct Box {
        pub label: i32,
        secret: i32,
    }
    pub fn make(label: i32) -> Box {
        Box {
            label: label,
            secret: 0,
        }
    }
}

fn main() {
    // Caller omits 'secret' (private) -- should error: missing field
    let _b: store::Box = store::Box { label: 1 };
}
