// Showcase: Traits and dynamic dispatch.
//
// Demonstrates:
//   - trait declarations with multiple methods
//   - impl Trait for Type (multiple concrete types)
//   - inherent methods coexisting with trait impls
//   - dyn Trait fat-pointer dynamic dispatch
//   - passing dyn Trait to functions
//   - multiple traits on different types
//   - trait returning primitives, &str, and i32

// ---------------------------------------------------------------------------
// Shape trait -- anything that has an area and a name.
// ---------------------------------------------------------------------------

trait Shape {
    fn area(&self) -> f32;
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Drawable trait -- anything that can describe how it renders.
// ---------------------------------------------------------------------------

trait Drawable {
    fn describe(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Circle
// ---------------------------------------------------------------------------

struct Circle {
    pub radius: f32,
}

impl Circle {
    // Inherent method -- not part of any trait.
    fn diameter(&self) -> f32 {
        self.radius * 2.0
    }
}

impl Shape for Circle {
    fn area(&self) -> f32 {
        // pi * r^2 approximated as 3.14159 * r * r
        let pi: f32 = 3.14159;
        pi * self.radius * self.radius
    }
    fn name(&self) -> &str {
        "circle"
    }
}

impl Drawable for Circle {
    fn describe(&self) -> &str {
        "a round shape"
    }
}

// ---------------------------------------------------------------------------
// Rectangle
// ---------------------------------------------------------------------------

struct Rectangle {
    pub width: f32,
    pub height: f32,
}

impl Rectangle {
    fn perimeter(&self) -> f32 {
        (self.width + self.height) * 2.0
    }
}

impl Shape for Rectangle {
    fn area(&self) -> f32 {
        self.width * self.height
    }
    fn name(&self) -> &str {
        "rectangle"
    }
}

impl Drawable for Rectangle {
    fn describe(&self) -> &str {
        "a four-sided shape"
    }
}

// ---------------------------------------------------------------------------
// Triangle
// ---------------------------------------------------------------------------

struct Triangle {
    pub base: f32,
    pub height: f32,
}

impl Shape for Triangle {
    fn area(&self) -> f32 {
        self.base * self.height * 0.5
    }
    fn name(&self) -> &str {
        "triangle"
    }
}

impl Drawable for Triangle {
    fn describe(&self) -> &str {
        "a three-sided shape"
    }
}

// ---------------------------------------------------------------------------
// Logger trait -- used to show a second independent trait hierarchy.
// ---------------------------------------------------------------------------

trait Logger {
    fn log_level(&self) -> &str;
    fn message(&self) -> &str;
}

struct InfoLog {
    pub text: &str,
}

struct WarnLog {
    pub text: &str,
}

impl Logger for InfoLog {
    fn log_level(&self) -> &str {
        "INFO"
    }
    fn message(&self) -> &str {
        self.text
    }
}

impl Logger for WarnLog {
    fn log_level(&self) -> &str {
        "WARN"
    }
    fn message(&self) -> &str {
        self.text
    }
}

// ---------------------------------------------------------------------------
// Free functions that accept dyn Trait.
// ---------------------------------------------------------------------------

fn print_shape(s: dyn Shape) {
    println!("[{}] area = {}", s.name(), s.area());
}

fn print_drawable(d: dyn Drawable) {
    println!("drawable: {}", d.describe());
}

fn print_log(l: dyn Logger) {
    println!("[{}] {}", l.log_level(), l.message());
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let c: Circle = Circle { radius: 5.0 };
    let r: Rectangle = Rectangle { width: 4.0, height: 6.0 };
    let t: Triangle = Triangle { base: 3.0, height: 8.0 };

    // --- static dispatch: inherent methods ---
    println!("=== Inherent methods ===");
    println!("circle diameter: {}", c.diameter());
    println!("rectangle perimeter: {}", r.perimeter());

    // --- dyn dispatch via free functions ---
    println!("=== Shape trait (dyn dispatch) ===");
    print_shape(&c as dyn Shape);
    print_shape(&r as dyn Shape);
    print_shape(&t as dyn Shape);

    // --- Drawable trait ---
    println!("=== Drawable trait (dyn dispatch) ===");
    print_drawable(&c as dyn Drawable);
    print_drawable(&r as dyn Drawable);
    print_drawable(&t as dyn Drawable);

    // --- Logger trait, completely separate hierarchy ---
    println!("=== Logger trait (dyn dispatch) ===");
    let info: InfoLog = InfoLog { text: "server started" };
    let warn: WarnLog = WarnLog { text: "low memory" };
    print_log(&info as dyn Logger);
    print_log(&warn as dyn Logger);

    // --- store dyn in a local variable and call through it ---
    println!("=== dyn stored in variable ===");
    let ds: dyn Shape = &c as dyn Shape;
    println!("stored shape name: {}", ds.name());
    println!("stored shape area: {}", ds.area());
}
