// out: 0 0
// out: 3
// File mod prescan regression (Bug C): the child mod's local-name scan must be
// bounded to the child's own tokens. If the scan bleeds into the parent tokens,
// top-level names like 'Point' would be wrongly added to the child's local_names
// and then qualify() would mangle them as 'utils_Point' (non-existent).

mod utils;

pub struct Point { pub x: i32, pub y: i32 }

fn main() {
    let o: Point = utils::origin();
    println!("{} {}", o.x, o.y);
    println!("{}", utils::add(1, 2));
}
