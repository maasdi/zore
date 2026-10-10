//! Compares the Zore parser, source manager, and diagnostics in `compiler-zore` with the Rust frontend.

#[path = "common.rs"]
mod common;

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use common::{
    Case, Generator, Program, build_program, byte_list, code_blocks, ore_files, repository,
};
use zore::ast::*;
use zore::diagnostic::{Diagnostic, Severity};
use zore::lexer::{IntBase, lex};
use zore::parser::{parse, parse_standard};
use zore::source::{SourceFile, SourceMap};

const PARSE: u8 = b'p';
const PARSE_NATIVE: u8 = b'n';
const PROBE: u8 = b's';
const GENERATED_CASES: usize = 4000;
const MUTATED_CASES: usize = 2000;
const GENERATED_PROBES: usize = 600;

fn oracle() -> &'static Path {
    static ORACLE: OnceLock<Program> = OnceLock::new();
    &ORACLE
        .get_or_init(|| build_program("compiler-zore/parseoracle/main.ore"))
        .executable
}

fn compare(cases: &[Case]) {
    common::compare(oracle(), cases, rust_records);
}

fn seed() -> u64 {
    common::seed("ZORE_PARSER_SEED")
}

fn body(text: &str) -> String {
    format!("package main\n\nfunc main() {{\n{text}\n}}\n")
}

fn rust_records(_dir: &Path, index: usize, case: &Case) -> Vec<String> {
    let Ok(text) = String::from_utf8(case.bytes.clone()) else {
        return vec!["invalid-utf8".into()];
    };
    let mut sources = SourceMap::new();
    let id = sources.add(format!("case-{index}.ore"), text).unwrap();
    let file = sources.file(id).unwrap();
    let mut records = Vec::new();
    if case.mode == PROBE {
        probe(&mut records, &sources, file);
        return records;
    }
    let parsed = if case.mode == PARSE_NATIVE {
        parse_standard(file)
    } else {
        parse(file)
    };
    let printer = Printer { file };
    records.push(format!("tree {}", printer.file(&parsed.file)));
    for (position, diagnostic) in parsed.diagnostics.iter().enumerate() {
        let span = diagnostic.span();
        if position < parsed.lexer_diagnostics {
            records.push(format!(
                "lexerror:{} {} {}",
                lexer_error_code(diagnostic.message()),
                span.start(),
                span.end()
            ));
        } else {
            describe(&mut records, &sources, diagnostic);
        }
    }
    records
}

fn describe(records: &mut Vec<String>, sources: &SourceMap, diagnostic: &Diagnostic) {
    let span = diagnostic.span();
    records.push(format!(
        "diag {} {} {}",
        span.start(),
        span.end(),
        diagnostic.message()
    ));
    if !diagnostic.primary_label().is_empty() {
        records.push(format!("  primary {}", diagnostic.primary_label()));
    }
    for (span, message) in diagnostic.related_labels() {
        records.push(format!(
            "  related {} {} {message}",
            span.start(),
            span.end()
        ));
    }
    for note in diagnostic.notes() {
        records.push(format!("  note {note}"));
    }
    match diagnostic.render(sources) {
        Ok(rendered) => {
            for line in rendered
                .split('\n')
                .collect::<Vec<_>>()
                .split_last()
                .unwrap()
                .1
            {
                records.push(format!("  render {line}"));
            }
        }
        Err(_) => records.push("  render failed".into()),
    }
}

fn probe(records: &mut Vec<String>, sources: &SourceMap, file: &SourceFile) {
    records.push(format!("lines {}", file.line_count()));
    for index in 0..file.line_count() {
        records.push(format!(
            "line {index} {}",
            byte_list(file.line(index).unwrap())
        ));
    }
    let mut locations = Vec::new();
    let mut boundaries = Vec::new();
    for offset in 0..=file.len() {
        match file.location(offset) {
            Some(location) => {
                locations.push(format!("{}:{}", location.line, location.column));
                boundaries.push(offset);
            }
            None => locations.push("-".into()),
        }
    }
    records.push(format!("loc {}", locations.join(" ")));
    let n = boundaries.len();
    let span = |start: u32, end: u32| file.span(start, end).unwrap();
    let diagnostic = Diagnostic::new(
        Severity::Error,
        "probe",
        span(boundaries[n / 3], boundaries[2 * n / 3]),
    )
    .primary_message("here")
    .related(span(boundaries[0], boundaries[n - 1]), "there")
    .note("probe note");
    describe(records, sources, &diagnostic);
}

