// out: 99
// out: 7
//
// Regression tests for four module-system bugs:
// A) validate_items must set cur_mod so private types within their own module are accepted.
// B) use-aliases must not leak out of a mod block.
// C) collect_mod_local_names for file mods must not scan past the spliced token range.
// D) mod::Struct {} must check item and field visibility.
// E) private impl methods must not be callable from outside the module.

// Bug A: private struct usable inside its own mod (previously false-rejected).
mod bugA {
    struct Secret {
        x: i32,
    }
    pub fn run() -> i32 {
        let s: Secret = Secret { x: 99 };
        s.x
    }
}

// Bug B: use-alias scoped to its mod (alias must not escape).
mod bugB {
    pub struct Marker {
        pub v: i32,
    }
    use bugB::Marker;
    pub fn make() -> Marker {
        Marker { v: 7 }
    }
}
// At this level 'Marker' is NOT in scope -- must use bugB::Marker.

fn main() {
    println!("{}", bugA::run());
    let m: bugB::Marker = bugB::make();
    println!("{}", m.v);
}
