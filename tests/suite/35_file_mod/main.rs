// out: 24

mod shapes;

use shapes::Rect;

fn main() {
    let r: Rect = Rect {
        width: 4,
        height: 6,
    };
    println!("{}", shapes::area(r));
}
