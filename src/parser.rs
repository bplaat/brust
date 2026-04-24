use crate::ast::{
    Block, BinOp, Expr, FieldDecl, FnDecl, File, ImplBlock, Item, Param, Receiver,
    Stmt, StructDecl, Ty, UnOp,
};
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

    fn peek_next(&self) -> &Token {
        let pos = (self.pos + 1).min(self.tokens.len() - 1);
        &self.tokens[pos]
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

    // --- grammar: items ---

    fn parse_item(&mut self) -> Result<Item, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Fn     => Ok(Item::Fn(self.parse_fn()?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct()?)),
            TokenKind::Impl   => Ok(Item::Impl(self.parse_impl()?)),
            _ => Err(Error::new(tok.line, tok.col, format!("expected item, got {:?}", tok.kind))),
        }
    }

    fn parse_struct(&mut self) -> Result<StructDecl, Error> {
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !fields.is_empty() {
                self.expect(&TokenKind::Comma)?;
                // Allow trailing comma
                if self.peek().kind == TokenKind::RBrace { break; }
            }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_ty()?;
            fields.push(FieldDecl { name: fname, ty });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDecl { name, fields })
    }

    fn parse_impl(&mut self) -> Result<ImplBlock, Error> {
        self.expect(&TokenKind::Impl)?;
        let type_name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            methods.push(self.parse_fn()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ImplBlock { type_name, methods })
    }

    fn parse_fn(&mut self) -> Result<FnDecl, Error> {
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let (receiver, params) = self.parse_receiver_and_params()?;
        self.expect(&TokenKind::RParen)?;
        let return_ty = if self.peek().kind == TokenKind::Arrow {
            self.advance();
            self.parse_ty()?
        } else {
            Ty::Unit
        };
        let body = self.parse_block()?;
        Ok(FnDecl { name, receiver, params, return_ty, body })
    }

    /// Parse optional `self`/`&self`/`&mut self` receiver then the rest of params.
    fn parse_receiver_and_params(&mut self) -> Result<(Option<Receiver>, Vec<Param>), Error> {
        // Check for self receiver
        let receiver = if self.peek().kind == TokenKind::SelfKw {
            self.advance();
            if self.peek().kind == TokenKind::Comma { self.advance(); }
            Some(Receiver::Value)
        } else if self.peek().kind == TokenKind::Amp {
            // &self or &mut self
            self.advance(); // consume &
            let r = if self.peek().kind == TokenKind::Mut {
                self.advance();
                Receiver::RefMut
            } else {
                Receiver::Ref
            };
            self.expect(&TokenKind::SelfKw)?;
            if self.peek().kind == TokenKind::Comma { self.advance(); }
            Some(r)
        } else {
            None
        };
        let params = self.parse_params()?;
        Ok((receiver, params))
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, Error> {
        let mut params = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !params.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RParen { break; }
            }
            // Skip &/&mut before param type for pass-by-ref params
            let is_ref = if self.peek().kind == TokenKind::Amp {
                self.advance();
                if self.peek().kind == TokenKind::Mut { self.advance(); }
                true
            } else {
                false
            };
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            // & before type in param
            if self.peek().kind == TokenKind::Amp {
                self.advance();
                if self.peek().kind == TokenKind::Mut { self.advance(); }
            }
            let ty = self.parse_ty()?;
            let _ = is_ref;
            params.push(Param { name, ty });
        }
        Ok(params)
    }

    fn parse_ty(&mut self) -> Result<Ty, Error> {
        // Skip & and &mut in type position (simplified - refs treated as values for now)
        if self.peek().kind == TokenKind::Amp {
            self.advance();
            if self.peek().kind == TokenKind::Mut { self.advance(); }
        }
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
                    name    => Ty::Named(name.to_string()),
                };
                self.advance();
                Ok(ty)
            }
            TokenKind::LParen => {
                self.advance();
                self.expect(&TokenKind::RParen)?;
                Ok(Ty::Unit)
            }
            _ => Err(Error::new(tok.line, tok.col, format!("expected type, got {:?}", tok.kind))),
        }
    }

    // --- grammar: statements ---

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
            TokenKind::Ident(name) if name == "println" => self.parse_println(),
            TokenKind::Ident(_) | TokenKind::SelfKw => self.parse_ident_stmt(),
            _ => Err(Error::new(tok.line, tok.col,
                format!("unexpected token in statement: {:?}", tok.kind))),
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Let)?;
        let mutable = if self.peek().kind == TokenKind::Mut {
            self.advance(); true
        } else { false };
        let name = self.expect_ident()?;
        let ty = if self.peek().kind == TokenKind::Colon {
            self.advance();
            Some(self.parse_ty()?)
        } else { None };
        self.expect(&TokenKind::Eq)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Let { name, mutable, ty, expr })
    }

    /// Parse a statement starting with an identifier or self: assignment or call.
    fn parse_ident_stmt(&mut self) -> Result<Stmt, Error> {
        // Parse a full expression (handles field access, method calls etc.)
        let expr = self.parse_expr()?;
        // If next is `=`, it's an assignment to a var or field
        if self.peek().kind == TokenKind::Eq {
            self.advance();
            let rhs = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            // Desugar: if expr is Var, emit Assign; otherwise emit a special form
            match expr {
                Expr::Var(name) => return Ok(Stmt::Assign { name, expr: rhs }),
                // Field assignment: self.x = expr → emit as expression statement for now
                other => return Ok(Stmt::Expr(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(other),
                    rhs: Box::new(rhs),
                })),
            }
        }
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr(expr))
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
                let inner = self.parse_if()?;
                Some(Block { stmts: vec![inner] })
            } else {
                Some(self.parse_block()?)
            }
        } else { None };
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
        let mut args = Vec::new();
        while self.peek().kind == TokenKind::Comma {
            self.advance();
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Println { format, args })
    }

    // --- expressions ---
    // Precedence (lowest → highest):
    //   || → && → == != → < > <= >= → | → ^ → & → << >> → + - → * / % → unary → postfix → primary

    fn parse_expr(&mut self) -> Result<Expr, Error> { self.parse_or() }

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
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_relational()?) };
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitor()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Lt => BinOp::Lt, TokenKind::Gt => BinOp::Gt,
                TokenKind::Le => BinOp::Le, TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_bitor()?) };
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitxor()?;
        while self.peek().kind == TokenKind::Pipe {
            self.advance();
            lhs = Expr::BinOp { op: BinOp::BitOr, lhs: Box::new(lhs), rhs: Box::new(self.parse_bitxor()?) };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitand()?;
        while self.peek().kind == TokenKind::Caret {
            self.advance();
            lhs = Expr::BinOp { op: BinOp::BitXor, lhs: Box::new(lhs), rhs: Box::new(self.parse_bitand()?) };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_shift()?;
        while self.peek().kind == TokenKind::Amp {
            self.advance();
            lhs = Expr::BinOp { op: BinOp::BitAnd, lhs: Box::new(lhs), rhs: Box::new(self.parse_shift()?) };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinOp::Shl, TokenKind::Shr => BinOp::Shr, _ => break,
            };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_additive()?) };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add, TokenKind::Minus => BinOp::Sub, _ => break,
            };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_multiplicative()?) };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul, TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem, _ => break,
            };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_unary()?) };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        match self.peek().kind {
            TokenKind::Minus => { self.advance(); Ok(Expr::UnOp { op: UnOp::Neg, operand: Box::new(self.parse_postfix()?) }) }
            TokenKind::Bang  => { self.advance(); Ok(Expr::UnOp { op: UnOp::Not, operand: Box::new(self.parse_postfix()?) }) }
            TokenKind::Tilde => { self.advance(); Ok(Expr::UnOp { op: UnOp::BitNot, operand: Box::new(self.parse_postfix()?) }) }
            _ => self.parse_postfix(),
        }
    }

    /// Parse postfix operations: field access `.name` and method calls `.name(args)`.
    fn parse_postfix(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.peek().kind == TokenKind::Dot {
                self.advance();
                let field = self.expect_ident()?;
                if self.peek().kind == TokenKind::LParen {
                    let args = self.parse_call_args()?;
                    expr = Expr::MethodCall { expr: Box::new(expr), method: field, args };
                } else {
                    expr = Expr::Field { expr: Box::new(expr), field };
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, Error> {
        self.expect(&TokenKind::LParen)?;
        let mut args = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !args.is_empty() { self.expect(&TokenKind::Comma)?; }
            // Skip & and &mut before args (pass-by-ref sugar)
            if self.peek().kind == TokenKind::Amp {
                self.advance();
                if self.peek().kind == TokenKind::Mut { self.advance(); }
            }
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::IntLit(n) => { let n = *n; self.advance(); Ok(Expr::Int(n)) }
            TokenKind::True      => { self.advance(); Ok(Expr::Bool(true)) }
            TokenKind::False     => { self.advance(); Ok(Expr::Bool(false)) }
            TokenKind::SelfKw   => { self.advance(); Ok(Expr::Var("self".to_string())) }

            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                if self.peek().kind == TokenKind::ColonColon {
                    // Associated call: `Type::method(args)`
                    self.advance();
                    let method = self.expect_ident()?;
                    let args = self.parse_call_args()?;
                    Ok(Expr::AssocCall { type_name: name, method, args })
                } else if self.peek().kind == TokenKind::LBrace
                    && name.chars().next().map_or(false, |c| c.is_uppercase())
                {
                    // Struct literal: `TypeName { field: expr, ... }`
                    self.advance(); // consume {
                    let mut fields = Vec::new();
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !fields.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if self.peek().kind == TokenKind::RBrace { break; }
                        }
                        let fname = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let val = self.parse_expr()?;
                        fields.push((fname, val));
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr::StructLit { name, fields })
                } else if self.peek().kind == TokenKind::LParen {
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
            _ => Err(Error::new(tok.line, tok.col, format!("expected expression, got {:?}", tok.kind))),
        }
    }
}
