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
    IntLit(i64, Option<IntSuffix>),
    FloatLit(f64, Option<FloatSuffix>),
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

#[derive(Debug, Clone, PartialEq)]
pub enum FloatSuffix {
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntSuffix {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
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
            b'"' => self.lex_string(loc, false)?,
            b'\'' => self.lex_char(loc, false)?,
            c if c.is_ascii_digit() => self.lex_number(loc)?,
            // b'...' byte char literal and b"..." byte string literal
            b'b' if self.src.get(self.pos + 1).copied() == Some(b'\'') => {
                self.advance(); // consume 'b'
                // b'X' has integer type u8; byte_mode enforces ASCII-only and 0..=255 range.
                match self.lex_char(loc, true)? {
                    TokenKind::CharLit(c) => TokenKind::IntLit(c as i64, Some(IntSuffix::U8)),
                    other => other,
                }
            }
            b'b' if self.src.get(self.pos + 1).copied() == Some(b'"') => {
                self.advance(); // consume 'b'
                self.lex_string(loc, true)?
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
                Some(c) if (c as char).is_ascii_hexdigit() || c == b'_' => {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        hex.push(c as char);
                    }
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

    /// Parse the `NN` portion of a `\xNN` escape and return the resulting char.
    /// The leading `\x` must already have been consumed.
    /// `byte_mode` controls whether values above 0x7F are accepted (for byte literals).
    fn parse_hex_escape(&mut self, loc: Loc, byte_mode: bool) -> Result<char, Error> {
        let hi = match self.peek() {
            Some(c) if (c as char).is_ascii_hexdigit() => {
                self.advance().unwrap() as char
            }
            _ => return Err(Error::new(loc, "expected two hex digits in \\x escape")),
        };
        let lo = match self.peek() {
            Some(c) if (c as char).is_ascii_hexdigit() => {
                self.advance().unwrap() as char
            }
            _ => return Err(Error::new(loc, "expected two hex digits in \\x escape")),
        };
        let v = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap();
        if !byte_mode && v > 0x7F {
            return Err(Error::new(
                loc,
                "this form of character escape may only be used with characters in the range [\\x00-\\x7f]",
            ));
        }
        Ok(v as char)
    }

    fn lex_string(&mut self, loc: Loc, byte_mode: bool) -> Result<TokenKind, Error> {
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
                        Some(b'x') => s.push(self.parse_hex_escape(loc, byte_mode)?),
                        Some(b'u') if byte_mode => {
                            return Err(Error::new(
                                loc,
                                "unicode escapes are not allowed in byte string literals",
                            ));
                        }
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
                Some(_) => {
                    let ch = self.next_utf8_char(loc)?;
                    if byte_mode && ch as u32 > 0x7F {
                        return Err(Error::new(
                            loc,
                            "non-ASCII characters are not allowed in byte string literals",
                        ));
                    }
                    s.push(ch);
                }
            }
        }
        Ok(TokenKind::StringLit(s))
    }

