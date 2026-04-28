use crate::loc::Loc;
use std::fmt;

#[derive(Debug)]
pub struct Error {
    pub loc: Loc,
    pub message: String,
}

impl Error {
    pub fn new(loc: Loc, message: impl Into<String>) -> Self {
        Self {
            loc,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: error: {}",
            self.loc.line, self.loc.col, self.message
        )
    }
}