/// Matches the codes of the Zore lexer; the lexer comparison test checks them in detail.
fn lexer_error_code(message: &str) -> &'static str {
    let starts = |prefix: &str| message.starts_with(prefix);
    if message == "unterminated block comment" {
        "unterminated-block-comment"
    } else if starts("identifier `") && message.contains("contains non-ASCII character") {
        "non-ascii-identifier"
    } else if starts("invalid digit ") {
        "invalid-digit"
    } else if starts("invalid suffix ") {
        "suffix"
    } else if message == "digit separator must be between two digits" {
        "separator"
    } else if starts("expected fractional digits") {
        "expected-fraction-digits"
    } else if starts("expected exponent digits") {
        "expected-exponent-digits"
    } else if starts("expected ") && message.contains(" digits after `") {
        "expected-prefixed-digits"
    } else if starts("expected digits") {
        "expected-digits"
    } else if message == "unterminated string literal" {
        "unterminated-string"
    } else if message == "unterminated rune literal" {
        "unterminated-rune"
    } else if message == "unterminated raw string literal" {
        "unterminated-raw-string"
    } else if starts("unknown escape sequence") {
        "unknown-escape"
    } else if message.contains("escape requires exactly") {
        "escape-digits"
    } else if message.ends_with("is not a Unicode scalar value") {
        "not-scalar"
    } else if message == "empty rune literal" {
        "empty-rune"
    } else if message == "rune literal contains more than one character" {
        "long-rune"
    } else if message.ends_with("is not a Zore operator") {
        "increment-decrement"
    } else if starts("unexpected character") {
        "unexpected-character"
    } else {
        panic!("no comparison code for lexer diagnostic `{message}`")
    }
}

/// Prints the tree as `(tag start end attr children...)`, `[...]` for lists, `_` for absent.
struct Printer<'a> {
    file: &'a SourceFile,
}

fn node(tag: &str, span: Option<zore::source::Span>, attr: &str, children: &[String]) -> String {
    let mut text = format!("({tag}");
    if let Some(span) = span {
        text.push_str(&format!(" {} {}", span.start(), span.end()));
    }
    if !attr.is_empty() {
        text.push(' ');
        text.push_str(attr);
    }
    for child in children {
        text.push(' ');
        text.push_str(child);
    }
    text.push(')');
    text
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(" "))
}

fn absent() -> String {
    "_".into()
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", byte_list(text))
}

fn mode(mode: ParamMode) -> &'static str {
    match mode {
        ParamMode::Borrow => "borrow",
        ParamMode::Mut => "mut",
        ParamMode::Own => "own",
    }
}

fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::BitAnd => "&",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

