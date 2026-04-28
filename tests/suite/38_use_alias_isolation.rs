// out: 10
// out: 20
// Sibling modules each declare a use-alias with the same short name.
// The aliases must not cross module boundaries in either direction.

mod alpha {
    pub struct Val {
        pub n: i32,
    }
    use alpha::Val as V;
    pub fn make(n: i32) -> V {
        V { n: n }
    }
}

mod beta {
    pub struct Val {
        pub n: i32,
    }
    // Beta also defines 'V', independently from alpha's alias.
    use beta::Val as V;
    pub fn make(n: i32) -> V {
        V { n: n }
    }
}

// At this scope neither 'V' alias is visible; must use qualified paths.
fn main() {
    let a: alpha::Val = alpha::make(10);
    let b: beta::Val = beta::make(20);
    println!("{}", a.n);
    println!("{}", b.n);
}
