use super::parser::{PResult, Parser, Reported};
use crate::ast::*;
use crate::lexer::{Keyword, Punct, TokenKind};
use crate::source::Span;

impl Parser<'_> {
    pub(super) fn file_ast(&mut self) -> File {
        let package = self.package_clause();
        let mut imports = Vec::new();
        let mut items = Vec::new();
        loop {
            let before = self.pos;
            let result = match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Keyword(Keyword::Import) if items.is_empty() => {
                    self.import().map(|import| imports.push(import))
                }
                TokenKind::Keyword(Keyword::Import) => {
                    let span = self.current_span();
                    Err(self.error("imports must come before other declarations", span))
                }
                _ => self.item().map(|item| items.push(item)),
            };
            if result.is_err() || !self.declaration_end() {
                self.synchronize(0, false);
            }
            debug_assert!(self.pos > before || *self.peek() == TokenKind::Eof);
        }
        File {
            package,
            imports,
            items,
        }
    }

    pub(super) fn declaration_end(&mut self) -> bool {
        match self.peek() {
            TokenKind::Semicolon(_) => {
                self.bump();
                true
            }
            TokenKind::Eof => true,
            _ => {
                self.unexpected("newline or `;` after declaration");
                false
            }
        }
    }

    pub(super) fn package_clause(&mut self) -> Option<Name> {
        if !self.at_keyword(Keyword::Package) {
            let span = self.current_span();
            self.error("expected `package` clause at the start of the file", span);
            return None;
        }
        self.bump();
        let name = self.name("a package name").ok();
        if name.is_none() || !self.declaration_end() {
            self.synchronize(0, false);
        }
        name
    }

    pub(super) fn import(&mut self) -> PResult<Import> {
        let start = self.bump().span;
        let span = self.current_span();
        match self.peek().clone() {
            TokenKind::String(path)
                if self.file.text().as_bytes()[span.start() as usize] == b'"' =>
            {
                self.bump();
                Ok(Import {
                    path,
                    path_span: span,
                    span: self.span_from(start),
                })
            }
            TokenKind::Punct(Punct::LParen) => Err(self.error(
                "grouped imports are not supported; write one `import` per line",
                span,
            )),
            _ => Err(self.unexpected("a double-quoted import path")),
        }
    }

    pub(super) fn item(&mut self) -> PResult<Item> {
        match self.peek() {
            TokenKind::Keyword(Keyword::Func | Keyword::Async) => self.func().map(Item::Func),
            TokenKind::Keyword(Keyword::Type) => self.type_decl(),
            TokenKind::Keyword(Keyword::Let | Keyword::Var | Keyword::Const) => {
                self.binding().map(Item::Binding)
            }
            TokenKind::Reserved(word) => {
                let message = format!(
                    "`{}` is reserved for possible future use; the feature is not available",
                    word.as_str()
                );
                let span = self.current_span();
                Err(self.error(message, span))
            }
            _ => Err(self.unexpected("a declaration")),
        }
    }

    pub(super) fn func(&mut self) -> PResult<FuncDecl> {
        let start = self.current_span();
        let is_async = self.at_keyword(Keyword::Async);
        if is_async {
            self.bump();
            if !self.at_keyword(Keyword::Func) {
                return Err(self.unexpected("`func` after `async`"));
            }
        }
        self.bump();
        let receiver = if self.at(Punct::LParen) {
            self.bump();
            let receiver = self.param("a receiver name")?;
            if self.at(Punct::Comma) {
                let span = self.current_span();
                return Err(self.error("a method has exactly one receiver", span));
            }
            self.expect(Punct::RParen)?;
            Some(receiver)
        } else {
            None
        };
        let name = self.name("a function name")?;
        self.expect(Punct::LParen)?;
        let params = self.comma_list(Punct::RParen, "parameter", true, |p| {
            p.param("a parameter name")
        })?;
        let results = self.results()?;
        let native =
            self.native_functions && (self.at_separator() || self.peek() == &TokenKind::Eof);
        let body = if native {
            Block {
                stmts: Vec::new(),
                span: self.current_span(),
            }
        } else {
            self.body_block("function signature", "function declarations require a body")?
        };
        Ok(FuncDecl {
            is_async,
            receiver,
            name,
            params,
            results,
            body,
            native,
            span: self.span_from(start),
        })
    }

    pub(super) fn param(&mut self, what: &str) -> PResult<Param> {
        let name = self.name(what)?;
        if self.at(Punct::Comma) || self.at(Punct::RParen) {
            let message = format!(
                "`{}` needs its own type; grouped `a, b T` parameters are not supported",
                name.text
            );
            return Err(self.error(message, name.span));
        }
        let mode = if self.at_keyword(Keyword::Mut) && !self.at_slice_type_after(1) {
            self.bump();
            ParamMode::Mut
        } else if self.at_keyword(Keyword::Own) {
            self.bump();
            ParamMode::Own
        } else {
            ParamMode::Borrow
        };
        let ty = self.ty()?;
        if self.at(Punct::Eq) {
            let span = self.current_span();
            return Err(self.error("default parameter values are not supported", span));
        }
        Ok(Param {
            span: self.span_from(name.span),
            name,
            mode,
            ty,
        })
    }

    pub(super) fn results(&mut self) -> PResult<Vec<Type>> {
        if self.at(Punct::LParen) {
            let open = self.bump().span;
            let results = self.comma_list(Punct::RParen, "result type", false, Self::ty)?;
            if results.len() < 2 {
                let span = self.span_from(open);
                let message = if results.is_empty() {
                    "an empty result list is written by omitting it"
                } else {
                    "a single result type is written without parentheses"
                };
                return Err(self.error(message, span));
            }
            return Ok(results);
        }
        if matches!(
            self.peek(),
            TokenKind::Punct(Punct::LBrace | Punct::RBrace) | TokenKind::Semicolon(_)
        ) {
            return Ok(Vec::new());
        }
        Ok(vec![self.ty()?])
    }

    pub(super) fn type_decl(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.name("a type name")?;
        if self.at_keyword(Keyword::Interface) {
            return self.interface_decl(start, name).map(Item::Interface);
        }
        if !self.at_keyword(Keyword::Struct) {
            let base = self.ty()?;
            return Ok(Item::Named(NamedDecl {
                name,
                base,
                span: self.span_from(start),
            }));
        }
        self.struct_decl(start, name).map(Item::Struct)
    }

    fn struct_decl(&mut self, start: Span, name: Name) -> PResult<StructDecl> {
        self.bump();
        let open = self.body_open("struct type name")?;
        let mut fields = Vec::new();
        loop {
            if self.at(Punct::RBrace) {
                self.bump();
                break;
            }
            if *self.peek() == TokenKind::Eof {
                return Err(self.unclosed(open));
            }
            let depth = self.open_delimiters;
            let field = self.field_decl();
            let ok = match field {
                Ok(field) => {
                    fields.push(field);
                    self.statement_end("field")
                }
                Err(Reported) => false,
            };
            if !ok {
                self.synchronize(depth, true);
            }
        }
        Ok(StructDecl {
            name,
            fields,
            span: self.span_from(start),
        })
    }

    fn interface_decl(&mut self, start: Span, name: Name) -> PResult<InterfaceDecl> {
        self.bump();
        let open = self.body_open("interface type name")?;
        let mut methods = Vec::new();
        loop {
            if self.at(Punct::RBrace) {
                self.bump();
                break;
            }
            if *self.peek() == TokenKind::Eof {
                return Err(self.unclosed(open));
            }
            let depth = self.open_delimiters;
            let ok = match self.interface_method() {
                Ok(method) => {
                    methods.push(method);
                    self.statement_end("method")
                }
                Err(Reported) => false,
            };
            if !ok {
                self.synchronize(depth, true);
            }
        }
        Ok(InterfaceDecl {
            name,
            methods,
            span: self.span_from(start),
        })
    }

    fn interface_method(&mut self) -> PResult<InterfaceMethod> {
        let start = self.current_span();
        let is_async = self.at_keyword(Keyword::Async);
        if is_async {
            self.bump();
        }
        let receiver = if self.at_keyword(Keyword::Mut) {
            self.bump();
            ParamMode::Mut
        } else if self.at_keyword(Keyword::Own) {
            self.bump();
            ParamMode::Own
        } else {
            ParamMode::Borrow
        };
        let name = self.name("a method name")?;
        self.expect(Punct::LParen)?;
        let params = self.comma_list(Punct::RParen, "parameter", true, |p| {
            p.param("a parameter name")
        })?;
        let results = self.results()?;
        Ok(InterfaceMethod {
            is_async,
            receiver,
            name,
            params,
            results,
            span: self.span_from(start),
        })
    }

    pub(super) fn field_decl(&mut self) -> PResult<FieldDecl> {
        let name = self.name("a field name")?;
        let ty = self.ty()?;
        Ok(FieldDecl {
            span: self.span_from(name.span),
            name,
            ty,
        })
    }
}
