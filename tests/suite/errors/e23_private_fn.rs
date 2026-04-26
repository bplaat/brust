// err: `vault_make_secret` is private

mod vault {
    pub struct Key {
        pub id: i32,
    }

    fn make_secret() -> Key {
        Key { id: 0 }
    }
}

fn main() {
    let k: vault::Key = vault::make_secret();
}
