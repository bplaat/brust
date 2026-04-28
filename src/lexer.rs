use crate::error::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    Mut,
    Return,
    If,
    Else,
    While,
    True,
    False,
    Struct,
    Impl,
    SelfKw, // `self`
    Enum,
    Match,
    Unsafe,
    As,
    // Identifiers and literals
    Ident(String),
    IntLit(i64),
    FloatLit(f64),
    CharLit(u32),
    StringLit(String),
    // Arithmetic operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    // Bitwise operators
    Amp,   // &
    Pipe,  // |
    Caret, // ^
    Tilde, // ~
    Shl,   // <<
    Shr,   // >>
    // Comparison operators
    EqEq,
    BangEq,
    Lt,
    Gt,
    Le,
    Ge,
    // Logical operators
    AmpAmp,
    PipePipe,
    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semicolon,
    Comma,
    Dot,
    Colon,
    ColonColon,
    Eq,
    Arrow,    // ->
    FatArrow, // =>
    Bang,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, Error> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if done {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.src.get(self.pos).copied()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while self.peek().map_or(false, |c| c.is_ascii_whitespace()) {
                self.advance();
            }
            // Skip line comments
            if self.src.get(self.pos..self.pos + 2) == Some(b"//") {
                while self.peek().map_or(false, |c| c != b'\n') {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, Error> {
        self.skip_whitespace_and_comments();

        let line = self.line;
        let col = self.col;

        let ch = match self.peek() {
            None => {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    line,
                    col,
                });
            }
            Some(c) => c,
        };

        let kind = match ch {
            b'(' => {
                self.advance();
                TokenKind::LParen
            }
            b')' => {
                self.advance();
                TokenKind::RParen
            }
            b'{' => {
                self.advance();
                TokenKind::LBrace
            }
            b'}' => {
                self.advance();
                TokenKind::RBrace
            }
            b';' => {
                self.advance();
                TokenKind::Semicolon
            }
            b',' => {
                self.advance();
                TokenKind::Comma
            }
            b'.' => {
                self.advance();
                TokenKind::Dot
            }
            b':' => {
                self.advance();
                if self.peek() == Some(b':') {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            b'=' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            b'!' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }
            b'<' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::Le
                } else if self.peek() == Some(b'<') {
                    self.advance();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            b'>' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::Ge
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            b'&' => {
                self.advance();
                if self.peek() == Some(b'&') {
                    self.advance();
                    TokenKind::AmpAmp
                } else {
                    TokenKind::Amp
                }
            }
            b'|' => {
                self.advance();
                if self.peek() == Some(b'|') {
                    self.advance();
                    TokenKind::PipePipe
                } else {
                    TokenKind::Pipe
                }
            }
            b'+' => {
                self.advance();
                TokenKind::Plus
            }
            b'-' => {
                self.advance();
                if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            b'*' => {
                self.advance();
                TokenKind::Star
            }
            b'/' => {
                self.advance();
                TokenKind::Slash
            }
            b'%' => {
                self.advance();
                TokenKind::Percent
            }
            b'^' => {
                self.advance();
                TokenKind::Caret
            }
            b'~' => {
                self.advance();
                TokenKind::Tilde
            }
            b'"' => self.lex_string(line, col)?,
            b'\'' => self.lex_char(line, col)?,
            c if c.is_ascii_digit() => self.lex_number(line, col)?,
            c if c.is_ascii_alphabetic() || c == b'_' => self.lex_ident_or_keyword(),
            _ => {
                self.advance();
                return Err(Error::new(
                    line,
                    col,
                    format!("unexpected character '{}'", ch as char),
                ));
            }
        };

        Ok(Token { kind, line, col })
    }

    fn lex_string(&mut self, line: usize, col: usize) -> Result<TokenKind, Error> {
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(Error::new(line, col, "unterminated string literal")),
                Some(b'"') => break,
                Some(b'\\') => match self.advance() {
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'"') => s.push('"'),
                    Some(c) => s.push(c as char),
                    None => return Err(Error::new(line, col, "unterminated escape sequence")),
                },
                Some(c) => s.push(c as char),
            }
        }
        Ok(TokenKind::StringLit(s))
    }

    fn lex_char(&mut self, line: usize, col: usize) -> Result<TokenKind, Error> {
        self.advance(); // consume opening '
        let ch: u32 = match self.advance() {
            None => return Err(Error::new(line, col, "unterminated char literal")),
            Some(b'\\') => match self.advance() {
                Some(b'n') => '\n' as u32,
                Some(b't') => '\t' as u32,
                Some(b'r') => '\r' as u32,
                Some(b'\\') => '\\' as u32,
                Some(b'\'') => '\'' as u32,
                Some(b'"') => '"' as u32,
                Some(b'0') => 0,
                Some(b'u') => {
                    // \u{XXXXXX}
                    if self.advance() != Some(b'{') {
                        return Err(Error::new(line, col, "expected '{' in unicode escape"));
                    }
                    let mut hex = String::new();
                    loop {
                        match self.peek() {
                            Some(b'}') => {
                                self.advance();
                                break;
                            }
                            Some(c) if (c as char).is_ascii_hexdigit() => {
                                hex.push(self.advance().unwrap() as char);
                            }
                            _ => return Err(Error::new(line, col, "invalid unicode escape")),
                        }
                    }
                    u32::from_str_radix(&hex, 16)
                        .map_err(|_| Error::new(line, col, "unicode escape out of range"))?
                }
                Some(c) => {
                    return Err(Error::new(
                        line,
                        col,
                        format!("unknown escape '\\{}'", c as char),
                    ));
                }
                None => return Err(Error::new(line, col, "unterminated escape")),
            },
            Some(c) => c as u32,
        };
        if self.advance() != Some(b'\'') {
            return Err(Error::new(line, col, "expected closing ' in char literal"));
        }
        Ok(TokenKind::CharLit(ch))
    }

    fn lex_number(&mut self, line: usize, col: usize) -> Result<TokenKind, Error> {
        // Check for 0x (hex) or 0b (binary) prefix
        if self.peek() == Some(b'0') {
            let next = self.src.get(self.pos + 1).copied();
            if next == Some(b'x') || next == Some(b'X') {
                self.advance();
                self.advance(); // consume '0x'
                let mut s = String::new();
                while self
                    .peek()
                    .map_or(false, |c| c.is_ascii_hexdigit() || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
                return i64::from_str_radix(&s, 16)
                    .map(TokenKind::IntLit)
                    .map_err(|_| {
                        Error::new(line, col, format!("hex literal '0x{s}' out of range"))
                    });
            }
            if next == Some(b'b') || next == Some(b'B') {
                self.advance();
                self.advance(); // consume '0b'
                let mut s = String::new();
                while self
                    .peek()
                    .map_or(false, |c| c == b'0' || c == b'1' || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
                return i64::from_str_radix(&s, 2)
                    .map(TokenKind::IntLit)
                    .map_err(|_| {
                        Error::new(line, col, format!("binary literal '0b{s}' out of range"))
                    });
            }
        }
        let mut s = String::new();
        while self
            .peek()
            .map_or(false, |c| c.is_ascii_digit() || c == b'_')
        {
            let c = self.advance().unwrap();
            if c != b'_' {
                s.push(c as char);
            }
        }
        // Float: digit sequence followed by `.` (not `..`) or `e`/`E`
        let is_float = self.peek() == Some(b'.')
            && self.src.get(self.pos + 1).copied() != Some(b'.')
            || matches!(self.peek(), Some(b'e') | Some(b'E'));
        if is_float {
            if self.peek() == Some(b'.') {
                s.push('.');
                self.advance();
                while self
                    .peek()
                    .map_or(false, |c| c.is_ascii_digit() || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
            }
            if matches!(self.peek(), Some(b'e') | Some(b'E')) {
                s.push('e');
                self.advance();
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    s.push(self.advance().unwrap() as char);
                }
                while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                    s.push(self.advance().unwrap() as char);
                }
            }
            // Consume optional `f32`/`f64` suffix
            if self.src.get(self.pos..self.pos + 3) == Some(b"f32")
                || self.src.get(self.pos..self.pos + 3) == Some(b"f64")
            {
                self.advance();
                self.advance();
                self.advance();
            }
            return s
                .parse::<f64>()
                .map(TokenKind::FloatLit)
                .map_err(|_| Error::new(line, col, format!("float literal '{s}' out of range")));
        }
        s.parse::<i64>()
            .map(TokenKind::IntLit)
            .map_err(|_| Error::new(line, col, format!("integer literal '{s}' out of range")))
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
        let mut name = String::new();
        while self
            .peek()
            .map_or(false, |c| c.is_ascii_alphanumeric() || c == b'_')
        {
            name.push(self.advance().unwrap() as char);
        }
        match name.as_str() {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "struct" => TokenKind::Struct,
            "impl" => TokenKind::Impl,
            "self" => TokenKind::SelfKw,
            "enum" => TokenKind::Enum,
            "match" => TokenKind::Match,
            "unsafe" => TokenKind::Unsafe,
            "as" => TokenKind::As,
            _ => TokenKind::Ident(name),
        }
    }
}