impl Printer<'_> {
    fn file(&self, file: &File) -> String {
        let span = self.file.span(0, self.file.len()).unwrap();
        node(
            "file",
            Some(span),
            "",
            &[
                file.package
                    .as_ref()
                    .map_or_else(absent, |name| self.name(name)),
                list(file.imports.iter().map(|import| {
                    node(
                        "import",
                        Some(import.span),
                        "",
                        &[node(
                            "path",
                            Some(import.path_span),
                            &quoted(&import.path),
                            &[],
                        )],
                    )
                })),
                list(file.items.iter().map(|item| self.item(item))),
            ],
        )
    }

    fn name(&self, name: &Name) -> String {
        node("name", Some(name.span), &name.text, &[])
    }

    fn item(&self, item: &Item) -> String {
        match item {
            Item::Func(func) => {
                let attr = format!(
                    "{} {}",
                    if func.is_async { "async" } else { "sync" },
                    if func.native { "native" } else { "body" }
                );
                node(
                    "func",
                    Some(func.span),
                    &attr,
                    &[
                        func.receiver
                            .as_ref()
                            .map_or_else(absent, |param| self.param(param)),
                        self.name(&func.name),
                        list(func.params.iter().map(|param| self.param(param))),
                        list(func.results.iter().map(|ty| self.ty(ty))),
                        self.block(&func.body),
                    ],
                )
            }
            Item::Struct(decl) => node(
                "struct",
                Some(decl.span),
                "",
                &[
                    self.name(&decl.name),
                    list(decl.fields.iter().map(|field| {
                        node(
                            "field",
                            Some(field.span),
                            "",
                            &[self.name(&field.name), self.ty(&field.ty)],
                        )
                    })),
                ],
            ),
            Item::Named(decl) => node(
                "named",
                Some(decl.span),
                "",
                &[self.name(&decl.name), self.ty(&decl.base)],
            ),
            Item::Interface(decl) => node(
                "interface",
                Some(decl.span),
                "",
                &[
                    self.name(&decl.name),
                    list(decl.methods.iter().map(|method| {
                        let attr = format!(
                            "{} {}",
                            if method.is_async { "async" } else { "sync" },
                            mode(method.receiver)
                        );
                        node(
                            "method",
                            Some(method.span),
                            &attr,
                            &[
                                self.name(&method.name),
                                list(method.params.iter().map(|param| self.param(param))),
                                list(method.results.iter().map(|ty| self.ty(ty))),
                            ],
                        )
                    })),
                ],
            ),
            Item::Binding(binding) => self.binding(binding),
        }
    }

    fn param(&self, param: &Param) -> String {
        node(
            "param",
            Some(param.span),
            mode(param.mode),
            &[self.name(&param.name), self.ty(&param.ty)],
        )
    }

    fn binding(&self, binding: &Binding) -> String {
        let tag = match binding.kind {
            BindingKind::Let => "let",
            BindingKind::Var => "var",
            BindingKind::Const => "const",
        };
        node(
            tag,
            Some(binding.span),
            "",
            &[
                list(
                    binding
                        .targets
                        .iter()
                        .map(|target| self.binding_target(target)),
                ),
                binding.ty.as_ref().map_or_else(absent, |ty| self.ty(ty)),
                self.expr(&binding.value),
            ],
        )
    }

    fn binding_target(&self, target: &BindingTarget) -> String {
        match target {
            BindingTarget::Name(name) => self.name(name),
            BindingTarget::Discard(span) => node("discard", Some(*span), "", &[]),
        }
    }

    fn ty(&self, ty: &Type) -> String {
        match ty {
            Type::Named(name) => node("type", Some(name.span), &name.text, &[]),
            Type::Qualified {
                package,
                name,
                span,
            } => node(
                "qualified",
                Some(*span),
                "",
                &[self.name(package), self.name(name)],
            ),
            Type::Array {
                element,
                size,
                span,
            } => node(
                "array",
                Some(*span),
                "",
                &[self.ty(element), self.expr(size)],
            ),
            Type::DynArray { element, span } => {
                node("dynarray", Some(*span), "", &[self.ty(element)])
            }
            Type::Map { key, value, span } => {
                node("map", Some(*span), "", &[self.ty(key), self.ty(value)])
            }
            Type::Slice {
                element,
                mutable,
                span,
            } => node(
                "slice",
                Some(*span),
                if *mutable { "mut" } else { "shared" },
                &[self.ty(element)],
            ),
            Type::Func {
                is_async,
                params,
                results,
                span,
            } => node(
                "functype",
                Some(*span),
                if *is_async { "async" } else { "sync" },
                &[
                    list(params.iter().map(|param| {
                        node("ftparam", None, mode(param.mode), &[self.ty(&param.ty)])
                    })),
                    list(results.iter().map(|ty| self.ty(ty))),
                ],
            ),
            Type::Channel { element, span } => {
                node("channeltype", Some(*span), "", &[self.ty(element)])
            }
            Type::Mutex { element, span } => node("mutex", Some(*span), "", &[self.ty(element)]),
            Type::Task { results, span } => node(
                "task",
                Some(*span),
                "",
                &[list(results.iter().map(|ty| self.ty(ty)))],
            ),
        }
    }

    fn optional(&self, expr: &Option<Box<Expr>>) -> String {
        expr.as_ref().map_or_else(absent, |expr| self.expr(expr))
    }

    fn expr(&self, expr: &Expr) -> String {
        let span = Some(expr.span);
        match &expr.kind {
            ExprKind::Name(name) => node("ident", span, name, &[]),
            ExprKind::Int(base) => {
                let radix = match base {
                    IntBase::Binary => "2",
                    IntBase::Octal => "8",
                    IntBase::Decimal => "10",
                    IntBase::Hexadecimal => "16",
                };
                node("int", span, radix, &[])
            }
            ExprKind::Float => node("float", span, "", &[]),
            ExprKind::String(value) => node("string", span, &quoted(value), &[]),
            ExprKind::Rune(value) => node("rune", span, &(*value as u32).to_string(), &[]),
            ExprKind::Bool(value) => node("bool", span, &value.to_string(), &[]),
            ExprKind::Nil => node("nil", span, "", &[]),
            ExprKind::Paren(inner) => node("paren", span, "", &[self.expr(inner)]),
            ExprKind::Unary { op, operand } => {
                let op = match op {
                    UnaryOp::Plus => "+",
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::Complement => "^",
                };
                node("unary", span, op, &[self.expr(operand)])
            }
            ExprKind::Binary { op, lhs, rhs } => node(
                "binary",
                span,
                binary_op(*op),
                &[self.expr(lhs), self.expr(rhs)],
            ),
            ExprKind::Await(inner) => node("await", span, "", &[self.expr(inner)]),
            ExprKind::Go(inner) => node("go", span, "", &[self.expr(inner)]),
            ExprKind::Try(inner) => node("try", span, "", &[self.expr(inner)]),
            ExprKind::Call { callee, args } => node(
                "call",
                span,
                "",
                &[
                    self.expr(callee),
                    list(args.iter().map(|arg| self.expr(arg))),
                ],
            ),
            ExprKind::Field { base, name } => {
                node("member", span, "", &[self.expr(base), self.name(name)])
            }
            ExprKind::Index { base, index } => {
                node("index", span, "", &[self.expr(base), self.expr(index)])
            }
            ExprKind::Slice { base, low, high } => node(
                "slicing",
                span,
                "",
                &[self.expr(base), self.optional(low), self.optional(high)],
            ),
            ExprKind::StructLit {
                package,
                ty,
                fields,
            } => node(
                "structlit",
                span,
                "",
                &[
                    package.as_ref().map_or_else(absent, |name| self.name(name)),
                    self.name(ty),
                    list(fields.iter().map(|field| {
                        node(
                            "init",
                            Some(field.span),
                            "",
                            &[self.name(&field.name), self.expr(&field.value)],
                        )
                    })),
                ],
            ),
            ExprKind::ArrayLit { ty, elements } => node(
                "arraylit",
                span,
                "",
                &[
                    self.ty(ty),
                    list(elements.iter().map(|element| self.expr(element))),
                ],
            ),
            ExprKind::MapLit { ty, entries } => node(
                "maplit",
                span,
                "",
                &[
                    self.ty(ty),
                    list(entries.iter().map(|entry| {
                        node(
                            "entry",
                            Some(entry.span),
                            "",
                            &[self.expr(&entry.key), self.expr(&entry.value)],
                        )
                    })),
                ],
            ),
            ExprKind::Closure(closure) => node(
                "closure",
                span,
                "",
                &[
                    list(closure.params.iter().map(|param| self.param(param))),
                    list(closure.results.iter().map(|ty| self.ty(ty))),
                    self.block(&closure.body),
                ],
            ),
            ExprKind::Channel { element, capacity } => node(
                "makechannel",
                span,
                "",
                &[self.ty(element), self.optional(capacity)],
            ),
            ExprKind::Malformed => node("malformed", span, "", &[]),
        }
    }

    fn block(&self, block: &Block) -> String {
        node(
            "block",
            Some(block.span),
            "",
            &[list(block.stmts.iter().map(|stmt| self.stmt(stmt)))],
        )
    }

    fn stmt(&self, stmt: &Stmt) -> String {
        let inner = match &stmt.kind {
            StmtKind::Binding(binding) => self.binding(binding),
            StmtKind::Assign {
                targets,
                op,
                values,
            } => {
                let op = match op {
                    AssignOp::Assign => "=".to_string(),
                    AssignOp::Compound(op) => format!("{}=", binary_op(*op)),
                };
                node(
                    "assign",
                    None,
                    &op,
                    &[
                        list(targets.iter().map(|target| match target {
                            AssignTarget::Place(expr) => self.expr(expr),
                            AssignTarget::Discard(span) => node("discard", Some(*span), "", &[]),
                        })),
                        list(values.iter().map(|value| self.expr(value))),
                    ],
                )
            }
            StmtKind::Expr(expr) => self.expr(expr),
            StmtKind::Return(values) => node(
                "return",
                None,
                "",
                &[list(values.iter().map(|value| self.expr(value)))],
            ),
            StmtKind::Break => node("break", None, "", &[]),
            StmtKind::Continue => node("continue", None, "", &[]),
            StmtKind::If(if_stmt) => self.if_stmt(if_stmt),
            StmtKind::For(for_stmt) => {
                let header = match &for_stmt.header {
                    ForHeader::Infinite => node("infinite", None, "", &[]),
                    ForHeader::Condition(condition) => {
                        node("condition", None, "", &[self.expr(condition)])
                    }
                    ForHeader::Counting {
                        init,
                        condition,
                        update,
                    } => node(
                        "counting",
                        None,
                        "",
                        &[self.stmt(init), self.expr(condition), self.stmt(update)],
                    ),
                    ForHeader::Each {
                        first,
                        second,
                        collection,
                    } => node(
                        "each",
                        None,
                        "",
                        &[
                            self.binding_target(first),
                            second
                                .as_ref()
                                .map_or_else(absent, |target| self.binding_target(target)),
                            self.expr(collection),
                        ],
                    ),
                };
                node("for", None, "", &[header, self.block(&for_stmt.body)])
            }
            StmtKind::Select(select) => node(
                "selectstmt",
                Some(select.span),
                "",
                &[
                    list(select.arms.iter().map(|arm| {
                        let comm = match &arm.comm {
                            SelectComm::Bind(binding) => self.binding(binding),
                            SelectComm::Expr(expr) => self.expr(expr),
                        };
                        node("arm", Some(arm.span), "", &[comm, self.block(&arm.body)])
                    })),
                    select
                        .default
                        .as_ref()
                        .map_or_else(absent, |block| self.block(block)),
                ],
            ),
            StmtKind::Block(block) => self.block(block),
        };
        node("stmt", Some(stmt.span), "", &[inner])
    }

    fn if_stmt(&self, if_stmt: &If) -> String {
        let otherwise = match &if_stmt.else_branch {
            None => absent(),
            Some(Else::If(nested)) => self.if_stmt(nested),
            Some(Else::Block(block)) => self.block(block),
        };
        node(
            "if",
            Some(if_stmt.span),
            "",
            &[
                self.expr(&if_stmt.condition),
                self.block(&if_stmt.then_block),
                otherwise,
            ],
        )
    }
}

