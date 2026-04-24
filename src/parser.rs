use crate::ast::{Block, BinOp, Expr, FnDecl, File, Item, Param, Stmt, Ty, UnOp};
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
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        // Optional return type: `-> Ty`
        let return_ty = if self.peek().kind == TokenKind::Arrow {
            self.advance();
            self.parse_ty()?
        } else {
            Ty::Unit
        };
        let body = self.parse_block()?;
        Ok(FnDecl { name, params, return_ty, body })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, Error> {
        let mut params = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !params.is_empty() {
                self.expect(&TokenKind::Comma)?;
            }
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_ty()?;
            params.push(Param { name, ty });
        }
        Ok(params)
    }

    fn parse_ty(&mut self) -> Result<Ty, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) => {
                let ty = match name.as_str() {
                    "i8"    => Ty::I8,
                    "i16"   => Ty::I16,
                    "i32"   => Ty::I32,
                    "i64"   => Ty::I64,
                    "isize" => Ty::Isize,
                    "u8"    => Ty::U8,
                    "u16"   => Ty::U16,
                    "u32"   => Ty::U32,
                    "u64"   => Ty::U64,
                    "usize" => Ty::Usize,
                    "bool"  => Ty::Bool,
                    _ => return Err(Error::new(tok.line, tok.col,
                        format!("unknown type '{name}'"))),
                };
                self.advance();
                Ok(ty)
            }
            TokenKind::LParen => {
                self.advance();
                self.expect(&TokenKind::RParen)?;
                Ok(Ty::Unit)
            }
            _ => Err(Error::new(tok.line, tok.col,
                format!("expected type, got {:?}", tok.kind))),
        }
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
        match &tok.kind {
            TokenKind::Let    => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If     => self.parse_if(),
            TokenKind::While  => self.parse_while(),
            // Identifier: println!, assignment `x = ...`, or call expression `f(...)`
            TokenKind::Ident(name) if name == "println" => self.parse_println(),
            TokenKind::Ident(_) => self.parse_ident_stmt(),
            _ => Err(Error::new(tok.line, tok.col,
                format!("unexpected token in statement: {:?}", tok.kind))),
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Let)?;
        let mutable = if self.peek().kind == TokenKind::Mut {
            self.advance();
            true
        } else {
            false
        };
        let name = self.expect_ident()?;
        let ty = if self.peek().kind == TokenKind::Colon {
            self.advance();
            Some(self.parse_ty()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Let { name, mutable, ty, expr })
    }

    /// Parse a statement starting with an identifier: assignment or call.
    fn parse_ident_stmt(&mut self) -> Result<Stmt, Error> {
        let name = self.expect_ident()?;
        if self.peek().kind == TokenKind::Eq {
            // assignment: `name = expr;`
            self.advance();
            let expr = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            Ok(Stmt::Assign { name, expr })
        } else if self.peek().kind == TokenKind::LParen {
            // call statement: `name(args);`
            let args = self.parse_call_args()?;
            self.expect(&TokenKind::Semicolon)?;
            Ok(Stmt::Expr(Expr::Call { name, args }))
        } else {
            let tok = self.peek().clone();
            Err(Error::new(tok.line, tok.col,
                format!("expected '=' or '(' after identifier, got {:?}", tok.kind)))
        }
    }

    fn parse_return(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Return)?;
        if self.peek().kind == TokenKind::Semicolon {
            self.advance();
            Ok(Stmt::Return(None))
        } else {
            let expr = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            Ok(Stmt::Return(Some(expr)))
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::If)?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if self.peek().kind == TokenKind::Else {
            self.advance();
            if self.peek().kind == TokenKind::If {
                // `else if ...` -> wrap in a block containing another if
                let inner = self.parse_if()?;
                Some(Block { stmts: vec![inner] })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If { cond, then_block, else_block })
    }

    fn parse_while(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
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
    // Precedence (lowest to highest):
    //   || -> && -> == != -> < > <= >= -> | -> ^ -> & -> << >> -> + - -> * / % -> unary -> primary

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_and()?;
        while self.peek().kind == TokenKind::PipePipe {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::BinOp { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_equality()?;
        while self.peek().kind == TokenKind::AmpAmp {
            self.advance();
            let rhs = self.parse_equality()?;
            lhs = Expr::BinOp { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq   => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_relational()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitor()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_bitor()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitxor()?;
        while self.peek().kind == TokenKind::Pipe {
            self.advance();
            let rhs = self.parse_bitxor()?;
            lhs = Expr::BinOp { op: BinOp::BitOr, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitand()?;
        while self.peek().kind == TokenKind::Caret {
            self.advance();
            let rhs = self.parse_bitand()?;
            lhs = Expr::BinOp { op: BinOp::BitXor, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_shift()?;
        while self.peek().kind == TokenKind::Amp {
            self.advance();
            let rhs = self.parse_shift()?;
            lhs = Expr::BinOp { op: BinOp::BitAnd, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
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
        match self.peek().kind {
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_primary()?;
                Ok(Expr::UnOp { op: UnOp::Neg, operand: Box::new(operand) })
            }
            TokenKind::Bang => {
                self.advance();
                let operand = self.parse_primary()?;
                Ok(Expr::UnOp { op: UnOp::Not, operand: Box::new(operand) })
            }
            TokenKind::Tilde => {
                self.advance();
                let operand = self.parse_primary()?;
                Ok(Expr::UnOp { op: UnOp::BitNot, operand: Box::new(operand) })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, Error> {
        self.expect(&TokenKind::LParen)?;
        let mut args = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !args.is_empty() {
                self.expect(&TokenKind::Comma)?;
            }
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::IntLit(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Int(n))
            }
            TokenKind::True  => { self.advance(); Ok(Expr::Bool(true)) }
            TokenKind::False => { self.advance(); Ok(Expr::Bool(false)) }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                if self.peek().kind == TokenKind::LParen {
                    let args = self.parse_call_args()?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Var(name))
                }
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
