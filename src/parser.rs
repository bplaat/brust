use crate::ast::{
    BinOp, Block, EnumDecl, EnumVariant, Expr, ExprKind, ExternFnDecl, FieldDecl, File, FnDecl,
    ImplBlock, Item, MatchArm, Param, Pat, PatBindings, Receiver, Stmt, StmtKind, StructDecl,
    TraitDecl, TraitMethodSig, Ty, UnOp, VariantFields,
};
use crate::error::Error;
use crate::lexer::{Lexer, Token, TokenKind};
use crate::loc::Loc;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

fn starts_uppercase(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

fn starts_lowercase(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_lowercase() || c == '_')
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Stack of module names, innermost last (e.g. ["math"] or ["outer", "inner"]).
    mod_stack: Vec<String>,
    /// Names declared at the top level of the current mod block (types + fns).
    mod_local_names: HashSet<String>,
    /// Use-alias table: short name -> brust internal (underscore-joined) name.
    use_aliases: HashMap<String, String>,
    /// Directory of the root source file (used to resolve `mod foo;` imports).
    source_dir: PathBuf,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source_dir: PathBuf) -> Self {
        Self {
            tokens,
            pos: 0,
            mod_stack: Vec::new(),
            mod_local_names: HashSet::new(),
            use_aliases: HashMap::new(),
            source_dir,
        }
    }

    /// Scan from `start` up to the matching `}` at depth 0 and collect the
    /// names of all top-level `fn`, `struct`, `enum`, and `type` declarations.
    fn collect_mod_local_names(&self, start: usize, end: Option<usize>) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut depth: usize = 0;
        let mut i = start;
        let limit = end.unwrap_or(self.tokens.len());
        while i < limit {
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
                TokenKind::Fn
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Type
                | TokenKind::Const
                | TokenKind::Static
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

    /// `{source_dir}/{name}/mod.rs`, lexes the content, and parses it as mod items.
    fn load_mod_file(&mut self, name: &str, loc: Loc) -> Result<Vec<Item>, Error> {
        // Candidate paths relative to current source_dir.
        let flat = self.source_dir.join(format!("{name}.rs"));
        let nested = self.source_dir.join(name).join("mod.rs");

        let (path, new_source_dir) = if flat.exists() {
            let dir = self.source_dir.clone();
            (flat, dir)
        } else if nested.exists() {
            let dir = nested.parent().unwrap().to_path_buf();
            (nested, dir)
        } else {
            return Err(Error::new(
                loc,
                format!("mod {name}: cannot find '{name}.rs' or '{name}/mod.rs'"),
            ));
        };

        let src = std::fs::read_to_string(&path).map_err(|e| {
            Error::new(
                loc.clone(),
                format!("mod {name}: cannot read '{}': {e}", path.display()),
            )
        })?;

        let mut file_tokens = Lexer::new(&src)
            .tokenize()
            .map_err(|e| Error::new(loc.clone(), format!("mod {name}: lex error: {e}")))?;

        // Remove the trailing EOF token before splicing.
        if matches!(file_tokens.last().map(|t| &t.kind), Some(TokenKind::Eof)) {
            file_tokens.pop();
        }
        let token_count = file_tokens.len();

        // Splice at current position so the parser reads them next.
        self.tokens.splice(self.pos..self.pos, file_tokens);
        let end_pos = self.pos + token_count;

        // Swap source_dir for nested mod resolution.
        let saved_dir = std::mem::replace(&mut self.source_dir, new_source_dir);

        // Pre-scan only the spliced token range to avoid reading into parent tokens.
        let local_names = self.collect_mod_local_names(self.pos, Some(end_pos));
        self.mod_stack.push(name.to_string());
        let saved_names = std::mem::replace(&mut self.mod_local_names, local_names);
        let saved_aliases = std::mem::replace(&mut self.use_aliases, HashMap::new());
        let mut items = Vec::new();
        while self.pos < end_pos {
            items.push(self.parse_item()?);
        }
        self.mod_stack.pop();
        self.mod_local_names = saved_names;
        self.use_aliases = saved_aliases;
        self.source_dir = saved_dir;

        Ok(items)
    }

    fn mod_prefix(&self) -> String {
        if self.mod_stack.is_empty() {
            String::new()
        } else {
            format!("{}_", self.mod_stack.join("_"))
        }
    }

    /// If `name` is a locally-declared name in the current module, return the
    /// fully-qualified internal name (`{mod_prefix}{name}`). Otherwise return `name`
    /// unchanged.
    fn qualify(&self, name: String) -> String {
        let prefix = self.mod_prefix();
        if !prefix.is_empty() && self.mod_local_names.contains(&name) {
            return format!("{prefix}{name}");
        }
        name
    }

    /// Resolve a `self::name` path: relative to current module.
    fn resolve_self_path(&self, name: &str) -> String {
        let prefix = self.mod_prefix();
        format!("{prefix}{name}")
    }

    /// Resolve a `super::name` path: relative to the parent module.
    fn resolve_super_path(&self, name: &str) -> String {
        if self.mod_stack.len() <= 1 {
            // Already at top level or one level deep: parent is root.
            name.to_string()
        } else {
            let parent: Vec<_> = self.mod_stack[..self.mod_stack.len() - 1]
                .iter()
                .cloned()
                .collect();
            format!("{}_{name}", parent.join("_"))
        }
    }

    /// Resolve a name: check use-aliases first, then apply module qualification.
    fn resolve_name(&self, name: String) -> String {
        if let Some(aliased) = self.use_aliases.get(&name) {
            return aliased.clone();
        }
        self.qualify(name)
    }

    /// Parse a path segment: an identifier, `self`, or `super`.
    fn parse_use_segment(&mut self) -> Result<String, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) => {
                let n = name.clone();
                self.advance();
                Ok(n)
            }
            TokenKind::SelfKw => {
                self.advance();
                Ok("self".to_string())
            }
            TokenKind::Super => {
                self.advance();
                Ok("super".to_string())
            }
            _ => Err(Error::new(
                tok.loc,
                format!("expected path segment, got {:?}", tok.kind),
            )),
        }
    }

    /// Parse a `use` path and populate `self.use_aliases`.
    /// Handles: `a::b::C`, `a::{B, C}`, `a::*`, `a::b as D`.
    fn parse_use_path(&mut self, prefix: &[String]) -> Result<(), Error> {
        let seg = self.parse_use_segment()?;
        // Strip crate-root and super markers (treat as root-relative).
        if seg == "crate" || seg == "super" || seg == "self" {
            if self.peek().kind == TokenKind::ColonColon {
                self.advance();
                return self.parse_use_path(prefix);
            }
            return Ok(());
        }
        let mut path = prefix.to_vec();
        path.push(seg);

        if self.peek().kind == TokenKind::ColonColon {
            self.advance();
            if self.peek().kind == TokenKind::LBrace {
                // Group: use a::{B, C, ...}
                self.advance();
                loop {
                    if self.peek().kind == TokenKind::RBrace {
                        break;
                    }
                    if self.peek().kind == TokenKind::Star {
                        // Glob: skip (no aliases registered)
                        self.advance();
                    } else {
                        self.parse_use_path(&path)?;
                    }
                    if self.peek().kind == TokenKind::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace)?;
            } else if self.peek().kind == TokenKind::Star {
                // Glob import: skip
                self.advance();
            } else {
                self.parse_use_path(&path)?;
            }
        } else {
            // Leaf segment: register alias.
            let alias = if self.peek().kind == TokenKind::As {
                self.advance();
                self.expect_ident()?
            } else {
                path.last().unwrap().clone()
            };
            let target = path.join("_");
            self.use_aliases.insert(alias, target);
        }
        Ok(())
    }

    pub fn parse_file(&mut self) -> Result<File, Error> {
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
        let is_pub = self.peek().kind == TokenKind::Pub;
        if is_pub {
            self.advance();
        }

        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Fn => Ok(Item::Fn(self.parse_fn(is_pub)?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct(is_pub)?)),
            TokenKind::Impl => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Trait => Ok(Item::Trait(self.parse_trait(is_pub)?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum(is_pub)?)),
            // `unsafe fn` -- unsafe modifier on fn decl is accepted but ignored
            // `unsafe extern "C" { ... }` -- extern block with unsafe prefix
            TokenKind::Unsafe => {
                self.advance();
                let next = self.peek().clone();
                match next.kind {
                    TokenKind::Fn => Ok(Item::Fn(self.parse_fn(is_pub)?)),
                    TokenKind::Extern => {
                        self.advance();
                        self.parse_extern_block(next.loc, is_pub, true)
                    }
                    _ => Err(Error::new(
                        next.loc,
                        format!(
                            "expected `fn` or `extern` after `unsafe`, got {:?}",
                            next.kind
                        ),
                    )),
                }
            }
            TokenKind::Type => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq)?;
                let ty = self.parse_ty()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(Item::TypeAlias { name, ty, is_pub })
            }
            TokenKind::Const => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let ty = self.parse_ty()?;
                self.expect(&TokenKind::Eq)?;
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(Item::Const {
                    name,
                    ty,
                    expr,
                    is_pub,
                })
            }
            TokenKind::Static => {
                self.advance();
                let mutable = if self.peek().kind == TokenKind::Mut {
                    self.advance();
                    true
                } else {
                    false
                };
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let ty = self.parse_ty()?;
                self.expect(&TokenKind::Eq)?;
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(Item::Static {
                    name,
                    ty,
                    expr,
                    mutable,
                    is_pub,
                })
            }
            TokenKind::Use => {
                self.advance();
                self.parse_use_path(&[])?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(Item::Skip)
            }
            TokenKind::Extern => {
                self.advance();
                self.parse_extern_block(tok.loc, is_pub, false)
            }
            TokenKind::Mod => {
                self.advance();
                let name = self.expect_ident()?;

                // mod foo; -- load from file
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                    let items = self.load_mod_file(&name, tok.loc)?;
                    return Ok(Item::Mod {
                        name,
                        items,
                        is_pub,
                    });
                }

                self.expect(&TokenKind::LBrace)?;
                // Pre-scan the token stream for names declared inside this mod.
                let local_names = self.collect_mod_local_names(self.pos, None);
                // Push onto the module stack and parse items.
                self.mod_stack.push(name.clone());
                let saved_names = std::mem::replace(&mut self.mod_local_names, local_names);
                let saved_aliases = std::mem::replace(&mut self.use_aliases, HashMap::new());
                let mut items = Vec::new();
                while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                    items.push(self.parse_item()?);
                }
                self.expect(&TokenKind::RBrace)?;
                // Restore outer context.
                self.mod_stack.pop();
                self.mod_local_names = saved_names;
                self.use_aliases = saved_aliases;
                Ok(Item::Mod {
                    name,
                    items,
                    is_pub,
                })
            }
            _ => Err(Error::new(
                tok.loc.clone(),
                format!("expected item, got {:?}", tok.kind),
            )),
        }
    }

    /// Parse `unsafe extern "C" { fn ...; }` / `extern crate ...;`.
    /// Called after consuming `extern` (or `unsafe extern`).
    /// `is_unsafe` is true when `unsafe` preceded the `extern` keyword.
    /// Bare `extern "C" { }` without `unsafe` is rejected (edition 2024 rule).
    fn parse_extern_block(
        &mut self,
        loc: Loc,
        _is_pub: bool,
        is_unsafe: bool,
    ) -> Result<Item, Error> {
        // Handle: extern crate foo; / extern crate foo as bar;
        if matches!(self.peek().kind, TokenKind::Ident(ref s) if s == "crate") {
            // Skip to semicolon
            while !matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Eof) {
                self.advance();
            }
            if self.peek().kind == TokenKind::Semicolon {
                self.advance();
            }
            return Ok(Item::Skip);
        }

        // Must be `"C"` (ABI string)
        match &self.peek().kind {
            TokenKind::StringLit(abi) if abi == "C" => {
                self.advance();
            }
            TokenKind::StringLit(abi) => {
                let abi = abi.clone();
                return Err(Error::new(
                    loc,
                    format!("unsupported ABI `\"{abi}\"`: only \"C\" is supported"),
                ));
            }
            // Not a string: skip the rest of the unknown extern form
            _ => {
                while !matches!(
                    self.peek().kind,
                    TokenKind::Semicolon | TokenKind::LBrace | TokenKind::Eof
                ) {
                    self.advance();
                }
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                    return Ok(Item::Skip);
                }
                // Consume brace block
                if self.peek().kind == TokenKind::LBrace {
                    self.advance();
                    let mut depth = 1usize;
                    loop {
                        match self.peek().kind.clone() {
                            TokenKind::LBrace => {
                                depth += 1;
                                self.advance();
                            }
                            TokenKind::RBrace => {
                                depth -= 1;
                                self.advance();
                                if depth == 0 {
                                    break;
                                }
                            }
                            TokenKind::Eof => break,
                            _ => {
                                self.advance();
                            }
                        }
                    }
                }
                return Ok(Item::Skip);
            }
        }

        // Extern "C" blocks must be at top level (C symbols must not be mangled).
        if !self.mod_stack.is_empty() {
            return Err(Error::new(
                loc,
                "`extern \"C\"` blocks must be at the top level, not inside a module".to_string(),
            ));
        }

        // Edition 2024: extern "C" blocks must be declared `unsafe extern "C"`.
        if !is_unsafe {
            return Err(Error::new(loc, "`extern \"C\"` block must be declared `unsafe extern \"C\"` to acknowledge that calling C functions is unsafe".to_string()));
        }

        self.expect(&TokenKind::LBrace)?;
        let mut fns = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            let fn_loc = self.loc();
            // Optional `pub` or `unsafe` modifiers on individual fn decls are accepted but ignored
            while matches!(self.peek().kind, TokenKind::Pub | TokenKind::Unsafe) {
                self.advance();
            }
            self.expect(&TokenKind::Fn)?;
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let mut params = Vec::new();
            let mut is_variadic = false;
            while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                if !params.is_empty() {
                    self.expect(&TokenKind::Comma)?;
                    if self.peek().kind == TokenKind::RParen {
                        break;
                    }
                }
                // Variadic marker `...`
                if self.peek().kind == TokenKind::DotDotDot {
                    self.advance();
                    is_variadic = true;
                    // `...` must be last; break after consuming it
                    break;
                }
                // Optional `_:` or `name:` pattern (brust uses simple ident params)
                let pname = self.expect_ident()?;
                // Optional `_` as param name followed by colon, or just a type
                if self.peek().kind == TokenKind::Colon {
                    self.advance();
                    let ty = self.parse_ty()?;
                    params.push(Param { name: pname, ty });
                } else {
                    // Unnamed param: what we read was actually the type name -- re-parse
                    // Treat single ident without colon as a named type param with name "_"
                    let ty = Ty::Named(self.qualify(pname));
                    params.push(Param {
                        name: "_".to_string(),
                        ty,
                    });
                }
            }
            self.expect(&TokenKind::RParen)?;
            let return_ty = if self.peek().kind == TokenKind::Arrow {
                self.advance();
                self.parse_ty()?
            } else {
                Ty::Unit
            };
            self.expect(&TokenKind::Semicolon)?;
            fns.push(ExternFnDecl {
                name,
                params,
                return_ty,
                is_variadic,
                loc: fn_loc,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Item::ExternBlock(fns))
    }

    fn parse_enum(&mut self, is_pub: bool) -> Result<EnumDecl, Error> {
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
                    let field_pub = self.peek().kind == TokenKind::Pub;
                    if field_pub {
                        self.advance();
                    }
                    let fname = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let fty = self.parse_ty()?;
                    named.push(FieldDecl {
                        name: fname,
                        ty: fty,
                        is_pub: field_pub,
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
            is_pub,
            loc,
        })
    }

    fn parse_struct(&mut self, is_pub: bool) -> Result<StructDecl, Error> {
        let loc = self.loc();
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;

        // Unit struct: `struct Foo;`
        if self.peek().kind == TokenKind::Semicolon {
            self.advance();
            return Ok(StructDecl {
                name,
                fields: Vec::new(),
                is_pub,
                is_tuple: false,
                loc,
            });
        }

        // Tuple struct: `struct Foo(T0, T1, ...);`
        if self.peek().kind == TokenKind::LParen {
            self.advance();
            let mut fields = Vec::new();
            while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                if !fields.is_empty() {
                    self.expect(&TokenKind::Comma)?;
                    if self.peek().kind == TokenKind::RParen {
                        break;
                    }
                }
                let field_pub = self.peek().kind == TokenKind::Pub;
                if field_pub {
                    self.advance();
                }
                let ty = self.parse_ty()?;
                fields.push(FieldDecl {
                    name: format!("_{}", fields.len()),
                    ty,
                    is_pub: field_pub,
                });
            }
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::Semicolon)?;
            return Ok(StructDecl {
                name,
                fields,
                is_pub,
                is_tuple: true,
                loc,
            });
        }

        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !fields.is_empty() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace {
                    break;
                }
            }
            let field_pub = self.peek().kind == TokenKind::Pub;
            if field_pub {
                self.advance();
            }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            fields.push(FieldDecl {
                name: fname,
                ty: self.parse_ty()?,
                is_pub: field_pub,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDecl {
            name,
            fields,
            is_pub,
            is_tuple: false,
            loc,
        })
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
            // Parse per-method pub visibility.
            let method_pub = self.peek().kind == TokenKind::Pub;
            if method_pub {
                self.advance();
            }
            methods.push(self.parse_fn(method_pub)?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ImplBlock {
            type_name,
            trait_name,
            methods,
        })
    }

    /// Parse a trait declaration: `trait Foo { fn method(&self, ...) -> Ty; ... }`.
    fn parse_trait(&mut self, is_pub: bool) -> Result<TraitDecl, Error> {
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
            let body = if self.peek().kind == TokenKind::LBrace {
                Some(Rc::new(self.parse_block()?))
            } else {
                self.expect(&TokenKind::Semicolon)?;
                None
            };
            let receiver = receiver.unwrap_or(Receiver::Ref);
            methods.push(TraitMethodSig {
                name: mname,
                receiver,
                params,
                return_ty,
                body,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(TraitDecl {
            name,
            methods,
            is_pub,
        })
    }

    fn parse_fn(&mut self, is_pub: bool) -> Result<FnDecl, Error> {
        let loc = self.loc();
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
            is_pub,
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
                    TokenKind::Const => {
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
                Ok(Ty::DynTrait(self.resolve_name(trait_name)))
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
                    "Self" => {
                        self.advance();
                        return Ok(Ty::SelfTy);
                    }
                    name => {
                        let base = name.to_string();
                        self.advance();
                        if self.peek().kind == TokenKind::ColonColon {
                            self.advance();
                            let type_name = self.expect_ident()?;
                            // Handle additional segments (e.g. std::fmt::Display).
                            let mut qualified = format!("{base}_{type_name}");
                            while self.peek().kind == TokenKind::ColonColon {
                                self.advance();
                                let next = self.expect_ident()?;
                                qualified = format!("{qualified}_{next}");
                            }
                            // Apply alias lookup on the fully-qualified name's last segment.
                            return Ok(Ty::Named(qualified));
                        }
                        // Check use-alias table, then apply module qualification.
                        return Ok(Ty::Named(self.resolve_name(base)));
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
            // self::Type and super::Type: module-relative paths.
            TokenKind::SelfKw => {
                self.advance();
                self.expect(&TokenKind::ColonColon)?;
                let name = self.expect_ident()?;
                Ok(Ty::Named(self.resolve_self_path(&name)))
            }
            TokenKind::Super => {
                self.advance();
                self.expect(&TokenKind::ColonColon)?;
                let name = self.expect_ident()?;
                Ok(Ty::Named(self.resolve_super_path(&name)))
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
                Ok(Stmt {
                    kind: StmtKind::Loop(body),
                    loc,
                })
            }
            TokenKind::For => self.parse_for(),
            TokenKind::Break => {
                self.advance();
                // `break val` — optional value expression
                let val = if self.peek().kind != TokenKind::Semicolon
                    && self.peek().kind != TokenKind::RBrace
                    && !self.at_eof()
                {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt {
                    kind: StmtKind::Break(val),
                    loc,
                })
            }
            TokenKind::Continue => {
                self.advance();
                if self.peek().kind == TokenKind::Semicolon {
                    self.advance();
                }
                Ok(Stmt {
                    kind: StmtKind::Continue,
                    loc,
                })
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
            TokenKind::Ident(name)
                if matches!(name.as_str(), "println" | "print" | "eprintln" | "eprint")
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .map(|t| t.kind == TokenKind::Bang)
                        .unwrap_or(false) =>
            {
                self.parse_println()
            }
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

        // Check for pattern syntax: `let (a, b) = ...` or `let Point(x, y) = ...`
        let is_tuple_pat = self.peek().kind == TokenKind::LParen;
        let is_tuple_struct_pat = matches!(&self.peek().kind,
            TokenKind::Ident(n) if starts_uppercase(n));

        if (is_tuple_pat || is_tuple_struct_pat) && !mutable {
            // Parse a full pattern
            let pat = self.parse_single_pat()?;
            let ty = if self.peek().kind == TokenKind::Colon {
                self.advance();
                Some(self.parse_ty()?)
            } else {
                None
            };
            self.expect(&TokenKind::Eq)?;
            let expr = self.parse_expr()?;
            // `let pat = expr else { ... };`
            if self.peek().kind == TokenKind::Else {
                self.advance();
                let else_block = self.parse_block()?;
                self.expect(&TokenKind::Semicolon)?;
                return Ok(Stmt {
                    kind: StmtKind::LetPat {
                        pat,
                        ty,
                        expr,
                        else_block: Some(else_block),
                    },
                    loc,
                });
            }
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt {
                kind: StmtKind::LetPat {
                    pat,
                    ty,
                    expr,
                    else_block: None,
                },
                loc,
            });
        }

        let name = self.expect_ident()?;
        let ty = if self.peek().kind == TokenKind::Colon {
            self.advance();
            Some(self.parse_ty()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq)?;
        let expr = self.parse_expr()?;

        // `let name = expr else { ... };` -- binding let-else (refutable pattern)
        if self.peek().kind == TokenKind::Else {
            self.advance();
            let else_block = self.parse_block()?;
            self.expect(&TokenKind::Semicolon)?;
            return Ok(Stmt {
                kind: StmtKind::LetPat {
                    pat: Pat::Binding(name),
                    ty,
                    expr,
                    else_block: Some(else_block),
                },
                loc,
            });
        }

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
        // Plain assignment: `lhs = rhs;`
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
        // Compound assignment: `name op= rhs;`  Desugar to `name = name op rhs`.
        let compound_op = match self.peek().kind {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Rem),
            TokenKind::AmpEq => Some(BinOp::BitAnd),
            TokenKind::PipeEq => Some(BinOp::BitOr),
            TokenKind::CaretEq => Some(BinOp::BitXor),
            TokenKind::ShlEq => Some(BinOp::Shl),
            TokenKind::ShrEq => Some(BinOp::Shr),
            _ => None,
        };
        if let Some(op) = compound_op {
            self.advance();
            let rhs = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon)?;
            let expr_loc = expr.loc;
            // For simple variable targets, desugar `x op= rhs` → `x = x op rhs`.
            if let ExprKind::Var(ref name) = expr.kind {
                let lhs_copy = Expr {
                    kind: ExprKind::Var(name.clone()),
                    loc: expr_loc,
                };
                let desugared_rhs = Expr {
                    kind: ExprKind::BinOp {
                        op,
                        lhs: Box::new(lhs_copy),
                        rhs: Box::new(rhs),
                    },
                    loc: expr_loc,
                };
                let ExprKind::Var(name) = expr.kind else {
                    unreachable!()
                };
                return Ok(Stmt {
                    kind: StmtKind::Assign {
                        name,
                        expr: desugared_rhs,
                    },
                    loc,
                });
            }
            // Complex lvalue (arr[i], obj.field): emit as `lhs op= rhs` in C via CompoundAssign.
            return Ok(Stmt {
                kind: StmtKind::CompoundAssign { op, lhs: expr, rhs },
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
        // `if let pat = expr [&& extra] { ... } [else { ... }]`
        if self.peek().kind == TokenKind::Let {
            self.advance();
            let pat = self.parse_pat()?;
            self.expect(&TokenKind::Eq)?;
            let expr = self.parse_if_let_expr()?;
            let and_cond = if self.peek().kind == TokenKind::AmpAmp {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
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
            return Ok(Stmt {
                kind: StmtKind::IfLet {
                    pat,
                    expr,
                    expr_ty: None,
                    and_cond,
                    then_block,
                    else_block,
                },
                loc,
            });
        }
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
        // `while let pat = expr { body }`
        if self.peek().kind == TokenKind::Let {
            self.advance();
            let pat = self.parse_pat()?;
            self.expect(&TokenKind::Eq)?;
            let expr = self.parse_expr()?;
            let body = self.parse_block()?;
            return Ok(Stmt {
                kind: StmtKind::WhileLet {
                    pat,
                    expr,
                    expr_ty: None,
                    body,
                },
                loc,
            });
        }
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
            kind: StmtKind::For {
                var,
                iter,
                body,
                elem_ty: None,
                iter_ty: None,
            },
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
            // Optional match guard: `if expr`
            let guard = if self.peek().kind == TokenKind::If {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
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
                guard,
                body,
                loc: arm_loc,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Stmt {
            kind: StmtKind::Match {
                expr,
                arms,
                scrutinee_ty: None,
            },
            loc,
        })
    }

    fn parse_arm_stmt(&mut self) -> Result<Stmt, Error> {
        let loc = self.loc();
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name)
                if matches!(name.as_str(), "println" | "print" | "eprintln" | "eprint")
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .map(|t| t.kind == TokenKind::Bang)
                        .unwrap_or(false) =>
            {
                let macro_name = self.expect_ident()?;
                let newline = macro_name == "println" || macro_name == "eprintln";
                let stderr = macro_name == "eprintln" || macro_name == "eprint";
                self.expect(&TokenKind::Bang)?;
                self.expect(&TokenKind::LParen)?;
                let str_tok = self.peek().clone();
                let format = match &str_tok.kind {
                    TokenKind::StringLit(s) => s.clone(),
                    _ => {
                        return Err(Error::new(
                            str_tok.loc,
                            format!("{macro_name}! expects a string literal"),
                        ));
                    }
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
                    kind: StmtKind::Println {
                        format,
                        args,
                        newline,
                        stderr,
                    },
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
        let first = self.parse_single_pat()?;
        // Or-pattern: `pat1 | pat2 | ...`
        if self.peek().kind == TokenKind::Pipe {
            let mut alternatives = vec![first];
            while self.peek().kind == TokenKind::Pipe {
                self.advance();
                alternatives.push(self.parse_single_pat()?);
            }
            Ok(Pat::Or(alternatives))
        } else {
            Ok(first)
        }
    }

    fn parse_single_pat(&mut self) -> Result<Pat, Error> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pat::Wildcard)
            }
            TokenKind::Ident(name) if starts_lowercase(name) => {
                // Binding pattern: `x` or `x @ sub_pat`
                let name = name.clone();
                self.advance();
                if self.peek().kind == TokenKind::At {
                    self.advance();
                    let sub = self.parse_single_pat()?;
                    Ok(Pat::At { name, pat: Box::new(sub) })
                } else {
                    Ok(Pat::Binding(name))
                }
            }
            TokenKind::Ident(name) if starts_uppercase(name) => {
                let type_name = name.clone();
                self.advance();

                // Tuple-struct pattern: `Point(x, y)` -- no `::` before `(`
                if self.peek().kind == TokenKind::LParen {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                        if !fields.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if self.peek().kind == TokenKind::RParen {
                                break;
                            }
                        }
                        fields.push(self.parse_single_pat()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Pat::TupleStruct {
                        type_name: self.resolve_name(type_name),
                        fields,
                    });
                }

                // Struct pattern: `Point { x, y, .. }` -- `{` directly after name
                if self.peek().kind == TokenKind::LBrace {
                    self.advance();
                    let mut binds = Vec::new();
                    let mut has_rest = false;
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !binds.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                        }
                        if self.peek().kind == TokenKind::RBrace {
                            break;
                        }
                        if self.peek().kind == TokenKind::DotDot {
                            self.advance();
                            has_rest = true;
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
                    // Emit as EnumVariant with the struct name as both type and "variant" (unit).
                    // For plain struct patterns, use type_name as variant with named bindings.
                    let resolved = self.resolve_name(type_name.clone());
                    return Ok(Pat::EnumVariant {
                        type_name: resolved.clone(),
                        variant: resolved,
                        bindings: PatBindings::Named(binds, has_rest),
                    });
                }

                self.expect(&TokenKind::ColonColon)?;
                let part2 = self.expect_ident()?;

                let (type_name, variant) = if self.peek().kind == TokenKind::ColonColon {
                    self.advance();
                    let part3 = self.expect_ident()?;
                    // Three-part: mod::Type::Variant -- resolve mod if local/aliased
                    (format!("{}_{part2}", self.resolve_name(type_name)), part3)
                } else {
                    // Two-part: Type::Variant -- resolve Type if local/aliased
                    (self.resolve_name(type_name), part2)
                };
                let bindings = if self.peek().kind == TokenKind::LParen {
                    self.advance();
                    let mut binds = Vec::new();
                    while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                        if !binds.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                            if self.peek().kind == TokenKind::RParen {
                                break;
                            }
                        }
                        binds.push(self.parse_single_pat()?);
                    }
                    self.expect(&TokenKind::RParen)?;
                    PatBindings::Tuple(binds)
                } else if self.peek().kind == TokenKind::LBrace {
                    self.advance();
                    let mut binds = Vec::new();
                    let mut has_rest = false;
                    while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                        if !binds.is_empty() {
                            self.expect(&TokenKind::Comma)?;
                        }
                        if self.peek().kind == TokenKind::RBrace {
                            break;
                        }
                        // `..` rest pattern inside named bindings
                        if self.peek().kind == TokenKind::DotDot {
                            self.advance();
                            has_rest = true;
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
                    PatBindings::Named(binds, has_rest)
                } else {
                    PatBindings::None
                };
                Ok(Pat::EnumVariant {
                    type_name,
                    variant,
                    bindings,
                })
            }
            // Tuple pattern: `(a, b, ...)`
            TokenKind::LParen => {
                self.advance();
                let mut pats = Vec::new();
                while self.peek().kind != TokenKind::RParen && !self.at_eof() {
                    if !pats.is_empty() {
                        self.expect(&TokenKind::Comma)?;
                        if self.peek().kind == TokenKind::RParen {
                            break;
                        }
                    }
                    pats.push(self.parse_single_pat()?);
                }
                self.expect(&TokenKind::RParen)?;
                Ok(Pat::Tuple(pats))
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
                // Range pattern: `lo..=hi`
                if self.peek().kind == TokenKind::DotDotEq {
                    self.advance();
                    if let TokenKind::IntLit(hi) = self.peek().kind.clone() {
                        self.advance();
                        return Ok(Pat::Range { lo: n, hi });
                    }
                    return Err(Error::new(
                        tok.loc,
                        "expected integer after `..=` in range pattern",
                    ));
                }
                Ok(Pat::Int(n))
            }
            TokenKind::CharLit(c) => {
                let c = *c;
                self.advance();
                // `'a'..='z'` char range pattern
                if self.peek().kind == TokenKind::DotDotEq {
                    self.advance();
                    if let TokenKind::CharLit(hi) = self.peek().kind.clone() {
                        self.advance();
                        return Ok(Pat::CharRange { lo: c, hi });
                    }
                    return Err(Error::new(tok.loc, "expected char after `..=` in char range pattern"));
                }
                Ok(Pat::Char(c))
            }
            TokenKind::Minus => {
                self.advance();
                if let TokenKind::IntLit(n) = self.peek().kind.clone() {
                    self.advance();
                    let lo = -n;
                    // Range pattern starting with negative: `-lo..=hi`
                    if self.peek().kind == TokenKind::DotDotEq {
                        self.advance();
                        if let TokenKind::IntLit(hi) = self.peek().kind.clone() {
                            self.advance();
                            return Ok(Pat::Range { lo, hi });
                        }
                        return Err(Error::new(
                            tok.loc,
                            "expected integer after `..=` in range pattern",
                        ));
                    }
                    Ok(Pat::Int(lo))
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
        let macro_name = self.expect_ident()?;
        let newline = macro_name == "println" || macro_name == "eprintln";
        let stderr = macro_name == "eprintln" || macro_name == "eprint";
        self.expect(&TokenKind::Bang)?;
        self.expect(&TokenKind::LParen)?;
        let str_tok = self.peek().clone();
        let format = match &str_tok.kind {
            TokenKind::StringLit(s) => s.clone(),
            _ => {
                return Err(Error::new(
                    str_tok.loc,
                    format!("{macro_name}! expects a string literal"),
                ));
            }
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
            kind: StmtKind::Println {
                format,
                args,
                newline,
                stderr,
            },
            loc,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.parse_range()
    }

    /// Parse a range expression: `lo..hi`, `lo..`, `..hi`, `..`
    /// This is the lowest-precedence binary form (below `||`).
    fn parse_range(&mut self) -> Result<Expr, Error> {
        // Leading `..` with optional end.
        if self.peek().kind == TokenKind::DotDot {
            let loc = self.loc();
            self.advance();
            let end = if self.peek().kind != TokenKind::RBracket
                && self.peek().kind != TokenKind::Semicolon
                && self.peek().kind != TokenKind::Comma
                && self.peek().kind != TokenKind::RBrace
                && self.peek().kind != TokenKind::RParen
            {
                Some(Box::new(self.parse_or()?))
            } else {
                None
            };
            return Ok(Expr {
                kind: ExprKind::Range { start: None, end },
                loc,
            });
        }
        let lhs = self.parse_or()?;
        if self.peek().kind == TokenKind::DotDot {
            let loc = lhs.loc;
            self.advance();
            // Optional end expression (absent in `lo..`).
            let end = if self.peek().kind != TokenKind::RBracket
                && self.peek().kind != TokenKind::Semicolon
                && self.peek().kind != TokenKind::Comma
                && self.peek().kind != TokenKind::RBrace
                && self.peek().kind != TokenKind::RParen
            {
                Some(Box::new(self.parse_or()?))
            } else {
                None
            };
            return Ok(Expr {
                kind: ExprKind::Range {
                    start: Some(Box::new(lhs)),
                    end,
                },
                loc,
            });
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_and, |t| {
            matches!(t, TokenKind::PipePipe).then_some(BinOp::Or)
        })
    }

    /// Like `parse_or` but does NOT consume `&&` (stops before it).
    /// Used for the scrutinee expression in `if let pat = expr && cond`.
    fn parse_if_let_expr(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_equality, |t| {
            matches!(t, TokenKind::PipePipe).then_some(BinOp::Or)
        })
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_equality, |t| {
            matches!(t, TokenKind::AmpAmp).then_some(BinOp::And)
        })
    }

    fn parse_equality(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_relational, |t| match t {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::BangEq => Some(BinOp::Ne),
            _ => None,
        })
    }

    fn parse_relational(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_bitor, |t| match t {
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        })
    }

    fn parse_bitor(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_bitxor, |t| {
            matches!(t, TokenKind::Pipe).then_some(BinOp::BitOr)
        })
    }

    fn parse_bitxor(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_bitand, |t| {
            matches!(t, TokenKind::Caret).then_some(BinOp::BitXor)
        })
    }

    fn parse_bitand(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_shift, |t| {
            matches!(t, TokenKind::Amp).then_some(BinOp::BitAnd)
        })
    }

    fn parse_shift(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_additive, |t| match t {
            TokenKind::Shl => Some(BinOp::Shl),
            TokenKind::Shr => Some(BinOp::Shr),
            _ => None,
        })
    }

    fn parse_additive(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_multiplicative, |t| match t {
            TokenKind::Plus => Some(BinOp::Add),
            TokenKind::Minus => Some(BinOp::Sub),
            _ => None,
        })
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Error> {
        self.parse_binop_left_assoc(Self::parse_cast, |t| match t {
            TokenKind::Star => Some(BinOp::Mul),
            TokenKind::Slash => Some(BinOp::Div),
            TokenKind::Percent => Some(BinOp::Rem),
            _ => None,
        })
    }

    /// Build a left-associative binary expression chain.
    /// `next` parses the next-higher-precedence level; `to_op` maps a token to
    /// its `BinOp` (returning `None` means "stop the loop").
    fn parse_binop_left_assoc(
        &mut self,
        next: fn(&mut Self) -> Result<Expr, Error>,
        to_op: fn(&TokenKind) -> Option<BinOp>,
    ) -> Result<Expr, Error> {
        let mut lhs = next(self)?;
        loop {
            let Some(op) = to_op(&self.peek().kind) else {
                break;
            };
            let loc = lhs.loc;
            self.advance();
            lhs = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(next(self)?),
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

    /// Build an expression from an already-resolved path (from self:: or super::).
    /// Handles struct literals (LBrace), function calls (LParen), and plain variable references.
    fn parse_resolved_path_expr(&mut self, name: String, loc: Loc) -> Result<Expr, Error> {
        if self.peek().kind == TokenKind::LParen {
            let args = self.parse_call_args()?;
            return Ok(Expr {
                kind: ExprKind::Call { name, args },
                loc,
            });
        }
        // Check uppercase using the last underscore-delimited segment, since resolved
        // names are mangled (e.g., "geom_Point" — the type segment is "Point").
        let last_seg = name.rsplit('_').next().unwrap_or(&name);
        if self.peek().kind == TokenKind::LBrace && starts_uppercase(last_seg) {
            let (fields, rest) = self.parse_struct_lit_body()?;
            return Ok(Expr {
                kind: ExprKind::StructLit { name, fields, rest },
                loc,
            });
        }
        Ok(Expr {
            kind: ExprKind::Var(name),
            loc,
        })
    }

    /// Parse `{ field: expr, ..., [..rest] }` — the body of a struct literal.
    /// Returns `(fields, rest)` where `rest` is the optional base expression.
    fn parse_struct_lit_body(&mut self) -> Result<(Vec<(String, Expr)>, Option<Box<Expr>>), Error> {
        self.advance(); // consume `{`
        let mut fields = Vec::new();
        let mut rest = None;
        while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
            if !fields.is_empty() || rest.is_some() {
                self.expect(&TokenKind::Comma)?;
                if self.peek().kind == TokenKind::RBrace {
                    break;
                }
            }
            if self.peek().kind == TokenKind::DotDot {
                self.advance();
                rest = Some(Box::new(self.parse_expr()?));
                // Allow trailing comma
                if self.peek().kind == TokenKind::Comma {
                    self.advance();
                }
                break;
            }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            fields.push((fname, self.parse_expr()?));
        }
        self.expect(&TokenKind::RBrace)?;
        Ok((fields, rest))
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
                // self::name resolves to current-module-qualified name.
                if self.peek().kind == TokenKind::ColonColon {
                    self.advance();
                    let name = self.expect_ident()?;
                    let resolved = self.resolve_self_path(&name);
                    return self.parse_resolved_path_expr(resolved, loc);
                }
                Ok(Expr {
                    kind: ExprKind::Var("self".to_string()),
                    loc,
                })
            }
            TokenKind::Super => {
                self.advance();
                // super::name resolves to parent-module-qualified name.
                self.expect(&TokenKind::ColonColon)?;
                let name = self.expect_ident()?;
                let resolved = self.resolve_super_path(&name);
                self.parse_resolved_path_expr(resolved, loc)
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

                // `todo!()`, `panic!("msg")`, `unreachable!()` diverging macros
                if matches!(name.as_str(), "todo" | "panic" | "unreachable")
                    && self.peek().kind == TokenKind::Bang
                {
                    self.advance(); // consume `!`
                    self.expect(&TokenKind::LParen)?;
                    let message = if self.peek().kind != TokenKind::RParen {
                        Some(Box::new(self.parse_expr()?))
                    } else {
                        None
                    };
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Expr {
                        kind: ExprKind::Abort { message },
                        loc,
                    });
                }

                // `matches!(expr, pat)` -- desugar to `match expr { pat => true, _ => false }`
                if name == "matches" && self.peek().kind == TokenKind::Bang {
                    self.advance(); // consume `!`
                    self.expect(&TokenKind::LParen)?;
                    let scrutinee = self.parse_expr()?;
                    self.expect(&TokenKind::Comma)?;
                    let pat = self.parse_pat()?;
                    self.expect(&TokenKind::RParen)?;
                    let make_bool_arm = |val: bool, pat: Pat| MatchArm {
                        pat,
                        guard: None,
                        body: Block {
                            stmts: vec![Stmt {
                                kind: StmtKind::Return(Some(Expr {
                                    kind: ExprKind::Bool(val),
                                    loc,
                                })),
                                loc,
                            }],
                        },
                        loc,
                    };
                    return Ok(Expr {
                        kind: ExprKind::Match {
                            expr: Box::new(scrutinee),
                            arms: vec![
                                make_bool_arm(true, pat),
                                make_bool_arm(false, Pat::Wildcard),
                            ],
                            scrutinee_ty: None,
                        },
                        loc,
                    });
                }

                if self.peek().kind == TokenKind::ColonColon {
                    self.advance();
                    let part2 = self.expect_ident()?;

                    if self.peek().kind == TokenKind::ColonColon {
                        self.advance();
                        let part3 = self.expect_ident()?;
                        // Three-part path: mod::Type::method or Type::Variant::field
                        // Resolve via alias table + module qualification.
                        let qualified = format!("{}_{part2}", self.resolve_name(name));
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
                    // Resolve via alias table + module qualification.
                    let type_name = self.resolve_name(name);
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
                } else if self.peek().kind == TokenKind::LBrace && starts_uppercase(&name) {
                    // Struct literal: resolve via alias table + module qualification.
                    let name = self.resolve_name(name);
                    let (fields, rest) = self.parse_struct_lit_body()?;
                    Ok(Expr {
                        kind: ExprKind::StructLit { name, fields, rest },
                        loc,
                    })
                } else if self.peek().kind == TokenKind::LParen {
                    // Function call: resolve via alias table + module qualification.
                    let name = self.resolve_name(name);
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
            TokenKind::LBrace => {
                // Block used as an expression: `{ stmts; expr }`
                // parse_block() already consumes `{` and `}`.
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Block(block),
                    loc,
                })
            }
            TokenKind::If => {
                self.advance(); // consume `if`
                // `if let pat = expr { ... }` in expression position
                if self.peek().kind == TokenKind::Let {
                    self.advance();
                    let pat = self.parse_pat()?;
                    self.expect(&TokenKind::Eq)?;
                    let expr = self.parse_if_let_expr()?;
                    let and_cond = if self.peek().kind == TokenKind::AmpAmp {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    let then_block = self.parse_block()?;
                    let else_block = if self.peek().kind == TokenKind::Else {
                        self.advance();
                        Some(self.parse_block()?)
                    } else {
                        None
                    };
                    // Encode as IfLet expression; and_cond fused into cond via Block wrapping
                    // for simplicity, we lower it like the statement form but inside an expr.
                    // We use a synthetic representation: store and_cond as the last stmt of then_block
                    // by wrapping the whole thing in ExprKind::IfLet.
                    let _ = and_cond; // TODO: and_cond support in if-let expr is deferred
                    return Ok(Expr {
                        kind: ExprKind::IfLet {
                            pat,
                            expr: Box::new(expr),
                            expr_ty: None,
                            then_block,
                            else_block,
                        },
                        loc,
                    });
                }
                let cond = self.parse_expr()?;
                let then_block = self.parse_block()?;
                let else_block = if self.peek().kind == TokenKind::Else {
                    self.advance();
                    if self.peek().kind == TokenKind::If {
                        // else if: parse recursively as expression, wrap in block
                        let inner_expr = self.parse_primary()?;
                        Some(Block {
                            stmts: vec![Stmt {
                                kind: StmtKind::Return(Some(inner_expr)),
                                loc,
                            }],
                        })
                    } else {
                        Some(self.parse_block()?)
                    }
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then_block,
                        else_block,
                    },
                    loc,
                })
            }
            TokenKind::Match => {
                self.advance(); // consume `match`
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::LBrace)?;
                let mut arms = Vec::new();
                while self.peek().kind != TokenKind::RBrace && !self.at_eof() {
                    let arm_loc = self.loc();
                    let pat = self.parse_pat()?;
                    let guard = if self.peek().kind == TokenKind::If {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
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
                        guard,
                        body,
                        loc: arm_loc,
                    });
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr {
                    kind: ExprKind::Match {
                        expr: Box::new(expr),
                        arms,
                        scrutinee_ty: None,
                    },
                    loc,
                })
            }
            TokenKind::Loop => {
                self.advance(); // consume `loop`
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Loop {
                        body: block,
                        result_ty: None,
                    },
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
