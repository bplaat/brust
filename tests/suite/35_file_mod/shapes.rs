pub struct Rect {
    pub width: i32,
    pub height: i32,
}

pub fn area(r: Rect) -> i32 {
    r.width * r.height
}