/// The string literals in Rust test source, decoded, in order.
fn rust_string_literals(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut literals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let previous_is_word =
            i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'r'
            && !previous_is_word
            && matches!(bytes.get(i + 1), Some(b'"' | b'#'))
        {
            let mut hashes = 0;
            let mut j = i + 1;
            while bytes.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if bytes.get(j) != Some(&b'"') {
                i += 1;
                continue;
            }
            let closing = format!("\"{}", "#".repeat(hashes));
            let start = j + 1;
            let end = start + source[start..].find(&closing).expect("closed raw string");
            literals.push(source[start..end].to_string());
            i = end + closing.len();
        } else if bytes[i] == b'"' {
            let mut value = String::new();
            let mut chars = source[i + 1..].char_indices();
            let mut consumed = 0;
            while let Some((offset, ch)) = chars.next() {
                consumed = offset + ch.len_utf8();
                match ch {
                    '"' => break,
                    '\\' => {
                        let (_, escaped) = chars.next().expect("escape");
                        match escaped {
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            '0' => value.push('\0'),
                            '\\' | '"' | '\'' => value.push(escaped),
                            'x' => {
                                let digits: String =
                                    (0..2).map(|_| chars.next().unwrap().1).collect();
                                value.push(char::from(u8::from_str_radix(&digits, 16).unwrap()));
                            }
                            'u' => {
                                let mut digits = String::new();
                                for (_, c) in chars.by_ref() {
                                    if c == '}' {
                                        break;
                                    }
                                    if c != '{' {
                                        digits.push(c);
                                    }
                                }
                                value.push(
                                    char::from_u32(u32::from_str_radix(&digits, 16).unwrap())
                                        .unwrap(),
                                );
                            }
                            '\n' => {
                                while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                                    chars.next();
                                }
                            }
                            other => panic!("unsupported escape \\{other}"),
                        }
                    }
                    _ => value.push(ch),
                }
            }
            literals.push(value);
            i += 1 + consumed;
        } else if bytes[i] == b'\'' {
            // A char literal is skipped whole so a quote inside it does not start a string.
            match (bytes.get(i + 1), bytes.get(i + 2), bytes.get(i + 3)) {
                (Some(b'\\'), _, _) => {
                    i += 3;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        i += 1;
                    }
                    i += 1;
                }
                (Some(_), Some(b'\''), _) => i += 3,
                _ => i += 1,
            }
        } else {
            i += 1;
        }
    }
    literals
}

