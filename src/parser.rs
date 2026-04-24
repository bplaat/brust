use crate::ast::{
    BinOp, Block, EnumDecl, EnumVariant, Expr, ExprKind, FieldDecl, File, FnDecl, ImplBlock, Item,
    MatchArm, Param, Pat, PatBindings, Receiver, Stmt, StmtKind, StructDecl, TraitDecl,
    TraitMethodSig, Ty, UnOp, VariantFields,
};
use crate::error::Error;
use crate::lexer::{Token, TokenKind};
use crate::loc::Loc;
use std::collections::HashSet;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Name of the mod block currently being parsed, if any.
    current_mod: Option<String>,
    /// Names declared at the top level of the current mod block (types + fns).
    mod_local_names: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            current_mod: None,
            mod_local_names: HashSet::new(),
        }
    }

    /// Scan from `start` up to the matching `}` at depth 0 and collect the
    /// names of all top-level `fn`, `struct`, `enum`, and `type` declarations.
    fn collect_mod_local_names(&self, start: usize) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut depth: usize = 0;
        let mut i = start;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LBrace => {
                    depth += 1;
                    i += 1;
                }
                TokenKind::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    i += 1;
                }
                TokenKind::Fn | TokenKind::Struct | TokenKind::Enum | TokenKind::Type
                    if depth == 0 =>
                {
                    i += 1;
                    // Skip optional `pub`
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Pub)) {
                        i += 1;
                    }
                    if let Some(TokenKind::Ident(name)) = self.tokens.get(i).map(|t| &t.kind) {
                        names.insert(name.clone());
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        names
    }

    /// If `name` is a locally-declared name in the current module, return the
    /// fully-qualified internal name (`{mod}_{name}`). Otherwise return `name`
    /// unchanged.
    fn qualify(&self, name: String) -> String {
        if let Some(ref m) = self.current_mod
            && self.mod_local_names.contains(&name)
        {
            return format!("{m}_{name}");
        }
        name
    }

    pub fn parse(mut self) -> Result<File, Error> {
        let mut items = Vec::new();
        while !self.at_eof() {
            items.push(self.parse_item()?);
        }
        Ok(File { items })
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn loc(&self) -> Loc {
        self.tokens.get(self.pos).map(|t| t.loc).unwrap_or_default()
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

    fn expect(&mut self, kind: &TokenKind) -> Result<(), Error> {
        let tok = self.peek().clone();
        if &tok.kind == kind {
            self.advance();
            Ok(())
        } else {
            Err(Error::new(
                tok.loc,
                format!("expected {:?}, got {:?}", kind, tok.kind),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, Error> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = &tok.kind {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            Err(Error::new(
                tok.loc,
                format!("expected identifier, got {:?}", tok.kind),
            ))
        }
    }

    fn parse_item(&mut self) -> Result<Item, Error> {
        if self.peek().kind == TokenKind::Pub {
            self.advance();
        }

        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Fn => Ok(Item::Fn(self.parse_fn()?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct()?)),
            TokenKind::Impl => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Trait => Ok(Item::Trait(self.parse_trait()?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::Type => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq)?;
                let ty = self.parse_ty()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(Item::TypeAlias { name, ty })
            }
            TokenKind::Mod => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::LBrace)?;
                // Pre-scan the token stream for names declared inside this mod.
                let local_names = self.collect_mod_local_names(self.pos);
                // Save outer context and activate module scope.
                let saved_mod = self.current_mod.replace(name.clone());
                let saved_names = std::mem::replace(&mut self.mod_local_names, local_names);
                let mut items = Vec::new();
                while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                    items.push(self.parse_item()?);
                }
                self.expect(&TokenKind::RBrace)?;
                // Restore outer context.
                self.current_mod = saved_mod;
                self.mod_local_names = saved_names;
                Ok(Item::Mod { name, items })
            }
            _ => Err(Error::new(
                tok.loc,
                format!("expected item, got {:?}", tok.kind),
            )),
        }
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::Enum)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !variants.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace {
                    break;
                }
            }
            let vname = self.expect_ident()?;
            let fields = if self.peek().kind == TokenKind::LParen {
                self.advance();
                let mut tys = Vec::new();
                while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                    if !tys.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                    }
                    tys.push(self.parse_ty()?);
                }
                self.expect(&TokenKind::RParen)?;
                VariantFields::Tuple(tys)
            } else if self.peek().kind == TokenKind::LBrace {
                self.advance();
                let mut named = Vec::new();
                while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                    if !named.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                        if self.peek().kind == TokenKind::RBrace {
                            break;
                        }
                    }
                    let fname = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let fty = self.parse_ty()?;
                    named.push(FieldDecl {
                        name: fname,
                        ty: fty,
                    });
                }
                self.expect(&TokenKind::RBrace)?;
                VariantFields::Named(named)
            } else {
                VariantFields::Unit
            };
            variants.push(EnumVariant {
                name: vname,
                fields,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(EnumDecl {
            name,
            variants,
            loc,
        })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !fields.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace {
                    break;
                }
            }
            if self.peek().kind == TokenKind::Pub {
                self.advance();
            }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            fields.push(FieldDecl {
                name: fname,
                ty: self.parse_ty()?,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDecl { name, fields, loc })
    }

    fn parse_impl(&mut self) -> Result<ImplBlock, Error> {
        self.expect(&TokenKind::Impl)?;
        let first_name = self.expect_ident()?;
        // `impl Foo for Bar { ... }` vs `impl Bar { ... }`
        let (trait_name, type_name) = if self.peek().kind == TokenKind::For {
            self.advance();
            let type_name = self.expect_ident()?;
            (Some(first_name), type_name)
        } else {
            (None, first_name)
        };
        self.expect(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            methods.push(self.parse_fn()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ImplBlock { type_name, trait_name, methods })
    }

    /// Parse a trait declaration: `trait Foo { fn method(&self, ...) -> Ty; ... }`.
    fn parse_trait(&mut self) -> Result<TraitDecl, Error> {
        self.expect(&TokenKind::Trait)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if self.peek().kind == TokenKind::Pub {
                self.advance();
            }
            self.expect(&TokenKind::Fn)?;
            let mname = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let (receiver, params) = self.parse_receiver_and_params()?;
            self.expect(&TokenKind::RParen)?;
            let return_ty = if self.peek().kind == TokenKind::Arrow {
                self.advance();
                self.parse_ty()?
            } else {
                Ty::Unit
            };
            self.expect(&TokenKind::Semicolon)?;
            let receiver = receiver.unwrap_or(Receiver::Ref);
            methods.push(TraitMethodSig { name: mname, receiver, params, return_ty });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(TraitDecl { name, methods })
    }

    fn parse_fn(&mut self) -> Result<FnDecl, Error> {
        let loc = self.loc();
        if self.peek().kind == TokenKind::Pub {
            self.advance();
        }
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
        Ok(FnDecl {
            name,
            receiver,
            params,
            return_ty,
            body,
            loc,
        })
    }

    fn parse_receiver_and_params(&mut self) -> Result<(Option<Receiver>, Vec<Param>), Error> {
        let receiver = if self.peek().kind == TokenKind::SelfKw {
            self.advance();
            if self.peek().kind == TokenKind::Comma {
                self.advance();
            }
            Some(Receiver::Value)
        } else if self.peek().kind == TokenKind::Amp {
            self.advance();
            let r = if self.peek().kind == TokenKind::Mut {
                self.advance();
                Receiver::RefMut
            } else {
                Receiver::Ref
            };
            self.expect(&TokenKind::SelfKw)?;
            if self.peek().kind == TokenKind::Comma {
                self.advance();
            }
            Some(r)
        } else {
            None
        };
        Ok((receiver, self.parse_params()?))
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, Error> {
        let mut params = Vec::new();
        while self.peek().kind != TokenKind::RParen && !self.at_eof() {
            if !params.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RParen {
                    break;
                }
            }
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            params.push(Param {
                name,
                ty: self.parse_ty()?,
            });
        }
        Ok(params)
    }

    fn parse_ty(&mut self) -> Result<Ty, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Bang => {
                self.advance();
                Ok(Ty::Never)
            }
            TokenKind::Amp => {
                self.advance();
                if let TokenKind::Ident(kw) = &self.peek().kind.clone()
                    && kw == "str"
                {
                    self.advance();
                    return Ok(Ty::Str);
                }
                if self.peek().kind == TokenKind::LBracket {
                    self.advance();
                    let inner = self.parse_ty()?;
                    self.expect(&TokenKind::RBracket)?;
                    return Ok(Ty::Slice(Box::new(inner)));
                }
                if self.peek().kind == TokenKind::Mut {
                    self.advance();
                    if self.peek().kind == TokenKind::LBracket {
                        self.advance();
                        let inner = self.parse_ty()?;
                        self.expect(&TokenKind::RBracket)?;
                        return Ok(Ty::Slice(Box::new(inner)));
                    }
                    return Ok(Ty::RefMut(Box::new(self.parse_ty()?)));
                }
                Ok(Ty::Ref(Box::new(self.parse_ty()?)))
            }
            TokenKind::LBracket => {
                self.advance();
                let elem_ty = self.parse_ty()?;
                self.expect(&TokenKind::Semicolon)?;
                let n_tok = self.peek().clone();
                if let TokenKind::IntLit(n) = n_tok.kind {
                    self.advance();
                    self.expect(&TokenKind::RBracket)?;
                    Ok(Ty::Array(Box::new(elem_ty), n as usize))
                } else {
                    Err(Error::new(
                        n_tok.loc,
                        "expected integer size in array type".to_string(),
                    ))
                }
            }
            TokenKind::Fn => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let mut params = Vec::new();
                while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                    if !params.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                    }
                    if self.peek().kind == TokenKind::RParen {
                        break;
                    }
                    params.push(self.parse_ty()?);
                }
                self.expect(&TokenKind::RParen)?;
                let ret = if self.peek().kind == TokenKind::Arrow {
                    self.advance();
                    self.parse_ty()?
                } else {
                    Ty::Unit
                };
                Ok(Ty::FnPtr {
                    params,
                    ret: Box::new(ret),
                })
            }
            TokenKind::Star => {
                self.advance();
                let tok2 = self.peek().clone();
                match &tok2.kind {
                    TokenKind::Ident(kw) if kw == "const" => {
                        self.advance();
                        Ok(Ty::RawConst(Box::new(self.parse_ty()?)))
                    }
                    TokenKind::Mut => {
                        self.advance();
                        Ok(Ty::RawMut(Box::new(self.parse_ty()?)))
                    }
                    _ => Err(Error::new(
                        tok2.loc,
                        "expected `const` or `mut` after `*` in type".to_string(),
                    )),
                }
            }
            TokenKind::Dyn => {
                self.advance();
                let trait_name = self.expect_ident()?;
                Ok(Ty::DynTrait(self.qualify(trait_name)))
            }
            TokenKind::Ident(name) => {
                let ty = match name.as_str() {
                    "i8" => Ty::I8,
                    "i16" => Ty::I16,
                    "i32" => Ty::I32,
                    "i64" => Ty::I64,
                    "isize" => Ty::Isize,
                    "u8" => Ty::U8,
                    "u16" => Ty::U16,
                    "u32" => Ty::U32,
                    "u64" => Ty::U64,
                    "usize" => Ty::Usize,
                    "f32" => Ty::F32,
                    "f64" => Ty::F64,
                    "bool" => Ty::Bool,
                    "char" => Ty::Char,
                    "str" => Ty::Str,
                    name => {
                        let base = name.to_string();
                        self.advance();
                        if self.peek().kind == TokenKind::ColonColon {
                            self.advance();
                            let type_name = self.expect_ident()?;
                            return Ok(Ty::Named(format!("{base}_{type_name}")));
                        }
                        // Inside a mod block, auto-qualify locally-declared type names.
                        return Ok(Ty::Named(self.qualify(base)));
                    }
                };
                self.advance();
                Ok(ty)
            }
            TokenKind::LParen => {
                self.advance();
                if self.peek().kind == TokenKind::RParen {
                    self.advance();
                    return Ok(Ty::Unit);
                }
                let first = self.parse_ty()?;
                if self.peek().kind == TokenKind::Comma {
                    let mut tys = vec![first];
                    while self.peek().kind == TokenKind::Comma {
                        self.advance();
                        if self.peek().kind == TokenKind::RParen {
                            break;
                        }
                        tys.push(self.parse_ty()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Ty::Tuple(tys))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }
            _ => Err(Error::new(
                tok.loc,
                format!("expected type, got {:?}", tok.kind),
            )),
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
        let loc = self.loc();
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Let => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Loop => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Stmt { kind: StmtKind::Loop(body), loc })
            }
            TokenKind::For => self.parse_for(),
            TokenKind::Break => {
                self.advance();
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt { kind: StmtKind::Break, loc })
            }
            TokenKind::Continue => {
                self.advance();
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt { kind: StmtKind::Continue, loc })
            }
            TokenKind::Match => self.parse_match(),
            TokenKind::Unsafe => {
                self.advance();
                let block = self.parse_block()?;
                let expr = Expr {
                    kind: ExprKind::Unsafe(block),
                    loc,
                };
                if self.peek().kind == TokenKind::RBrace {
                    return Ok(Stmt {
                        kind: StmtKind::Return(Some(expr)),
                        loc,
                    });
                }
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt {
                    kind: StmtKind::Expr(expr),
                    loc,
                })
            }
            TokenKind::Ident(name) if name == "println" => self.parse_println(),
            TokenKind::Ident(_) | TokenKind::SelfKw => self.parse_ident_stmt(),
            _ => {
                let expr = self.parse_expr()?;
                if self.peek().kind == TokenKind::Eq {
                    self.advance();
                    let rhs = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon)?;
                    let binop_loc = expr.loc;
                    return Ok(Stmt {
                        kind: StmtKind::Expr(Expr {
                            kind: ExprKind::BinOp {
                                op: BinOp::Eq,
                                lhs: Box::new(expr),
                                rhs: Box::new(rhs),
                            },
                            loc: binop_loc,
                        }),
                        loc,
                    });
                }
                if self.peek().kind == TokenKind::RBrace {
                    return Ok(Stmt {
                        kind: StmtKind::Return(Some(expr)),
                        loc,
                    });
                }
                self.expect(&TokenKind::Semicolon)?;
                Ok(Stmt {
                    kind: StmtKind::Expr(expr),
                    loc,
                })
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
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
        Ok(Stmt {
            kind: StmtKind::Let {
                name,
                mutable,
                ty,
                expr,
            },
            loc,
        })
    }

    fn parse_ident_stmt(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        let expr = self.parse_expr()?;
        if self.peek().kind == TokenKind::Eq {
            self.advance();
            let rhs = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            if let ExprKind::Var(name) = expr.kind {
                return Ok(Stmt {
                    kind: StmtKind::Assign { name, expr: rhs },
                    loc,
                });
            } else {
                let binop_loc = expr.loc;
                return Ok(Stmt {
                    kind: StmtKind::Expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Eq,
                            lhs: Box::new(expr),
                            rhs: Box::new(rhs),
                        },
                        loc: binop_loc,
                    }),
                    loc,
                });
            }
        }
        if self.peek().kind == TokenKind::RBrace {
            return Ok(Stmt {
                kind: StmtKind::Return(Some(expr)),
                loc,
            });
        }
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt {
            kind: StmtKind::Expr(expr),
            loc,
        })
    }

    fn parse_return(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::Return)?;
        if self.peek().kind == TokenKind::Semicolon {
            self.advance();
            Ok(Stmt {
                kind: StmtKind::Return(None),
                loc,
            })
        } else {
            let expr = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            Ok(Stmt {
                kind: StmtKind::Return(Some(expr)),
                loc,
            })
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
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
        } else {
            None
        };
        Ok(Stmt {
            kind: StmtKind::If {
                cond,
                then_block,
                else_block,
            },
            loc,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt {
            kind: StmtKind::While { cond, body },
            loc,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::For)?;
        let var = self.expect_ident()?;
        self.expect(&TokenKind::In)?;
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt {
            kind: StmtKind::For { var, iter, body },
            loc,
        })
    }

    fn parse_match(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::Match)?;
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            let arm_loc = self.loc();
            let pat = self.parse_pat()?;
            self.expect(&TokenKind::FatArrow)?;
            let body = if self.peek().kind == TokenKind::LBrace {
                self.parse_block()?
            } else {
                let stmt = self.parse_arm_stmt()?;
                Block { stmts: vec![stmt] }
            };
            if self.peek().kind == TokenKind::Comma {
                self.advance();
            }
            arms.push(MatchArm {
                pat,
                body,
                loc: arm_loc,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Stmt {
            kind: StmtKind::Match { expr, arms },
            loc,
        })
    }

    fn parse_arm_stmt(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "println" => {
                self.expect_ident()?;
                self.expect(&TokenKind::Bang)?;
                self.expect(&TokenKind::LParen)?;
                let str_tok = self.peek().clone();
                let format = match &str_tok.kind {
                    TokenKind::StringLit(s) => s.clone(),
                    _ => return Err(Error::new(str_tok.loc, "println! expects a string literal")),
                };
                self.advance();
                let mut args = Vec::new();
                while self.peek().kind == TokenKind::Comma {
                    self.advance();
                    args.push(self.parse_expr()?);
                }
                self.expect(&TokenKind::RParen)?;
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt {
                    kind: StmtKind::Println { format, args },
                    loc,
                })
            }
            _ => {
                let expr = self.parse_expr()?;
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                    Ok(Stmt {
                        kind: StmtKind::Expr(expr),
                        loc,
                    })
                } else {
                    Ok(Stmt {
                        kind: StmtKind::Return(Some(expr)),
                        loc,
                    })
                }
            }
        }
    }

    fn parse_pat(&mut self) -> Result<Pat, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pat::Wildcard)
            }
            TokenKind::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()) => {
                let type_name = name.clone();
                self.advance();
                self.expect(&TokenKind::ColonColon)?;
                let part2 = self.expect_ident()?;

                let (type_name, variant) = if self.peek().kind == TokenKind::ColonColon {
                    self.advance();
                    let part3 = self.expect_ident()?;
                    // Three-part: mod::Type::Variant — qualify mod if local
                    (format!("{}_{part2}", self.qualify(type_name)), part3)
                } else {
                    // Two-part: Type::Variant — qualify Type if local
                    (self.qualify(type_name), part2)
                };
                let bindings = if self.peek().kind == TokenKind::LParen {
                    self.advance();
                    let mut binds = Vec::new();
                    while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                        if !binds.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                        }
                        binds.push(self.expect_ident()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    PatBindings::Tuple(binds)
                } else if self.peek().kind == TokenKind::LBrace {
                    self.advance();
                    let mut binds = Vec::new();
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !binds.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                        }
                        if self.peek().kind == TokenKind::RBrace {
                            break;
                        }
                        let field = self.expect_ident()?;
                        let binding = if self.peek().kind == TokenKind::Colon {
                            self.advance();
                            self.expect_ident()?
                        } else {
                            field.clone()
                        };
                        binds.push((field, binding));
                    }
                    self.expect(&TokenKind::RBrace)?;
                    PatBindings::Named(binds)
                } else {
                    PatBindings::None
                };
                Ok(Pat::EnumVariant {
                    type_name,
                    variant,
                    bindings,
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(Pat::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pat::Bool(false))
            }
            TokenKind::IntLit(n) => {
                let n = *n;
                self.advance();
                Ok(Pat::Int(n))
            }
            TokenKind::Minus => {
                self.advance();
                if let TokenKind::IntLit(n) = self.peek().kind.clone() {
                    self.advance();
                    Ok(Pat::Int(-n))
                } else {
                    Err(Error::new(tok.loc, "expected integer after `-` in pattern"))
                }
            }
            _ => Err(Error::new(
                tok.loc,
                format!("expected pattern, got {:?}", tok.kind),
            )),
        }
    }

    fn parse_println(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        self.expect_ident()?;
        self.expect(&TokenKind::Bang)?;
        self.expect(&TokenKind::LParen)?;
        let str_tok = self.peek().clone();
        let format = match &str_tok.kind {
            TokenKind::StringLit(s) => s.clone(),
            _ => return Err(Error::new(str_tok.loc, "println! expects a string literal")),
        };
        self.advance();
        let mut args = Vec::new();
        while self.peek().kind == TokenKind::Comma {
            self.advance();
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt {
            kind: StmtKind::Println { format, args },
            loc,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_and()?;
        while self.peek().kind == TokenKind::PipePipe {
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Or,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_and()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_equality()?;
        while self.peek().kind == TokenKind::AmpAmp {
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_equality()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                _ => break,
            };
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_relational()?),
                },
                loc,
            };
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
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_bitor()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitxor()?;
        while self.peek().kind == TokenKind::Pipe {
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::BitOr,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_bitxor()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitand()?;
        while self.peek().kind == TokenKind::Caret {
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::BitXor,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_bitand()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_shift()?;
        while self.peek().kind == TokenKind::Amp {
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::BitAnd,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_shift()?),
                },
                loc,
            };
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
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_additive()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_multiplicative()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_cast()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(self.parse_cast()?),
                },
                loc,
            };
        }
        Ok(lhs)
    }

    fn parse_cast(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_unary()?;
        while self.peek().kind == TokenKind::As {
            let loc = expr.loc;
            self.advance();
            let ty = self.parse_ty()?;
            expr = Expr {
                kind: ExprKind::Cast {
                    expr: Box::new(expr),
                    ty,
                },
                loc,
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        match self.peek().kind {
            TokenKind::Minus => {
                let loc = self.loc();
                self.advance();
                Ok(Expr {
                    kind: ExprKind::UnOp {
                        op: UnOp::Neg,
                        operand: Box::new(self.parse_unary()?),
                    },
                    loc,
                })
            }
            TokenKind::Bang => {
                let loc = self.loc();
                self.advance();
                Ok(Expr {
                    kind: ExprKind::UnOp {
                        op: UnOp::Not,
                        operand: Box::new(self.parse_unary()?),
                    },
                    loc,
                })
            }
            TokenKind::Tilde => {
                let loc = self.loc();
                self.advance();
                Ok(Expr {
                    kind: ExprKind::UnOp {
                        op: UnOp::BitNot,
                        operand: Box::new(self.parse_unary()?),
                    },
                    loc,
                })
            }
            TokenKind::Star => {
                let loc = self.loc();
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Deref(Box::new(self.parse_unary()?)),
                    loc,
                })
            }
            TokenKind::Amp => {
                let loc = self.loc();
                self.advance();
                let mutable = if self.peek().kind == TokenKind::Mut {
                    self.advance();
                    true
                } else {
                    false
                };
                Ok(Expr {
                    kind: ExprKind::AddrOf {
                        mutable,
                        expr: Box::new(self.parse_unary()?),
                    },
                    loc,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.peek().kind == TokenKind::Dot {
                self.advance();
                let loc = expr.loc;
                if let TokenKind::IntLit(idx) = self.peek().kind.clone() {
                    self.advance();
                    expr = Expr {
                        kind: ExprKind::Field {
                            expr: Box::new(expr),
                            field: idx.to_string(),
                        },
                        loc,
                    };
                } else {
                    let field = self.expect_ident()?;
                    if self.peek().kind == TokenKind::LParen {
                        let args = self.parse_call_args()?;
                        expr = Expr {
                            kind: ExprKind::MethodCall {
                                expr: Box::new(expr),
                                method: field,
                                args,
                            },
                            loc,
                        };
                    } else {
                        expr = Expr {
                            kind: ExprKind::Field {
                                expr: Box::new(expr),
                                field,
                            },
                            loc,
                        };
                    }
                }
            } else if self.peek().kind == TokenKind::LBracket {
                let loc = expr.loc;
                self.advance();
                let index = self.parse_expr()?;
                self.expect(&TokenKind::RBracket)?;
                expr = Expr {
                    kind: ExprKind::Index {
                        expr: Box::new(expr),
                        index: Box::new(index),
                    },
                    loc,
                };
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
            if !args.is_empty() {
                self.expect(&TokenKind::Comma)?;
            }
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let loc = self.loc();
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::IntLit(n) => {
                let n = *n;
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Int(n),
                    loc,
                })
            }
            TokenKind::FloatLit(f) => {
                let f = *f;
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Float(f),
                    loc,
                })
            }
            TokenKind::CharLit(c) => {
                let c = *c;
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Char(c),
                    loc,
                })
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Str(s),
                    loc,
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    loc,
                })
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Bool(false),
                    loc,
                })
            }
            TokenKind::SelfKw => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Var("self".to_string()),
                    loc,
                })
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while self.peek().kind != TokenKind::RBracket && !self.at_eof() {
                    if !elems.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                        if self.peek().kind == TokenKind::RBracket {
                            break;
                        }
                    }
                    elems.push(self.parse_expr()?);
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr {
                    kind: ExprKind::ArrayLit(elems),
                    loc,
                })
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();

                if self.peek().kind == TokenKind::ColonColon {
                    self.advance();
                    let part2 = self.expect_ident()?;

                    if self.peek().kind == TokenKind::ColonColon {
                        self.advance();
                        let part3 = self.expect_ident()?;
                        // Three-part path: mod::Type::method or Type::Variant::field
                        // Qualify the leading name if it is a local type name.
                        let qualified = format!("{}_{part2}", self.qualify(name));
                        if self.peek().kind == TokenKind::LParen {
                            let args = self.parse_call_args()?;
                            return Ok(Expr {
                                kind: ExprKind::AssocCall {
                                    type_name: qualified,
                                    method: part3,
                                    args,
                                },
                                loc,
                            });
                        } else if self.peek().kind == TokenKind::LBrace {
                            self.advance();
                            let mut fields = Vec::new();
                            while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                                if !fields.is_empty() {
                                    self.expect(&TokenKind::Comma)?;
                                    if self.peek().kind == TokenKind::RBrace {
                                        break;
                                    }
                                }
                                let fname = self.expect_ident()?;
                                self.expect(&TokenKind::Colon)?;
                                fields.push((fname, self.parse_expr()?));
                            }
                            self.expect(&TokenKind::RBrace)?;
                            return Ok(Expr {
                                kind: ExprKind::EnumStructLit {
                                    type_name: qualified,
                                    variant: part3,
                                    fields,
                                },
                                loc,
                            });
                        } else {
                            return Ok(Expr {
                                kind: ExprKind::AssocCall {
                                    type_name: qualified,
                                    method: part3,
                                    args: vec![],
                                },
                                loc,
                            });
                        }
                    }

                    // Two-part path: Type::method or Enum::Variant or mod::fn
                    // Qualify the leading name if it refers to a local type.
                    let type_name = self.qualify(name);
                    if self.peek().kind == TokenKind::LParen {
                        let args = self.parse_call_args()?;
                        Ok(Expr {
                            kind: ExprKind::AssocCall {
                                type_name,
                                method: part2,
                                args,
                            },
                            loc,
                        })
                    } else if self.peek().kind == TokenKind::LBrace {
                        self.advance();
                        let mut fields = Vec::new();
                        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                            if !fields.is_empty() {
                                self.expect(&TokenKind::Comma)?;
                                if self.peek().kind == TokenKind::RBrace {
                                    break;
                                }
                            }
                            let fname = self.expect_ident()?;
                            self.expect(&TokenKind::Colon)?;
                            fields.push((fname, self.parse_expr()?));
                        }
                        self.expect(&TokenKind::RBrace)?;
                        Ok(Expr {
                            kind: ExprKind::EnumStructLit {
                                type_name,
                                variant: part2,
                                fields,
                            },
                            loc,
                        })
                    } else {
                        Ok(Expr {
                            kind: ExprKind::AssocCall {
                                type_name,
                                method: part2,
                                args: vec![],
                            },
                            loc,
                        })
                    }
                } else if self.peek().kind == TokenKind::LBrace
                    && name.chars().next().is_some_and(|c| c.is_uppercase())
                {
                    // Struct literal: auto-qualify if it's a local type.
                    let name = self.qualify(name);
                    self.advance();
                    let mut fields = Vec::new();
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !fields.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if self.peek().kind == TokenKind::RBrace {
                                break;
                            }
                        }
                        let fname = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        fields.push((fname, self.parse_expr()?));
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr {
                        kind: ExprKind::StructLit { name, fields },
                        loc,
                    })
                } else if self.peek().kind == TokenKind::LParen {
                    // Function call: auto-qualify if it's a local function name.
                    let name = self.qualify(name);
                    let args = self.parse_call_args()?;
                    Ok(Expr {
                        kind: ExprKind::Call { name, args },
                        loc,
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::Var(name),
                        loc,
                    })
                }
            }
            TokenKind::LParen => {
                self.advance();
                if self.peek().kind == TokenKind::RParen {
                    self.advance();
                    return Ok(Expr {
                        kind: ExprKind::Tuple(vec![]),
                        loc,
                    });
                }
                let first = self.parse_expr()?;
                if self.peek().kind == TokenKind::Comma {
                    let mut elems = vec![first];
                    while self.peek().kind == TokenKind::Comma {
                        self.advance();
                        if self.peek().kind == TokenKind::RParen {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr {
                        kind: ExprKind::Tuple(elems),
                        loc,
                    })
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }
            TokenKind::Unsafe => {
                self.advance();
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Unsafe(block),
                    loc,
                })
            }
            _ => Err(Error::new(
                tok.loc,
                format!("expected expression, got {:?}", tok.kind),
            )),
        }
    }
}
