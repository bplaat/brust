use std::fmt;

#[derive(Debug)]
pub struct Error {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl Error {
    pub fn new(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            col,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: error: {}", self.line, self.col, self.message)
    }
}
