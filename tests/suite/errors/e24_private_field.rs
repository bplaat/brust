// err: field `secret` of `bag_Bag` is private

mod bag {
    pub struct Bag {
        pub label: i32,
        secret: i32,
    }

    pub fn make(label: i32) -> Bag {
        Bag { label: label, secret: 99 }
    }
}

fn main() {
    let b: bag::Bag = bag::make(1);
    println!("{}", b.secret);
}