fn parse_case(label: impl Into<String>, text: &str) -> Case {
    Case::text(label, PARSE, text)
}

#[test]
fn targeted_programs_and_errors_match() {
    let inputs = [
        "",
        "package main",
        "package main\n",
        "func main() {}",
        "package 1\nfunc main() {}\n",
        "package main\nimport \"zore/os\"\nimport `raw`\nimport (\n\"a\"\n)\nfunc main() {}\nimport \"late\"\n",
        "package main\ntype Point struct {\n    X int\n    Y int\n}\ntype Bad interface {}\n",
        "package main\nfunc (p mut Point) move(dx int, dy own []int) (int, error) { return 0, nil }\n",
        "package main\nfunc f(a, b int) {}\nfunc g(a int = 1) {}\nfunc h() () {}\nfunc k() (int) {}\n",
        "package main\nfunc main()\n{\n}\n",
        "package main\nfunc main() {\n    if x {\n    }\n    else {\n    }\n}\n",
        "package main\nfunc main() {\n    let a = 1\n    var b, _ = f()\n    const c int = 2\n    let d: int = 3\n    let e := 4\n    let f\n}\n",
        "package main\nfunc main() {\n    x = 1\n    a, b = b, a\n    c += 1\n    a, b += 1\n    _ += 1\n    c -= 1, 2\n    f() = 3\n    1 + 2\n    ;\n}\n",
        "package main\nfunc main() {\n    for {}\n    for x < 3 {}\n    for var i = 0; i < 3; i += 1 {}\n    for i, v in list {}\n    for _ in list {}\n    for ; ; {}\n    for var i = 0; i < 3\n    {}\n}\n",
        "package main\nfunc main() {\n    for p := Point{X: 1}; p.X < 3; p.X += 1 {}\n    for let p = Point{X: 1}; p.X < 3; p.X += 1 {}\n}\n",
        "package main\nfunc main() {\n    for a, b = Array<Array<int>>{}, Point{X: 1}; a.len() < 1; a = a {}\n    for c, d = channel<Array<int>>(1), Point{X: 1}; d.X < 1; d.X += 1 {}\n}\n",
        "package main\nfunc main() {\n    select {\n    case let v, ok = ch.receive() {\n    }\n    case ch.send(1) {\n    }\n    default {\n    }\n    default {}\n    }\n    select {}\n}\n",
        "package main\nfunc main() {\n    let a = [int; 3]{1, 2, 3}\n    let b = Array<Array<int>>{}\n    let c = map[string]int{\"a\": 1,\n    }\n    let d = []int{}\n    let e = channel<int>(4)\n    let f = Array<int>\n}\n",
        "package main\nfunc main() {\n    let g = func(x int) int { return x }(2)\n    let h = func named() {}\n    let t Task<int, error> = go work()\n    let m Mutex<int> = nil\n    let v = await (await f())?\n    let w = x?.y\n    let s = a[1:2]\n    let z = a[:]\n    let q = a[1:2:3]\n}\n",
        "package main\nfunc main() {\n    let a = 1 < 2 < 3\n    let b = -x + ^y * !z\n    let c = p.Point{X: 1, Y: 2}\n    let d = Point{1, 2}\n    let e = Point{func: 1}\n    let f = [3]int{}\n    let g = Box<int>{}\n}\n",
        "package main\nvar total = 0\nconst limit = 10\nconst a, b = 1, 2\nlet x, y int = 1, 2\n",
        "package main\nfunc f(cb func(int, mut []int) (int, error), g async func(own User)) {}\nfunc k(x func(a int)) {}\n",
        "package main\nfunc main() {\n    let x = Array<int>{1, 2}\n    let y = f(1,\n        2)\n}\n",
        "package main\nfunc main() {\n    let x = Array<int>{1, 2\n    }\n    let unclosed = (1 +\n}\n",
        "package main\nfunc main() {\n    if a {\n",
        "package main\ntype S struct {\n    X int\n",
        "package main\ntype Duration int\ntype Name pkg.Text\ntype Items Array<int>\ntype Alias\ntype Other = int\n",
        "package main\nfunc main() {\n    let x = 1 let y = 2\n    let t Array<int>\n    let u Array<int>   // comment\n    let v Task\n    var w map[string]Array<int> = nil\n}\n",
        "package main\nfunc main() {\n    let a = 0x1G + 1e\n    let b = \"unterminated\n    let c = 'ab'\n    x++\n    @\n}\n",
        "package main\nfunc main() {\n    a.b.c(d)[e](f)\n    go a.b()\n    go func() {}()\n    await x\n    f()?\n}\n",
        "package main\nfunc main() {\n\tlet tab = 1\r\n\tlet crlf = 2\r\n}\r\n",
        "package main\nfunc main() {\n    let é = 1\n    let x = 名\n}\n",
        "package main\nasync func\nasync x\nenum\n",
        "package main\nfunc main() { return }\nfunc g() int { return 1, 2 }\n",
        "package main\nfunc main() {\n    let v = x >= y\n    let w = Array<int>{}>=1\n    let r = Array<Array<int>>=1\n}\n",
        "package main\nvar a Array<int>\r\nvar b Task<int>\r\nvar c channel<Array<int>> // c\r\nvar d Mutex<int>",
        "package main\nfunc main() {\n    for p = Point{X: 1}; p.X < 3; p.X += 1 {}\n    for q.Y = Point{}; q.Y < 1; q.Y += 1 {\n    }\n}\n",
        "package main\ntype R interface {\n    Read(buf mut []byte) (int, error)\n    mut Add(n int)\n    own Close() error\n    async Fetch(id int) (User, error)\n    async mut Push(x own T)\n}\ntype One interface { Count() int }\ntype Two interface { A(); B() }\n",
        "package main\ntype E interface {\n    Read\n    mut (x int)\n    Close() error extra\n    func Bad()\n    Fine()\n}\n",
        "package main\ntype U interface {\n    Read()\n",
        "package main\ntype N interface\n{\n}\nfunc f(r Reader, w mut io.Writer, c own Closer) {}\n",
    ];
    let mut cases: Vec<Case> = inputs
        .iter()
        .enumerate()
        .map(|(index, text)| parse_case(format!("targeted program {index}"), text))
        .collect();
    let statements = [
        "x",
        "x.y",
        "f(",
        "f(a b)",
        "f(a,)",
        "a[",
        "a[1",
        "{",
        "}",
        "if {",
        "if x",
        "if x {} else",
        "if x {} else if y {} else {}",
        "for x in {",
        "return 1,",
        "let",
        "let x =",
        "let _ = _",
        "const _ = 1",
        "var mut x = 1",
        "func",
        "type T struct {}",
        "break; continue",
        "x := 1",
        "go x",
        "go f()?",
        "await",
        "a ? b",
        "Point{X: 1,\n}",
        "Point{X: 1\n}",
        "f(1\n)",
        "[]int{}",
        "[int]{}",
        "[int; ]{}",
        "map[int]{}",
        "channel<int>",
        "channel(int)",
        "x = Array<int",
        "x = func(a int, b) {}",
        "x = func() (int,) {}",
        "x = a.(b)",
        "x = struct{}",
        "x = Array<int>\r\n{}",
        "x = Array<int> // note\r\n{}",
        "var t Task<int>\r\n= nil",
    ];
    for (index, statement) in statements.iter().enumerate() {
        cases.push(parse_case(format!("statement {index}"), &body(statement)));
    }
    compare(&cases);
}

