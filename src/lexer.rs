use crate::error::Error;
use crate::loc::Loc;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    Mut,
    Return,
    If,
    Else,
    Loop,
    For,
    In,
    Break,
    Continue,
    While,
    True,
    False,
    Struct,
    Impl,
    SelfKw,
    Enum,
    Match,
    Unsafe,
    As,
    Type,
    Mod,
    Pub,
    Trait,
    Dyn,
    Use,
    Extern,
    Super,
    Const,
    Static,
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
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    // Compound assignment operators
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
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
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    At,
    Dot,
    DotDot,
    DotDotEq,
    DotDotDot,
    Colon,
    ColonColon,
    Eq,
    Arrow,
    FatArrow,
    Bang,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub loc: Loc,
}

pub struct Lexer {
    src: Vec<u8>,
    pos: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self {
            src: src.as_bytes().to_vec(),
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
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.advance();
            }
            if self.src.get(self.pos..self.pos + 2) == Some(b"//") {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.advance();
                }
            } else if self.peek() == Some(b'#') {
                // Attribute: #[...] or #![...] -- skip entirely.
                self.advance();
                if self.peek() == Some(b'!') {
                    self.advance();
                }
                if self.peek() == Some(b'[') {
                    self.advance();
                    let mut depth: usize = 1;
                    loop {
                        match self.advance() {
                            Some(b'[') => depth += 1,
                            Some(b']') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            None => break,
                            _ => {}
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, Error> {
        self.skip_whitespace_and_comments();

        let loc = Loc::new(self.line, self.col);

        let ch = match self.peek() {
            None => {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    loc,
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
            b'[' => {
                self.advance();
                TokenKind::LBracket
            }
            b']' => {
                self.advance();
                TokenKind::RBracket
            }
            b';' => {
                self.advance();
                TokenKind::Semicolon
            }
            b',' => {
                self.advance();
                TokenKind::Comma
            }
            b'@' => {
                self.advance();
                TokenKind::At
            }
            b'.' => {
                self.advance();
                if self.peek() == Some(b'.') {
                    self.advance();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        TokenKind::DotDotEq
                    } else if self.peek() == Some(b'.') {
                        self.advance();
                        TokenKind::DotDotDot
                    } else {
                        TokenKind::DotDot
                    }
                } else {
                    TokenKind::Dot
                }
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
                    if self.peek() == Some(b'=') {
                        self.advance();
                        TokenKind::ShlEq
                    } else {
                        TokenKind::Shl
                    }
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
                    if self.peek() == Some(b'=') {
                        self.advance();
                        TokenKind::ShrEq
                    } else {
                        TokenKind::Shr
                    }
                } else {
                    TokenKind::Gt
                }
            }
            b'&' => {
                self.advance();
                if self.peek() == Some(b'&') {
                    self.advance();
                    TokenKind::AmpAmp
                } else if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::AmpEq
                } else {
                    TokenKind::Amp
                }
            }
            b'|' => {
                self.advance();
                if self.peek() == Some(b'|') {
                    self.advance();
                    TokenKind::PipePipe
                } else if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::PipeEq
                } else {
                    TokenKind::Pipe
                }
            }
            b'+' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            b'-' => {
                self.advance();
                if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Arrow
                } else if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            b'*' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            b'/' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            b'%' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::PercentEq
                } else {
                    TokenKind::Percent
                }
            }
            b'^' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::CaretEq
                } else {
                    TokenKind::Caret
                }
            }
            b'~' => {
                self.advance();
                TokenKind::Tilde
            }
            b'"' => self.lex_string(loc)?,
            b'\'' => self.lex_char(loc)?,
            c if c.is_ascii_digit() => self.lex_number(loc)?,
            // b'...' byte char literal and b"..." byte string literal
            b'b' if self.src.get(self.pos + 1).copied() == Some(b'\'') => {
                self.advance(); // consume 'b'
                // b'X' has integer type (u8); value must fit in 0..=255.
                match self.lex_char(loc)? {
                    TokenKind::CharLit(c) => {
                        if c > 255 {
                            return Err(Error::new(
                                loc,
                                "byte literal value must be in the range 0..=255",
                            ));
                        }
                        TokenKind::IntLit(c as i64)
                    }
                    other => other,
                }
            }
            b'b' if self.src.get(self.pos + 1).copied() == Some(b'"') => {
                self.advance(); // consume 'b'
                self.lex_string(loc)?
            }
            c if c.is_ascii_alphabetic() || c == b'_' => self.lex_ident_or_keyword(),
            _ => {
                self.advance();
                return Err(Error::new(
                    loc,
                    format!("unexpected character '{}'", ch as char),
                ));
            }
        };

        Ok(Token { kind, loc })
    }

    /// Read one complete UTF-8 character from the source byte stream.
    /// The source is always valid UTF-8 (constructed from a `&str`), so an
    /// invalid sequence indicates a bug in the caller, not bad user input.
    fn next_utf8_char(&mut self, loc: Loc) -> Result<char, Error> {
        let first = match self.advance() {
            None => return Err(Error::new(loc, "unexpected end of input")),
            Some(b) => b,
        };
        if first < 0x80 {
            return Ok(first as char);
        }
        let (n_cont, mut cp) = if first & 0xE0 == 0xC0 {
            (1, u32::from(first & 0x1F))
        } else if first & 0xF0 == 0xE0 {
            (2, u32::from(first & 0x0F))
        } else if first & 0xF8 == 0xF0 {
            (3, u32::from(first & 0x07))
        } else {
            return Err(Error::new(loc, "invalid UTF-8 sequence in source"));
        };
        for _ in 0..n_cont {
            match self.advance() {
                Some(b) if b & 0xC0 == 0x80 => cp = (cp << 6) | u32::from(b & 0x3F),
                _ => return Err(Error::new(loc, "invalid UTF-8 sequence in source")),
            }
        }
        char::from_u32(cp).ok_or_else(|| Error::new(loc, "invalid Unicode codepoint in source"))
    }

    /// Parse the `{HEX}` portion of a `\u{HEX}` escape sequence and return the
    /// resulting char. The leading `\u` must already have been consumed.
    fn parse_unicode_escape(&mut self, loc: Loc) -> Result<char, Error> {
        if self.advance() != Some(b'{') {
            return Err(Error::new(loc, "expected '{' in unicode escape"));
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
                _ => return Err(Error::new(loc, "invalid character in unicode escape")),
            }
        }
        if hex.is_empty() {
            return Err(Error::new(loc, "empty unicode escape sequence"));
        }
        let v = u32::from_str_radix(&hex, 16)
            .map_err(|_| Error::new(loc, "unicode escape value out of range"))?;
        if v > 0x10FFFF {
            return Err(Error::new(
                loc,
                format!("unicode escape '\\u{{{hex}}}' is out of range (max is \\u{{10FFFF}})"),
            ));
        }
        if (0xD800..=0xDFFF).contains(&v) {
            return Err(Error::new(
                loc,
                format!("unicode escape '\\u{{{hex}}}' is a surrogate code point"),
            ));
        }
        Ok(char::from_u32(v).unwrap())
    }

    fn lex_string(&mut self, loc: Loc) -> Result<TokenKind, Error> {
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return Err(Error::new(loc, "unterminated string literal")),
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance(); // consume backslash
                    match self.advance() {
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'"') => s.push('"'),
                        Some(b'0') => s.push('\0'),
                        Some(b'u') => s.push(self.parse_unicode_escape(loc)?),
                        // Backslash-newline: skip the newline and all leading whitespace on the
                        // next line, matching Rust's string literal line continuation.
                        Some(b'\n') => {
                            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                                self.advance();
                            }
                        }
                        Some(c) => {
                            return Err(Error::new(
                                loc,
                                format!("unknown escape '\\{}'", c as char),
                            ));
                        }
                        None => return Err(Error::new(loc, "unterminated escape sequence")),
                    }
                }
                Some(_) => s.push(self.next_utf8_char(loc)?),
            }
        }
        Ok(TokenKind::StringLit(s))
    }

    fn lex_char(&mut self, loc: Loc) -> Result<TokenKind, Error> {
        self.advance(); // consume opening '
        let ch: char = if self.peek() == Some(b'\\') {
            self.advance(); // consume backslash
            match self.advance() {
                Some(b'n') => '\n',
                Some(b't') => '\t',
                Some(b'r') => '\r',
                Some(b'\\') => '\\',
                Some(b'\'') => '\'',
                Some(b'"') => '"',
                Some(b'0') => '\0',
                Some(b'u') => self.parse_unicode_escape(loc)?,
                Some(c) => {
                    return Err(Error::new(loc, format!("unknown escape '\\{}'", c as char)));
                }
                None => return Err(Error::new(loc, "unterminated escape")),
            }
        } else {
            // Non-escape character: decode the full UTF-8 sequence.
            self.next_utf8_char(loc)?
        };
        if self.advance() != Some(b'\'') {
            return Err(Error::new(loc, "expected closing ' in char literal"));
        }
        Ok(TokenKind::CharLit(ch as u32))
    }

    /// Consume an optional integer type suffix (e.g. `u8`, `i64`, `usize`).
    /// Must be called right after the digit sequence, before returning the token.
    fn consume_int_suffix(&mut self) {
        // Longest suffixes first to avoid consuming a prefix of a longer one.
        const SUFFIXES: &[&[u8]] = &[
            b"u128", b"i128", b"usize", b"isize", b"u64", b"i64", b"u32", b"i32", b"u16", b"i16",
            b"u8", b"i8",
        ];
        for suffix in SUFFIXES {
            let end = self.pos + suffix.len();
            if self.src.get(self.pos..end) == Some(*suffix) {
                // Only consume if not followed by another alphanumeric char.
                if !self
                    .src
                    .get(end)
                    .copied()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    for _ in 0..suffix.len() {
                        self.advance();
                    }
                    return;
                }
            }
        }
    }

    fn lex_number(&mut self, loc: Loc) -> Result<TokenKind, Error> {
        if self.peek() == Some(b'0') {
            let next = self.src.get(self.pos + 1).copied();
            if next == Some(b'x') || next == Some(b'X') {
                self.advance();
                self.advance();
                let mut s = String::new();
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_hexdigit() || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
                if s.is_empty() {
                    return Err(Error::new(loc, "expected hex digits after '0x'"));
                }
                let tok = i64::from_str_radix(&s, 16)
                    .map(TokenKind::IntLit)
                    .map_err(|_| Error::new(loc, format!("hex literal '0x{s}' out of range")))?;
                self.consume_int_suffix();
                return Ok(tok);
            }
            if next == Some(b'b') || next == Some(b'B') {
                self.advance();
                self.advance();
                let mut s = String::new();
                while self
                    .peek()
                    .is_some_and(|c| c == b'0' || c == b'1' || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
                if s.is_empty() {
                    return Err(Error::new(loc, "expected binary digits after '0b'"));
                }
                let tok = i64::from_str_radix(&s, 2)
                    .map(TokenKind::IntLit)
                    .map_err(|_| Error::new(loc, format!("binary literal '0b{s}' out of range")))?;
                self.consume_int_suffix();
                return Ok(tok);
            }
        }
        let mut s = String::new();
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'_') {
            let c = self.advance().unwrap();
            if c != b'_' {
                s.push(c as char);
            }
        }
        let is_float = (self.peek() == Some(b'.')
            && self.src.get(self.pos + 1).copied() != Some(b'.'))
            || matches!(self.peek(), Some(b'e') | Some(b'E'));
        if is_float {
            if self.peek() == Some(b'.') {
                s.push('.');
                self.advance();
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'_') {
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
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    s.push(self.advance().unwrap() as char);
                }
            }
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
                .map_err(|_| Error::new(loc, format!("float literal '{s}' out of range")));
        }
        let tok = s
            .parse::<i64>()
            .map(TokenKind::IntLit)
            .map_err(|_| Error::new(loc, format!("integer literal '{s}' out of range")))?;
        self.consume_int_suffix();
        Ok(tok)
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
        let mut name = String::new();
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
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
            "loop" => TokenKind::Loop,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "struct" => TokenKind::Struct,
            "impl" => TokenKind::Impl,
            "self" => TokenKind::SelfKw,
            "enum" => TokenKind::Enum,
            "match" => TokenKind::Match,
            "unsafe" => TokenKind::Unsafe,
            "as" => TokenKind::As,
            "type" => TokenKind::Type,
            "mod" => TokenKind::Mod,
            "pub" => TokenKind::Pub,
            "trait" => TokenKind::Trait,
            "dyn" => TokenKind::Dyn,
            "use" => TokenKind::Use,
            "extern" => TokenKind::Extern,
            "super" => TokenKind::Super,
            "const" => TokenKind::Const,
            "static" => TokenKind::Static,
            _ => TokenKind::Ident(name),
        }
    }
}