    fn lex_char(&mut self, loc: Loc, byte_mode: bool) -> Result<TokenKind, Error> {
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
                Some(b'x') => self.parse_hex_escape(loc, byte_mode)?,
                Some(b'u') if byte_mode => {
                    return Err(Error::new(
                        loc,
                        "unicode escapes are not allowed in byte literals",
                    ));
                }
                Some(b'u') => self.parse_unicode_escape(loc)?,
                Some(c) => {
                    return Err(Error::new(loc, format!("unknown escape '\\{}'", c as char)));
                }
                None => return Err(Error::new(loc, "unterminated escape")),
            }
        } else {
            let ch = self.next_utf8_char(loc)?;
            if byte_mode && ch as u32 > 0x7F {
                return Err(Error::new(
                    loc,
                    "non-ASCII characters are not allowed in byte literals",
                ));
            }
            ch
        };
        if self.advance() != Some(b'\'') {
            return Err(Error::new(loc, "expected closing ' in char literal"));
        }
        Ok(TokenKind::CharLit(ch as u32))
    }

    /// Consume an optional integer type suffix (e.g. `u8`, `i64`, `usize`).
    /// Must be called right after the digit sequence, before returning the token.
    fn consume_int_suffix(&mut self) -> Option<IntSuffix> {
        // Longest suffixes first to avoid consuming a prefix of a longer one.
        const SUFFIXES: &[(&[u8], IntSuffix)] = &[
            (b"usize", IntSuffix::Usize),
            (b"isize", IntSuffix::Isize),
            (b"u64", IntSuffix::U64),
            (b"i64", IntSuffix::I64),
            (b"u32", IntSuffix::U32),
            (b"i32", IntSuffix::I32),
            (b"u16", IntSuffix::U16),
            (b"i16", IntSuffix::I16),
            (b"u8", IntSuffix::U8),
            (b"i8", IntSuffix::I8),
        ];
        for (bytes, suffix) in SUFFIXES {
            let end = self.pos + bytes.len();
            if self.src.get(self.pos..end) == Some(*bytes) {
                // Only consume if not followed by another alphanumeric char or underscore.
                if !self
                    .src
                    .get(end)
                    .copied()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    for _ in 0..bytes.len() {
                        self.advance();
                    }
                    return Some(suffix.clone());
                }
            }
        }
        None
    }

    fn check_int_range(loc: Loc, value: i64, suffix: &IntSuffix) -> Result<(), Error> {
        let ok = match suffix {
            IntSuffix::I8 => (0..=127i64).contains(&value),
            IntSuffix::I16 => (0..=32767i64).contains(&value),
            IntSuffix::I32 => (0..=2147483647i64).contains(&value),
            IntSuffix::I64 | IntSuffix::Isize => true,
            IntSuffix::U8 => (0..=255i64).contains(&value),
            IntSuffix::U16 => (0..=65535i64).contains(&value),
            IntSuffix::U32 => (0..=4294967295i64).contains(&value),
            IntSuffix::U64 | IntSuffix::Usize => value >= 0,
        };
        if ok {
            Ok(())
        } else {
            let ty_name = match suffix {
                IntSuffix::I8 => "i8",
                IntSuffix::I16 => "i16",
                IntSuffix::I32 => "i32",
                IntSuffix::I64 => "i64",
                IntSuffix::Isize => "isize",
                IntSuffix::U8 => "u8",
                IntSuffix::U16 => "u16",
                IntSuffix::U32 => "u32",
                IntSuffix::U64 => "u64",
                IntSuffix::Usize => "usize",
            };
            Err(Error::new(
                loc,
                format!("literal value {value} is out of range for `{ty_name}`"),
            ))
        }
    }

    fn consume_float_suffix(&mut self) -> Option<FloatSuffix> {
        // Allow optional leading underscore separator (e.g. `5_f32`).
        let offset = if self.peek() == Some(b'_')
            && self.src.get(self.pos + 1).copied() == Some(b'f')
        {
            1
        } else {
            0
        };
        for (bytes, suffix) in &[
            (b"f32" as &[u8], FloatSuffix::F32),
            (b"f64" as &[u8], FloatSuffix::F64),
        ] {
            let start = self.pos + offset;
            let end = start + bytes.len();
            if self.src.get(start..end) == Some(*bytes) {
                if !self
                    .src
                    .get(end)
                    .copied()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    for _ in 0..offset + bytes.len() {
                        self.advance();
                    }
                    return Some(suffix.clone());
                }
            }
        }
        None
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
                let value = i64::from_str_radix(&s, 16)
                    .map_err(|_| Error::new(loc, format!("hex literal '0x{s}' out of range")))?;
                let suffix = self.consume_int_suffix();
                if let Some(ref suf) = suffix {
                    Self::check_int_range(loc, value, suf)?;
                }
                return Ok(TokenKind::IntLit(value, suffix));
            }
            if next == Some(b'o') || next == Some(b'O') {
                self.advance();
                self.advance();
                let mut s = String::new();
                while self
                    .peek()
                    .is_some_and(|c| matches!(c, b'0'..=b'7') || c == b'_')
                {
                    let c = self.advance().unwrap();
                    if c != b'_' {
                        s.push(c as char);
                    }
                }
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(Error::new(loc, "invalid digit in octal literal"));
                }
                if s.is_empty() {
                    return Err(Error::new(loc, "expected octal digits after '0o'"));
                }
                let value = i64::from_str_radix(&s, 8)
                    .map_err(|_| Error::new(loc, format!("octal literal '0o{s}' out of range")))?;
                let suffix = self.consume_int_suffix();
                if let Some(ref suf) = suffix {
                    Self::check_int_range(loc, value, suf)?;
                }
                return Ok(TokenKind::IntLit(value, suffix));
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
                let value = i64::from_str_radix(&s, 2)
                    .map_err(|_| Error::new(loc, format!("binary literal '0b{s}' out of range")))?;
                let suffix = self.consume_int_suffix();
                if let Some(ref suf) = suffix {
                    Self::check_int_range(loc, value, suf)?;
                }
                return Ok(TokenKind::IntLit(value, suffix));
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
            let suffix = self.consume_float_suffix();
            return s
                .parse::<f64>()
                .map(|f| TokenKind::FloatLit(f, suffix))
                .map_err(|_| Error::new(loc, format!("float literal '{s}' out of range")));
        }
        // Check for float suffix on a decimal integer literal (e.g. `5f32` is a float).
        if let Some(float_suffix) = self.consume_float_suffix() {
            return s
                .parse::<f64>()
                .map(|f| TokenKind::FloatLit(f, Some(float_suffix)))
                .map_err(|_| Error::new(loc, format!("float literal '{s}' out of range")));
        }
        let value = s
            .parse::<i64>()
            .map_err(|_| Error::new(loc, format!("integer literal '{s}' out of range")))?;
        let suffix = self.consume_int_suffix();
        if let Some(ref suf) = suffix {
            Self::check_int_range(loc, value, suf)?;
        }
        Ok(TokenKind::IntLit(value, suffix))
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