#[test]
fn parser_and_source_test_literals_match() {
    let mut cases = Vec::new();
    for file in [
        "tests/parser/parser.rs",
        "tests/diagnostics/source_diagnostics.rs",
        "tests/lexer/lexer.rs",
    ] {
        let source = fs::read_to_string(repository().join(file)).unwrap();
        for (index, literal) in rust_string_literals(&source).iter().enumerate() {
            cases.push(parse_case(format!("{file} literal {index}"), literal));
            cases.push(parse_case(
                format!("{file} literal {index} in main"),
                &body(literal),
            ));
        }
    }
    assert!(
        cases.len() > 1000,
        "found only {} literal cases",
        cases.len()
    );
    compare(&cases);
}

#[test]
fn conformance_code_spans_and_specification_blocks_match() {
    let mut paths: Vec<_> = fs::read_dir(repository().join("tests/conformance"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).unwrap();
        for (number, line) in text.lines().enumerate() {
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1 && !span.is_empty() {
                    let label = format!("{name} line {} code span in main", number + 1);
                    cases.push(parse_case(label, &body(span)));
                }
            }
        }
    }
    let spec = fs::read_to_string(repository().join("spec/language-spec.md")).unwrap();
    for (line, block) in code_blocks(&spec) {
        let label = format!("spec/language-spec.md block at line {line}");
        cases.push(parse_case(label.clone(), &block));
        cases.push(parse_case(format!("{label} in main"), &body(&block)));
    }
    assert!(cases.len() > 2000, "found only {} cases", cases.len());
    compare(&cases);
}

