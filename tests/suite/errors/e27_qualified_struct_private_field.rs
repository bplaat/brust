// err: field `secret` of `vault_Safe` is private
// Bug D: mod::Struct { private_field: ... } must be rejected.

mod vault {
    pub struct Safe {
        secret: i32,
        pub label: i32,
    }
}

fn main() {
    let _s: vault::Safe = vault::Safe {
        secret: 1,
        label: 2,
    };
}
