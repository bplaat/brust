// out: circle area: 81
// out: rect area: 12
// out: point
// out: 7

enum Shape {
    Circle(i32),
    Rect(i32, i32),
    Point,
}

fn area(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r * r,
        Shape::Rect(w, h) => w * h,
        Shape::Point => 0,
    }
}

fn describe(s: Shape) {
    match s {
        Shape::Circle(r) => println!("circle area: {}", r * r),
        Shape::Rect(w, h) => println!("rect area: {}", w * h),
        Shape::Point => println!("point"),
    }
}

fn main() {
    describe(Shape::Circle(9));
    describe(Shape::Rect(3, 4));
    describe(Shape::Point);
    println!("{}", area(Shape::Circle(2)) + area(Shape::Rect(1, 3)));
}