#[test]
fn repository_sources_match() {
    let root = repository();
    let mut cases = Vec::new();
    for (dir, mode) in [
        ("examples", PARSE),
        ("benchmarks", PARSE),
        ("compiler-zore", PARSE),
        ("std", PARSE_NATIVE),
    ] {
        let mut paths = Vec::new();
        ore_files(&root.join(dir), &mut paths);
        for path in paths {
            cases.push(Case {
                label: path.strip_prefix(&root).unwrap().display().to_string(),
                mode,
                bytes: fs::read(&path).unwrap(),
            });
        }
    }
    assert!(cases.len() > 40, "found only {} source files", cases.len());
    compare(&cases);
}

#[rustfmt::skip]
const PIECES: &[&str] = &[
    "package ", "main ", "func ", "f", "x", "T", "(", ")", "{", "}", "[", "]", "let ", "var ",
    "const ", "=", "1", "2.5", ",", ";", "\n", "\n", "if ", "for ", "in ", "else ", "return ",
    ".", "?", "await ", "go ", "+", "-", "*", "<", ">", ">>", ">=", "==", "&&", "!", "^", "_",
    "type ", "struct ", "\"s\"", "`r`", "'c'", "@", ":", ":=", "own ", "mut ", "import ",
    "async ", "map", "channel", "Array", "Task", "Mutex", "select ", "case ", "default ",
    "break", "continue", "nil", "true", "int", "+=", "<<=", "// c\n", "/* c */", "é", "\t",
    "\r\n", "0x", "'", "\"",
];

#[test]
fn seeded_token_sequences_match() {
    let seed = seed();
    let mut generator = Generator(seed | 1);
    let mut cases = Vec::new();
    for index in 0..GENERATED_CASES {
        let count = generator.below(40);
        let text: String = (0..count)
            .map(|_| PIECES[generator.below(PIECES.len())])
            .collect();
        let text = if index % 2 == 0 { body(&text) } else { text };
        cases.push(parse_case(
            format!("seed {seed} generated case {index}"),
            &text,
        ));
    }
    compare(&cases);
}

