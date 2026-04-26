// utils.rs: child module that references a type declared in the parent file.
// 'Point' is not declared here -- it lives at the top level of main.rs.
// With an unbounded prescan (Bug C), 'Point' would appear in utils's
// mod_local_names and be wrongly qualified as 'utils_Point'.

pub fn origin() -> Point {
    Point { x: 0, y: 0 }
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
