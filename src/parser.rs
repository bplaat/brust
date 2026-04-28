use crate::ast::{
    Block, BinOp, EnumDecl, EnumVariant, Expr, FieldDecl, FnDecl, File, ImplBlock, Item,
    MatchArm, Param, Pat, PatBindings, Receiver, Stmt, StructDecl, Ty, UnOp, VariantFields,
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

    fn peek(&self) -> &Token { &self.tokens[self.pos] }

    fn at_eof(&self) -> bool { self.peek().kind == TokenKind::Eof }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() { self.pos += 1; }
        tok
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<(), Error> {
        let tok = self.peek().clone();
        if &tok.kind == kind { self.advance(); Ok(()) }
        else { Err(Error::new(tok.line, tok.col, format!("expected {:?}, got {:?}", kind, tok.kind))) }
    }

    fn expect_ident(&mut self) -> Result<String, Error> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = &tok.kind {
            let name = name.clone(); self.advance(); Ok(name)
        } else {
            Err(Error::new(tok.line, tok.col, format!("expected identifier, got {:?}", tok.kind)))
        }
    }

    // --- items ---

    fn parse_item(&mut self) -> Result<Item, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Fn     => Ok(Item::Fn(self.parse_fn()?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct()?)),
            TokenKind::Impl   => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Enum   => Ok(Item::Enum(self.parse_enum()?)),
            _ => Err(Error::new(tok.line, tok.col, format!("expected item, got {:?}", tok.kind))),
        }
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, Error> {
        self.expect(&TokenKind::Enum)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !variants.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace { break; }
            }
            let vname = self.expect_ident()?;
            let fields = if self.peek().kind == TokenKind::LParen {
                // Tuple variant: Variant(T0, T1)
                self.advance();
                let mut tys = Vec::new();
                while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                    if !tys.is_empty() { self.expect(&TokenKind::Comma)?; }
                    tys.push(self.parse_ty()?);
                }
                self.expect(&TokenKind::RParen)?;
                VariantFields::Tuple(tys)
            } else if self.peek().kind == TokenKind::LBrace {
                // Named struct variant: Variant { x: T, y: T }
                self.advance();
                let mut named = Vec::new();
                while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                    if !named.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                        if self.peek().kind == TokenKind::RBrace { break; }
                    }
                    let fname = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let fty = self.parse_ty()?;
                    named.push(FieldDecl { name: fname, ty: fty });
                }
                self.expect(&TokenKind::RBrace)?;
                VariantFields::Named(named)
            } else {
                VariantFields::Unit
            };
            variants.push(EnumVariant { name: vname, fields });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(EnumDecl { name, variants })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, Error> {
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !fields.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace { break; }
            }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            fields.push(FieldDecl { name: fname, ty: self.parse_ty()? });
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
            self.advance(); self.parse_ty()?
        } else { Ty::Unit };
        let body = self.parse_block()?;
        Ok(FnDecl { name, receiver, params, return_ty, body })
    }

    fn parse_receiver_and_params(&mut self) -> Result<(Option<Receiver>, Vec<Param>), Error> {
        let receiver = if self.peek().kind == TokenKind::SelfKw {
            self.advance();
            if self.peek().kind == TokenKind::Comma { self.advance(); }
            Some(Receiver::Value)
        } else if self.peek().kind == TokenKind::Amp {
            self.advance();
            let r = if self.peek().kind == TokenKind::Mut { self.advance(); Receiver::RefMut }
                    else { Receiver::Ref };
            self.expect(&TokenKind::SelfKw)?;
            if self.peek().kind == TokenKind::Comma { self.advance(); }
            Some(r)
        } else { None };
        Ok((receiver, self.parse_params()?))
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, Error> {
        let mut params = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !params.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RParen { break; }
            }
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            params.push(Param { name, ty: self.parse_ty()? });
        }
        Ok(params)
    }

    fn parse_ty(&mut self) -> Result<Ty, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Amp => {
                self.advance();
                if self.peek().kind == TokenKind::Mut { self.advance(); Ok(Ty::RefMut(Box::new(self.parse_ty()?))) }
                else { Ok(Ty::Ref(Box::new(self.parse_ty()?))) }
            }
            TokenKind::Star => {
                self.advance();
                let tok2 = self.peek().clone();
                match &tok2.kind {
                    TokenKind::Ident(kw) if kw == "const" => { self.advance(); Ok(Ty::RawConst(Box::new(self.parse_ty()?))) }
                    TokenKind::Mut => { self.advance(); Ok(Ty::RawMut(Box::new(self.parse_ty()?))) }
                    _ => Err(Error::new(tok2.line, tok2.col, "expected `const` or `mut` after `*` in type".to_string())),
                }
            }
            TokenKind::Ident(name) => {
                let ty = match name.as_str() {
                    "i8"    => Ty::I8,    "i16"   => Ty::I16,
                    "i32"   => Ty::I32,   "i64"   => Ty::I64,   "isize" => Ty::Isize,
                    "u8"    => Ty::U8,    "u16"   => Ty::U16,
                    "u32"   => Ty::U32,   "u64"   => Ty::U64,   "usize" => Ty::Usize,
                    "bool"  => Ty::Bool,
                    name    => Ty::Named(name.to_string()),
                };
                self.advance(); Ok(ty)
            }
            TokenKind::LParen => {
                self.advance();
                if self.peek().kind == TokenKind::RParen { self.advance(); return Ok(Ty::Unit); }
                let first = self.parse_ty()?;
                if self.peek().kind == TokenKind::Comma {
                    let mut tys = vec![first];
                    while self.peek().kind == TokenKind::Comma {
                        self.advance();
                        if self.peek().kind == TokenKind::RParen { break; }
                        tys.push(self.parse_ty()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Ty::Tuple(tys))
                } else { self.expect(&TokenKind::RParen)?; Ok(first) }
            }
            _ => Err(Error::new(tok.line, tok.col, format!("expected type, got {:?}", tok.kind))),
        }
    }

    // --- statements ---

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
            TokenKind::Match  => self.parse_match(),
            TokenKind::Unsafe => {
                // `unsafe { ... }` as a statement wraps the block
                self.advance();
                let block = self.parse_block()?;
                // Inline the unsafe block's stmts as a regular Expr::Unsafe to preserve tail-return
                let expr = Expr::Unsafe(block);
                if self.peek().kind == TokenKind::RBrace {
                    return Ok(Stmt::Return(Some(expr)));
                }
                if self.peek().kind == TokenKind::Semicolon { self.advance(); }
                Ok(Stmt::Expr(expr))
            }
            TokenKind::Ident(name) if name == "println" => self.parse_println(),
            TokenKind::Ident(_) | TokenKind::SelfKw => self.parse_ident_stmt(),
            // Any other expression (int literal, bool, unary, deref, tuple, ...) — may be assignment or tail return
            _ => {
                let expr = self.parse_expr()?;
                // Handle `*ptr = rhs;` and `expr.field = rhs;` assignments
                if self.peek().kind == TokenKind::Eq {
                    self.advance();
                    let rhs = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    return Ok(Stmt::Expr(Expr::BinOp {
                        op: BinOp::Eq, lhs: Box::new(expr), rhs: Box::new(rhs),
                    }));
                }
                if self.peek().kind == TokenKind::RBrace {
                    return Ok(Stmt::Return(Some(expr)));
                }
                self.expect(&TokenKind::Semicolon)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Let)?;
        let mutable = if self.peek().kind == TokenKind::Mut { self.advance(); true } else { false };
        let name = self.expect_ident()?;
        let ty = if self.peek().kind == TokenKind::Colon {
            self.advance(); Some(self.parse_ty()?)
        } else { None };
        self.expect(&TokenKind::Eq)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Let { name, mutable, ty, expr })
    }

    fn parse_ident_stmt(&mut self) -> Result<Stmt, Error> {
        let expr = self.parse_expr()?;
        if self.peek().kind == TokenKind::Eq {
            self.advance();
            let rhs = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            return match expr {
                Expr::Var(name) => Ok(Stmt::Assign { name, expr: rhs }),
                other => Ok(Stmt::Expr(Expr::BinOp {
                    op: BinOp::Eq, lhs: Box::new(other), rhs: Box::new(rhs),
                })),
            };
        }
        // Implicit tail return: last expression in a block without `;`
        if self.peek().kind == TokenKind::RBrace {
            return Ok(Stmt::Return(Some(expr)));
        }
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Expr(expr))
    }

    fn parse_return(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Return)?;
        if self.peek().kind == TokenKind::Semicolon {
            self.advance(); Ok(Stmt::Return(None))
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
            } else { Some(self.parse_block()?) }
        } else { None };
        Ok(Stmt::If { cond, then_block, else_block })
    }

    fn parse_while(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_match(&mut self) -> Result<Stmt, Error> {
        self.expect(&TokenKind::Match)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            let pat = self.parse_pat()?;
            self.expect(&TokenKind::FatArrow)?;
            let body = if self.peek().kind == TokenKind::LBrace {
                self.parse_block()?
            } else {
                let stmt = self.parse_arm_stmt()?;
                Block { stmts: vec![stmt] }
            };
            if self.peek().kind == TokenKind::Comma { self.advance(); }
            arms.push(MatchArm { pat, body });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Stmt::Match { expr, arms })
    }

    fn parse_arm_stmt(&mut self) -> Result<Stmt, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "println" => {
                self.expect_ident()?;
                self.expect(&TokenKind::Bang)?;
                self.expect(&TokenKind::LParen)?;
                let str_tok = self.peek().clone();
                let format = match &str_tok.kind {
                    TokenKind::StringLit(s) => s.clone(),
                    _ => return Err(Error::new(str_tok.line, str_tok.col,
                        "println! expects a string literal")),
                };
                self.advance();
                let mut args = Vec::new();
                while self.peek().kind == TokenKind::Comma {
                    self.advance();
                    args.push(self.parse_expr()?);
                }
                self.expect(&TokenKind::RParen)?;
                if self.peek().kind == TokenKind::Semicolon { self.advance(); }
                Ok(Stmt::Println { format, args })
            }
            _ => {
                let expr = self.parse_expr()?;
                // Single-expression match arm is always an implicit return value
                if self.peek().kind == TokenKind::Semicolon { self.advance(); Ok(Stmt::Expr(expr)) }
                else { Ok(Stmt::Return(Some(expr))) }
            }
        }
    }

    fn parse_pat(&mut self) -> Result<Pat, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "_" => { self.advance(); Ok(Pat::Wildcard) }
            TokenKind::Ident(name) if name.chars().next().map_or(false, |c| c.is_uppercase()) => {
                let type_name = name.clone();
                self.advance();
                self.expect(&TokenKind::ColonColon)?;
                let variant = self.expect_ident()?;
                let bindings = if self.peek().kind == TokenKind::LParen {
                    // Tuple bindings: Variant(a, b, _)
                    self.advance();
                    let mut binds = Vec::new();
                    while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                        if !binds.is_empty() { self.expect(&TokenKind::Comma)?; }
                        binds.push(self.expect_ident()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    PatBindings::Tuple(binds)
                } else if self.peek().kind == TokenKind::LBrace {
                    // Named bindings: Variant { x, y } or Variant { x: a, y: b }
                    self.advance();
                    let mut binds = Vec::new();
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !binds.is_empty() { self.expect(&TokenKind::Comma)?; }
                        if self.peek().kind == TokenKind::RBrace { break; }
                        let field = self.expect_ident()?;
                        let binding = if self.peek().kind == TokenKind::Colon {
                            self.advance();
                            self.expect_ident()?
                        } else {
                            field.clone() // shorthand: field name == binding name
                        };
                        binds.push((field, binding));
                    }
                    self.expect(&TokenKind::RBrace)?;
                    PatBindings::Named(binds)
                } else {
                    PatBindings::None
                };
                Ok(Pat::EnumVariant { type_name, variant, bindings })
            }
            TokenKind::True  => { self.advance(); Ok(Pat::Bool(true)) }
            TokenKind::False => { self.advance(); Ok(Pat::Bool(false)) }
            TokenKind::IntLit(n) => { let n = *n; self.advance(); Ok(Pat::Int(n)) }
            TokenKind::Minus => {
                self.advance();
                if let TokenKind::IntLit(n) = self.peek().kind.clone() {
                    self.advance(); Ok(Pat::Int(-n))
                } else {
                    Err(Error::new(tok.line, tok.col, "expected integer after `-` in pattern"))
                }
            }
            _ => Err(Error::new(tok.line, tok.col, format!("expected pattern, got {:?}", tok.kind))),
        }
    }

    fn parse_println(&mut self) -> Result<Stmt, Error> {
        self.expect_ident()?;
        self.expect(&TokenKind::Bang)?;
        self.expect(&TokenKind::LParen)?;
        let str_tok = self.peek().clone();
        let format = match &str_tok.kind {
            TokenKind::StringLit(s) => s.clone(),
            _ => return Err(Error::new(str_tok.line, str_tok.col,
                "println! expects a string literal")),
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

    fn parse_expr(&mut self) -> Result<Expr, Error> { self.parse_or() }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_and()?;
        while self.peek().kind == TokenKind::PipePipe {
            self.advance();
            lhs = Expr::BinOp { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(self.parse_and()?) };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_equality()?;
        while self.peek().kind == TokenKind::AmpAmp {
            self.advance();
            lhs = Expr::BinOp { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(self.parse_equality()?) };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek().kind { TokenKind::EqEq => BinOp::Eq, TokenKind::BangEq => BinOp::Ne, _ => break };
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
                TokenKind::Le => BinOp::Le, TokenKind::Ge => BinOp::Ge, _ => break,
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
            let op = match self.peek().kind { TokenKind::Shl => BinOp::Shl, TokenKind::Shr => BinOp::Shr, _ => break };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_additive()?) };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind { TokenKind::Plus => BinOp::Add, TokenKind::Minus => BinOp::Sub, _ => break };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_multiplicative()?) };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_cast()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul, TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem, _ => break,
            };
            self.advance();
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(self.parse_cast()?) };
        }
        Ok(lhs)
    }

    // `x as T` — higher precedence than arithmetic, lower than unary
    fn parse_cast(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_unary()?;
        while self.peek().kind == TokenKind::As {
            self.advance();
            let ty = self.parse_ty()?;
            expr = Expr::Cast { expr: Box::new(expr), ty };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        match self.peek().kind {
            TokenKind::Minus  => { self.advance(); Ok(Expr::UnOp { op: UnOp::Neg,    operand: Box::new(self.parse_unary()?) }) }
            TokenKind::Bang   => { self.advance(); Ok(Expr::UnOp { op: UnOp::Not,    operand: Box::new(self.parse_unary()?) }) }
            TokenKind::Tilde  => { self.advance(); Ok(Expr::UnOp { op: UnOp::BitNot, operand: Box::new(self.parse_unary()?) }) }
            TokenKind::Star   => { self.advance(); Ok(Expr::Deref(Box::new(self.parse_unary()?))) }
            TokenKind::Amp    => {
                self.advance();
                let mutable = if self.peek().kind == TokenKind::Mut { self.advance(); true } else { false };
                Ok(Expr::AddrOf { mutable, expr: Box::new(self.parse_unary()?) })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.peek().kind == TokenKind::Dot {
                self.advance();
                // Tuple/field index: `expr.0` → `Field { field: "0" }`
                if let TokenKind::IntLit(idx) = self.peek().kind.clone() {
                    self.advance();
                    expr = Expr::Field { expr: Box::new(expr), field: idx.to_string() };
                } else {
                    let field = self.expect_ident()?;
                    if self.peek().kind == TokenKind::LParen {
                        let args = self.parse_call_args()?;
                        expr = Expr::MethodCall { expr: Box::new(expr), method: field, args };
                    } else {
                        expr = Expr::Field { expr: Box::new(expr), field };
                    }
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
                    self.advance();
                    let method = self.expect_ident()?;
                    if self.peek().kind == TokenKind::LParen {
                        let args = self.parse_call_args()?;
                        Ok(Expr::AssocCall { type_name: name, method, args })
                    } else if self.peek().kind == TokenKind::LBrace {
                        // Struct-like enum variant: `Type::Variant { x: expr, ... }`
                        self.advance();
                        let mut fields = Vec::new();
                        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                            if !fields.is_empty() {
                                self.expect(&TokenKind::Comma)?;
                                if self.peek().kind == TokenKind::RBrace { break; }
                            }
                            let fname = self.expect_ident()?;
                            self.expect(&TokenKind::Colon)?;
                            fields.push((fname, self.parse_expr()?));
                        }
                        self.expect(&TokenKind::RBrace)?;
                        Ok(Expr::EnumStructLit { type_name: name, variant: method, fields })
                    } else {
                        Ok(Expr::AssocCall { type_name: name, method, args: vec![] })
                    }
                } else if self.peek().kind == TokenKind::LBrace
                    && name.chars().next().map_or(false, |c| c.is_uppercase())
                {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !fields.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if self.peek().kind == TokenKind::RBrace { break; }
                        }
                        let fname = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        fields.push((fname, self.parse_expr()?));
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
                if self.peek().kind == TokenKind::RParen {
                    self.advance();
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.parse_expr()?;
                if self.peek().kind == TokenKind::Comma {
                    let mut elems = vec![first];
                    while self.peek().kind == TokenKind::Comma {
                        self.advance();
                        if self.peek().kind == TokenKind::RParen { break; }
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Tuple(elems))
                } else { self.expect(&TokenKind::RParen)?; Ok(first) }
            }

            TokenKind::Unsafe => {
                self.advance();
                let block = self.parse_block()?;
                Ok(Expr::Unsafe(block))
            }

            _ => Err(Error::new(tok.line, tok.col, format!("expected expression, got {:?}", tok.kind))),
        }
    }
}