const OPERATORS: &[&str] = &[
    "*", "/", "%", "<<", ">>", "&", "+", "-", "|", "^", "==", "!=", "<", "<=", ">", ">=", "&&",
    "||",
];

#[test]
fn seeded_operator_expressions_match() {
    let operands = [
        "a", "1", "f(x)", "s[i]", "p.q", "(b)", "-c", "!d", "^e", "await t", "v?",
    ];
    let seed = seed();
    let mut generator = Generator(seed.rotate_left(7) | 1);
    let mut cases = Vec::new();
    for index in 0..GENERATED_CASES / 2 {
        let mut text = operands[generator.below(operands.len())].to_string();
        for _ in 0..1 + generator.below(5) {
            text.push_str(&format!(
                " {} {}",
                OPERATORS[generator.below(OPERATORS.len())],
                operands[generator.below(operands.len())]
            ));
        }
        cases.push(parse_case(
            format!("seed {seed} expression {index}"),
            &body(&format!("x = {text}")),
        ));
    }
    compare(&cases);
}

#[test]
fn seeded_mutations_of_real_sources_match() {
    let root = repository();
    let mut paths = Vec::new();
    ore_files(&root.join("examples"), &mut paths);
    let sources: Vec<String> = paths
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    let seed = seed();
    let mut generator = Generator(seed.rotate_left(23) | 1);
    let mut cases = Vec::new();
    for index in 0..MUTATED_CASES {
        let original = generator.below(sources.len());
        let mut text = sources[original].clone();
        for _ in 0..1 + generator.below(3) {
            let mut map = SourceMap::new();
            let id = map.add("mutated.ore", text.clone()).unwrap();
            let tokens = lex(map.file(id).unwrap()).tokens;
            let token = &tokens[generator.below(tokens.len())];
            let (start, end) = (token.span.start() as usize, token.span.end() as usize);
            text = match generator.below(4) {
                0 => format!("{}{}", &text[..start], &text[end..]),
                1 => format!("{}{}", &text[..end], &text[start..]),
                2 => format!(
                    "{}{}{}",
                    &text[..start],
                    PIECES[generator.below(PIECES.len())],
                    &text[start..]
                ),
                _ => format!("{}\n{}", &text[..start], &text[start..]),
            };
        }
        cases.push(parse_case(
            format!(
                "seed {seed} mutation {index} of {}",
                paths[original].strip_prefix(&root).unwrap().display()
            ),
            &text,
        ));
    }
    compare(&cases);
}

#[test]
fn source_manager_locations_lines_and_rendering_match() {
    let inputs = [
        "",
        "x",
        "\n",
        "a\nb",
        "a\nb\n",
        "a\r\nb\r\n",
        "lone\rcarriage",
        "\t\tx\t= 1",
        "a\tbc\td",
        "aé🙂z\n",
        "名前\n二行目",
        "x\u{301}y",
        "nul\0here",
        "esc\u{1b}[0m",
        "del\u{7f} c1\u{85} nbsp\u{a0}",
        "\u{feff}bom",
        "\n\n\n",
        "trailing\n\n",
        "line one\nline two is longer\nthree",
    ];
    let mut cases: Vec<Case> = inputs
        .iter()
        .enumerate()
        .map(|(index, text)| Case::text(format!("probe {index}"), PROBE, text))
        .collect();
    let root = repository();
    let mut paths = Vec::new();
    ore_files(&root.join("examples"), &mut paths);
    for path in paths.iter().take(12) {
        cases.push(Case {
            label: format!("probe {}", path.strip_prefix(&root).unwrap().display()),
            mode: PROBE,
            bytes: fs::read(path).unwrap(),
        });
    }
    let pieces = [
        "a", "bc", " ", "\t", "\n", "\r\n", "\r", "é", "🙂", "名", "\u{301}", "\0", "\u{1b}",
        "\u{7f}", "\u{85}", "\u{a0}", "\u{2028}",
    ];
    let seed = seed();
    let mut generator = Generator(seed.rotate_left(41) | 1);
    for index in 0..GENERATED_PROBES {
        let count = generator.below(30);
        let text: String = (0..count)
            .map(|_| pieces[generator.below(pieces.len())])
            .collect();
        cases.push(Case::text(
            format!("seed {seed} generated probe {index}"),
            PROBE,
            &text,
        ));
    }
    compare(&cases);
}

#[test]
fn invalid_utf8_is_rejected_before_parsing() {
    let cases: Vec<Case> = [&b"package main\n\xff"[..], b"\xc3", b"package \xed\xa0\x80"]
        .iter()
        .enumerate()
        .map(|(index, bytes)| Case {
            label: format!("invalid case {index}"),
            mode: PARSE,
            bytes: bytes.to_vec(),
        })
        .collect();
    compare(&cases);
}
