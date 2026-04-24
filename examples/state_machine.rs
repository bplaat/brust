// Showcase: traffic light state machine — enums, while loop, and pattern matching.
//
// Demonstrates:
//   - Simple unit enum as a state type
//   - State transition function returning an enum variant
//   - while loop advancing the state N times

enum Light {
    Red,
    Yellow,
    Green,
}

fn next(state: Light) -> Light {
    match state {
        Light::Red    => Light::Green,
        Light::Green  => Light::Yellow,
        Light::Yellow => Light::Red,
    }
}

fn label(state: Light) -> &str {
    match state {
        Light::Red    => "RED   -- stop",
        Light::Green  => "GREEN -- go",
        Light::Yellow => "YELLOW -- slow",
    }
}

fn main() {
    let mut state: Light = Light::Red;
    let mut step: i32 = 0;
    while step < 9 {
        println!("{}", label(state));
        state = next(state);
        step = step + 1;
    }
}
