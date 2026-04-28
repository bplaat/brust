/// Source location: 1-based line and column.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loc {
    pub line: u32,
    pub col: u32,
}

impl Loc {
    pub fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }
}
