use crate::ast::{Block, BinOp, Expr, FnDecl, File, Item, Stmt};
use crate::error::Error;
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<File, Error> {
        let mut items = Vec::new();
        while !self.at_eof() {
            items.push(self.parse_item()?);
        }
        Ok(File { items })
    }

    // --- helpers ---

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn at_eof(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<&Token, Error> {
        let tok = self.peek().clone();
        if &tok.kind == kind {
            self.advance();
            Ok(&self.tokens[self.pos - 1])
        } else {
            Err(Error::new(tok.line, tok.col, format!("expected {:?}, got {:?}", kind, tok.kind)))
        }
    }

    fn expect_ident(&mut self) -> Result<String, Error> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = &tok.kind {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            Err(Error::new(tok.line, tok.col, format!("expected identifier, got {:?}", tok.kind)))
        }
    }

    // --- grammar ---

    fn parse_item(&mut self) -> Result<Item, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Fn => Ok(Item::Fn(self.parse_fn()?)),
            _ => Err(Error::new(tok.line, tok.col, format!("expected item, got {:?}", tok.kind))),
        }
    }

    fn parse_fn(&mut self) -> Result<FnDecl, Error> {
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;
        let body = self.parse_block()?;
        Ok(FnDecl { name, body })
    }

    fn parse_block(&mut self) -> Result<Block, Error> {
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Error> {
        let tok = self.peek().clone();
        // println! macro call
        if let TokenKind::Ident(name) = &tok.kind {
            if name == "println" {
                return self.parse_println();
            }
        }
        Err(Error::new(tok.line, tok.col, format!("unexpected token in statement: {:?}", tok.kind)))
    }

    fn parse_println(&mut self) -> Result<Stmt, Error> {
        self.expect_ident()?; // println
        self.expect(&TokenKind::Bang)?;
        self.expect(&TokenKind::LParen)?;

        let str_tok = self.peek().clone();
        let format = match &str_tok.kind {
            TokenKind::StringLit(s) => s.clone(),
            _ => return Err(Error::new(str_tok.line, str_tok.col,
                "println! expects a string literal as first argument")),
        };
        self.advance();

        // Parse optional expression arguments: , expr, expr, ...
        let mut args = Vec::new();
        while self.peek().kind == TokenKind::Comma {
            self.advance(); // consume ','
            args.push(self.parse_expr()?);
        }

        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Println { format, args })
    }

    // --- expressions (recursive descent with precedence) ---

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.parse_additive()
    }

    fn parse_additive(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus  => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplicative()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star    => BinOp::Mul,
                TokenKind::Slash   => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        // Unary minus: -expr
        if self.peek().kind == TokenKind::Minus {
            self.advance();
            let operand = self.parse_primary()?;
            return Ok(Expr::BinOp {
                op: BinOp::Sub,
                lhs: Box::new(Expr::Int(0)),
                rhs: Box::new(operand),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::IntLit(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Int(n))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(Error::new(tok.line, tok.col,
                format!("expected expression, got {:?}", tok.kind))),
        }
    }
}
