use zore::ast::*;
use zore::parser::{Parsed, parse};
use zore::source::{SourceMap, Span};

struct Case {
    sources: SourceMap,
    parsed: Parsed,
}

impl Case {
    fn new(text: &str) -> Self {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", text.into()).unwrap();
        let parsed = parse(sources.file(id).unwrap());
        Self { sources, parsed }
    }

    fn body(body: &str) -> Self {
        Self::new(&format!("package main\n\nfunc main() {{\n{body}\n}}\n"))
    }

    fn text(&self, span: Span) -> &str {
        self.sources.slice(span).unwrap()
    }

    fn errors(&self) -> Vec<(&str, &str)> {
        self.parsed
            .diagnostics
            .iter()
            .map(|d| (d.message(), self.text(d.span())))
            .collect()
    }

    fn assert_clean(&self) -> &Self {
        assert!(self.parsed.diagnostics.is_empty(), "{:#?}", self.errors());
        self
    }

    fn main_stmts(&self) -> &[Stmt] {
        match self.parsed.file.items.last() {
            Some(Item::Func(func)) => &func.body.stmts,
            other => panic!("expected function, found {other:?}"),
        }
    }

    /// S-expression rendering of `main`'s statements.
    fn shape(&self) -> Vec<String> {
        self.main_stmts().iter().map(|s| stmt(self, s)).collect()
    }

    fn render(&self) -> String {
        self.parsed
            .diagnostics
            .iter()
            .map(|d| d.render(&self.sources).unwrap())
            .collect()
    }
}

/// Panics unless the type is `Type::Named`.
fn type_name(t: &Type) -> &str {
    match t {
        Type::Named(name) => &name.text,
        Type::Qualified { .. }
        | Type::Array { .. }
        | Type::Slice { .. }
        | Type::DynArray { .. }
        | Type::Map { .. }
        | Type::Func { .. }
        | Type::Channel { .. }
        | Type::Mutex { .. }
        | Type::Task { .. } => panic!("expected a named type, found a composite type"),
    }
}

fn ty(case: &Case, t: &Type) -> String {
    match t {
        Type::Named(name) => name.text.clone(),
        Type::Qualified { package, name, .. } => format!("{}.{}", package.text, name.text),
        Type::Array { element, size, .. } => {
            format!("[{} ; {}]", ty(case, element), expr(case, size))
        }
        Type::DynArray { element, .. } => format!("Array<{}>", ty(case, element)),
        Type::Map { key, value, .. } => format!("map[{}]{}", ty(case, key), ty(case, value)),
        Type::Slice {
            element, mutable, ..
        } => format!(
            "{}[]{}",
            if *mutable { "mut " } else { "" },
            ty(case, element)
        ),
        Type::Channel { element, .. } => format!("channel<{}>", ty(case, element)),
        Type::Mutex { element, .. } => format!("Mutex<{}>", ty(case, element)),
        Type::Task { results, .. } => {
            let results: Vec<String> = results.iter().map(|r| ty(case, r)).collect();
            format!("Task<{}>", results.join(", "))
        }
        Type::Func {
            is_async,
            params,
            results,
            ..
        } => {
            let params: Vec<String> = params
                .iter()
                .map(|param| format!("{}{}", mode(param.mode), ty(case, &param.ty)))
                .collect();
            let results: Vec<String> = results.iter().map(|r| ty(case, r)).collect();
            format!(
                "{}func({}) ({})",
                if *is_async { "async " } else { "" },
                params.join(", "),
                results.join(", ")
            )
        }
    }
}

fn mode(mode: ParamMode) -> &'static str {
    match mode {
        ParamMode::Borrow => "",
        ParamMode::Mut => "mut ",
        ParamMode::Own => "own ",
    }
}

fn expr(case: &Case, e: &Expr) -> String {
    match &e.kind {
        ExprKind::Name(name) => name.clone(),
        ExprKind::Int(_) | ExprKind::Float => case.text(e.span).to_owned(),
        ExprKind::String(value) => format!("{value:?}"),
        ExprKind::Rune(value) => format!("{value:?}"),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Nil => "nil".into(),
        ExprKind::Malformed => "<malformed>".into(),
        ExprKind::Paren(inner) => format!("(paren {})", expr(case, inner)),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Plus => "+",
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::Complement => "^",
            };
            format!("({op} {})", expr(case, operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", binary(*op), expr(case, lhs), expr(case, rhs))
        }
        ExprKind::Await(inner) => format!("(await {})", expr(case, inner)),
        ExprKind::Go(inner) => format!("(go {})", expr(case, inner)),
        ExprKind::Channel { element, capacity } => match capacity {
            Some(capacity) => format!(
                "(make channel<{}> {})",
                ty(case, element),
                expr(case, capacity)
            ),
            None => format!("(make channel<{}>)", ty(case, element)),
        },
        ExprKind::Try(inner) => format!("(? {})", expr(case, inner)),
        ExprKind::Call { callee, args } => {
            let mut out = format!("(call {}", expr(case, callee));
            for arg in args {
                out.push(' ');
                out.push_str(&expr(case, arg));
            }
            out + ")"
        }
        ExprKind::Field { base, name } => format!("(. {} {})", expr(case, base), name.text),
        ExprKind::Index { base, index } => {
            format!("(index {} {})", expr(case, base), expr(case, index))
        }
        ExprKind::Slice { base, low, high } => {
            let bound = |b: &Option<Box<Expr>>| b.as_ref().map_or("_".into(), |b| expr(case, b));
            format!(
                "(slice {} {} {})",
                expr(case, base),
                bound(low),
                bound(high)
            )
        }
        ExprKind::StructLit {
            package,
            ty: struct_ty,
            fields,
        } => {
            let qualifier = package
                .as_ref()
                .map_or(String::new(), |package| format!("{}.", package.text));
            let mut out = format!("(lit {qualifier}{}", struct_ty.text);
            for field in fields {
                out.push_str(&format!(
                    " {}:{}",
                    field.name.text,
                    expr(case, &field.value)
                ));
            }
            out + ")"
        }
        ExprKind::MapLit {
            ty: map_ty,
            entries,
        } => {
            let mut out = format!("(lit {}", ty(case, map_ty));
            for entry in entries {
                out.push_str(&format!(
                    " {}:{}",
                    expr(case, &entry.key),
                    expr(case, &entry.value)
                ));
            }
            out + ")"
        }
        ExprKind::ArrayLit {
            ty: array_ty,
            elements,
        } => {
            let mut out = format!("(lit {}", ty(case, array_ty));
            for element in elements {
                out.push(' ');
                out.push_str(&expr(case, element));
            }
            out + ")"
        }
        ExprKind::Closure(closure) => {
            let params: Vec<String> = closure
                .params
                .iter()
                .map(|p| format!("{} {}{}", p.name.text, mode(p.mode), ty(case, &p.ty)))
                .collect();
            let results: Vec<String> = closure.results.iter().map(|r| ty(case, r)).collect();
            let body: Vec<String> = closure.body.stmts.iter().map(|s| stmt(case, s)).collect();
            format!(
                "(func ({}) ({}) {{{}}})",
                params.join(", "),
                results.join(", "),
                body.join("; ")
            )
        }
    }
}

fn binary(op: BinaryOp) -> &'static str {
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

fn block(case: &Case, b: &Block) -> String {
    let stmts: Vec<String> = b.stmts.iter().map(|s| stmt(case, s)).collect();
    format!("{{{}}}", stmts.join("; "))
}

fn binding(case: &Case, b: &Binding) -> String {
    let kind = match b.kind {
        BindingKind::Let => "let",
        BindingKind::Var => "var",
        BindingKind::Const => "const",
    };
    let targets: Vec<&str> = b
        .targets
        .iter()
        .map(|t| match t {
            BindingTarget::Name(name) => name.text.as_str(),
            BindingTarget::Discard(_) => "_",
        })
        .collect();
    let ty_text =
        b.ty.as_ref()
            .map(|t| format!(" {}", ty(case, t)))
            .unwrap_or_default();
    format!(
        "({kind} {}{ty_text} {})",
        targets.join(","),
        expr(case, &b.value)
    )
}

fn stmt(case: &Case, s: &Stmt) -> String {
    match &s.kind {
        StmtKind::Binding(b) => binding(case, b),
        StmtKind::Assign {
            targets,
            op,
            values,
        } => {
            let targets: Vec<String> = targets
                .iter()
                .map(|t| match t {
                    AssignTarget::Place(e) => expr(case, e),
                    AssignTarget::Discard(_) => "_".into(),
                })
                .collect();
            let values: Vec<String> = values.iter().map(|v| expr(case, v)).collect();
            let op = match op {
                AssignOp::Assign => "=".to_owned(),
                AssignOp::Compound(op) => format!("{}=", binary(*op)),
            };
            format!("({op} {} {})", targets.join(","), values.join(","))
        }
        StmtKind::Expr(e) => expr(case, e),
        StmtKind::Return(values) => {
            let values: Vec<String> = values.iter().map(|v| expr(case, v)).collect();
            format!(
                "(return{})",
                values.iter().map(|v| format!(" {v}")).collect::<String>()
            )
        }
        StmtKind::Break => "break".into(),
        StmtKind::Continue => "continue".into(),
        StmtKind::If(i) => if_stmt(case, i),
        StmtKind::For(f) => {
            let header = match &f.header {
                ForHeader::Infinite => String::new(),
                ForHeader::Condition(c) => format!(" {}", expr(case, c)),
                ForHeader::Counting {
                    init,
                    condition,
                    update,
                } => format!(
                    " {}; {}; {}",
                    stmt(case, init),
                    expr(case, condition),
                    stmt(case, update)
                ),
                ForHeader::Each {
                    first,
                    second,
                    collection,
                } => {
                    let name = |target: &BindingTarget| match target {
                        BindingTarget::Name(name) => name.text.clone(),
                        BindingTarget::Discard(_) => "_".into(),
                    };
                    let names = std::iter::once(first)
                        .chain(second)
                        .map(name)
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(" {names} in {}", expr(case, collection))
                }
            };
            format!("(for{header} {})", block(case, &f.body))
        }
        StmtKind::Block(b) => block(case, b),
        StmtKind::Select(select) => {
            let mut out = String::from("(select");
            for arm in &select.arms {
                let comm = match &arm.comm {
                    SelectComm::Bind(b) => binding(case, b),
                    SelectComm::Expr(e) => expr(case, e),
                };
                out.push_str(&format!(" (case {comm} {})", block(case, &arm.body)));
            }
            if let Some(default) = &select.default {
                out.push_str(&format!(" (default {})", block(case, default)));
            }
            out.push(')');
            out
        }
    }
}

fn if_stmt(case: &Case, i: &If) -> String {
    let mut out = format!(
        "(if {} {}",
        expr(case, &i.condition),
        block(case, &i.then_block)
    );
    match &i.else_branch {
        None => {}
        Some(Else::Block(b)) => out.push_str(&format!(" else {}", block(case, b))),
        Some(Else::If(inner)) => out.push_str(&format!(" else {}", if_stmt(case, inner))),
    }
    out + ")"
}

/// Parse `source` as one expression statement `_ = source` and render it.
fn expr_shape(source: &str) -> String {
    let case = Case::body(&format!("_ = {source}"));
    case.assert_clean();
    let shape = case.shape();
    shape[0]
        .strip_prefix("(= _ ")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap()
        .to_owned()
}

fn rejects(body: &str, message: &str) {
    let case = Case::body(body);
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{body:?}: expected {message:?}, got {errors:#?}"
    );
}

fn rejects_file(text: &str, message: &str) {
    let case = Case::new(text);
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{text:?}: expected {message:?}, got {errors:#?}"
    );
}

#[test]
fn semantic_target_parses_to_expected_shape() {
    let mut sources = SourceMap::new();
    let id = sources
        .load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../examples/semantic-target/main.ore"
        ))
        .unwrap();
    let parsed = parse(sources.file(id).unwrap());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = &parsed.file;
    assert_eq!(file.package.as_ref().unwrap().text, "main");
    let [Item::Struct(user), Item::Func(greet), Item::Func(main)] = &file.items[..] else {
        panic!("{:#?}", file.items);
    };
    assert_eq!(user.name.text, "User");
    assert_eq!(user.fields[0].name.text, "Name");
    assert_eq!(type_name(&user.fields[0].ty), "string");
    assert_eq!(greet.params[0].mode, ParamMode::Borrow);
    assert_eq!(type_name(&greet.params[0].ty), "User");
    assert!(greet.results.is_empty() && main.params.is_empty());
    assert_eq!(
        sources.slice(greet.span).unwrap().lines().next(),
        Some("func greet(user User) {")
    );
    assert!(
        sources
            .slice(main.span)
            .unwrap()
            .ends_with("greet(user)\n}")
    );

    let Some(Item::Func(_)) = file.items.last() else {
        unreachable!()
    };
    let case = Case { sources, parsed };
    assert_eq!(
        case.shape(),
        ["(let user (lit User Name:\"John\"))", "(call greet user)"]
    );
}

#[test]
fn hello_example_parses() {
    let mut sources = SourceMap::new();
    let id = sources
        .load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../examples/hello/main.ore"
        ))
        .unwrap();
    let parsed = parse(sources.file(id).unwrap());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn precedence_and_associativity_follow_the_operator_table() {
    for (source, expected) in [
        ("a + b * c", "(+ a (* b c))"),
        ("a - b - c", "(- (- a b) c)"),
        ("a + b << c", "(+ a (<< b c))"),
        ("a | b ^ c", "(^ (| a b) c)"),
        ("a == b || c", "(|| (== a b) c)"),
        ("a || b && c", "(|| a (&& b c))"),
        ("a & b + c", "(+ (& a b) c)"),
        ("a % b / c", "(/ (% a b) c)"),
        ("a >> b != c", "(!= (>> a b) c)"),
        ("(a + b) * c", "(* (paren (+ a b)) c)"),
        ("-a * b", "(* (- a) b)"),
        ("!-^+a", "(! (- (^ (+ a))))"),
        ("a &^ b", "(& a (^ b))"),
        ("- -x", "(- (- x))"),
        ("f(a)(b).c.d(e)", "(call (. (. (call (call f a) b) c) d) e)"),
        ("-read()?", "(- (? (call read)))"),
        ("read()??", "(? (? (call read)))"),
        ("(value?).field", "(. (paren (? value)) field)"),
        ("a < b && b < c", "(&& (< a b) (< b c))"),
        ("(a < b) == c", "(== (paren (< a b)) c)"),
        ("int64(value)", "(call int64 value)"),
        ("\"a\" + `b`", "(+ \"a\" \"b\")"),
        ("'x'", "'x'"),
        ("1.5 + 0xFF", "(+ 1.5 0xFF)"),
        ("true || false || nil", "(|| (|| true false) nil)"),
    ] {
        assert_eq!(expr_shape(source), expected, "{source}");
    }
}

#[test]
fn await_groups_trailing_propagation_outside() {
    for (source, expected) in [
        ("await operation()?", "(? (await (call operation)))"),
        (
            "await object.method()?",
            "(? (await (call (. object method))))",
        ),
        ("await task?", "(? (await task))"),
        (
            "await (operation()?)",
            "(await (paren (? (call operation))))",
        ),
        ("await operation()", "(await (call operation))"),
        ("-await operation()?", "(- (? (await (call operation))))"),
        ("await a + b", "(+ (await a) b)"),
        ("await -x", "(await (- x))"),
    ] {
        assert_eq!(expr_shape(source), expected, "{source}");
    }
}

#[test]
fn comparison_chains_and_misplaced_propagation_are_rejected() {
    for source in ["a < b < c", "a == b == c", "a < b == c", "a != b >= c"] {
        rejects(&format!("_ = {source}"), "cannot be chained");
    }
    rejects("_ = value?.field", "binds more loosely");
    rejects("_ = call()?()", "binds more loosely");
}

#[test]
fn bindings_follow_initialization_syntax() {
    let case = Case::body(
        "let count = 0
        var total int64 = 0
        const limit = 100
        const capacity int64 = 100
        let value, err = load()
        var first, second = pair()
        let _, _ = pair()
        let _ = calculate()
        var _ = calculate()",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let count 0)",
            "(var total int64 0)",
            "(const limit 100)",
            "(const capacity int64 100)",
            "(let value,err (call load))",
            "(var first,second (call pair))",
            "(let _,_ (call pair))",
            "(let _ (call calculate))",
            "(var _ (call calculate))",
        ]
    );
    let Stmt { span, .. } = &case.main_stmts()[1];
    assert_eq!(case.text(*span), "var total int64 = 0");
}

#[test]
fn invalid_bindings_are_rejected() {
    for (body, message) in [
        ("let count: int64 = 0", "not written with `:`"),
        ("let count int64", "require an initializer"),
        ("var count int64", "require an initializer"),
        ("let count", "require an initializer"),
        ("let a, b = 1, 2", "exactly one initializer"),
        ("let a, b int = pair()", "typed multiple bindings"),
        ("const a, b = pair()", "exactly one name"),
        ("const _ = 1", "`_` cannot be used as a binding name"),
        ("let func = 1", "`func` is a keyword"),
        ("let own = 1", "`own` is a keyword"),
        ("let match = 1", "`match` is reserved"),
        ("let count := 1", "`:=` is not Zore syntax"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn assignments_and_call_statements() {
    let case = Case::body(
        "count = count + 1
        count += 1
        mask <<= 2
        left, right = right, left
        left, right = pair()
        user.Name = name
        _ = save()
        _, _ = pair()
        greet(user)
        await operation()
        read(file)?
        await operation()?",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(= count (+ count 1))",
            "(+= count 1)",
            "(<<= mask 2)",
            "(= left,right right,left)",
            "(= left,right (call pair))",
            "(= (. user Name) name)",
            "(= _ (call save))",
            "(= _,_ (call pair))",
            "(call greet user)",
            "(await (call operation))",
            "(? (call read file))",
            "(? (await (call operation)))",
        ]
    );
}

#[test]
fn invalid_assignments_and_bare_expressions_are_rejected() {
    for (body, message) in [
        ("_ += 1", "`_` cannot be a compound"),
        ("a, b += 1", "exactly one target"),
        ("a += 1, 2", "exactly one value"),
        ("f() = 1", "invalid assignment target"),
        ("1 = x", "invalid assignment target"),
        ("(a) = 1", "invalid assignment target"),
        ("a = b = c", "expected newline or `;`"),
        ("a, b", "expected `=`"),
        ("_", "expected `=`"),
        ("x = _", "`_` cannot be used as a value"),
        ("count", "not a statement"),
        ("1 + 2", "not a statement"),
        ("a == b", "not a statement"),
        ("\"text\"", "not a statement"),
        ("await task", "not a statement"),
        ("(greet(user))", "not a statement"),
        (";", "empty statement"),
        ("a();;", "empty statement"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn functions_methods_and_parameters() {
    let case = Case::new(
        "package main

        func add(a int, b int,) int { return a + b }
        func pair() (int, error) { return 1, nil }
        func forward() (int, int) { return pair() }
        func (user User) greet() {}
        func (user mut User) rename(name string) { user.Name = name }
        func (user own User) save() {}
        func edit(user mut User, sink own Sink) {}
        async func fetch(id int) (User, error) {
            let response = await get(id)?
            return parse(response)?
        }
        func multi(
            first int,
            second int,
        ) {}
        ",
    );
    case.assert_clean();
    let funcs: Vec<&FuncDecl> = case
        .parsed
        .file
        .items
        .iter()
        .map(|item| match item {
            Item::Func(func) => func,
            other => panic!("{other:?}"),
        })
        .collect();
    let names: Vec<&str> = funcs.iter().map(|f| f.name.text.as_str()).collect();
    assert_eq!(
        names,
        [
            "add", "pair", "forward", "greet", "rename", "save", "edit", "fetch", "multi"
        ]
    );
    assert_eq!(funcs[0].params.len(), 2);
    assert_eq!(type_name(&funcs[0].results[0]), "int");
    let results: Vec<&str> = funcs[1].results.iter().map(type_name).collect();
    assert_eq!(results, ["int", "error"]);
    let modes: Vec<Option<ParamMode>> = funcs[3..6]
        .iter()
        .map(|f| f.receiver.as_ref().map(|r| r.mode))
        .collect();
    assert_eq!(
        modes,
        [
            Some(ParamMode::Borrow),
            Some(ParamMode::Mut),
            Some(ParamMode::Own)
        ]
    );
    assert_eq!(funcs[6].params[0].mode, ParamMode::Mut);
    assert_eq!(funcs[6].params[1].mode, ParamMode::Own);
    assert!(funcs[7].is_async && !funcs[0].is_async);
    assert_eq!(funcs[8].params.len(), 2);
    assert_eq!(case.text(funcs[6].params[1].span), "sink own Sink");
}

#[test]
fn invalid_function_syntax_is_rejected() {
    for (text, message) in [
        ("func add(a, b int) int { return a }", "grouped `a, b T`"),
        ("func add(a int = 1) {}", "default parameter values"),
        ("func add(a int...) {}", "expected `,` or `)`"),
        ("func add(a int) int", "require a body"),
        ("func main()\n{}", "same line as the function signature"),
        ("func one() (int) { return 1 }", "without parentheses"),
        ("func none() () {}", "omitting it"),
        (
            "func pair() (int, int,) { return 1, 2 }",
            "trailing comma is not allowed",
        ),
        ("func (a A, b B) m() {}", "exactly one receiver"),
        (
            "func (_ User) m() {}",
            "`_` cannot be used as a receiver name",
        ),
        ("func f(_ int) {}", "`_` cannot be used as a parameter name"),
        ("func type() {}", "`type` is a keyword"),
        ("async main() {}", "expected `func` after `async`"),
        (
            "func f(\n a int\n) {}",
            "missing trailing comma after the last parameter",
        ),
    ] {
        rejects_file(&format!("package main\n{text}\n"), message);
    }
    rejects("f(a,,b)", "expected an expression");
    rejects("f(,)", "expected an expression");
}

#[test]
fn struct_keyword_does_not_end_a_line() {
    // `struct` is not eligible for semicolon insertion.
    Case::new("package main\ntype User struct\n{}\n").assert_clean();
}

#[test]
fn structs_and_struct_literals() {
    let case = Case::new(
        "package main
        type User struct {
            Name string
            age int
        }
        type Empty struct {}
        type Point struct { X int; Y int }
        func main() {
            let user = User{
                Name: loadName(),
                age: loadAge(),
            }
            let point = Point{Y: 2, X: 1}
            let empty = Empty{}
            greet(User{Name: \"x\", age: 1})
            if (Point{X: 1, Y: 2}).X == 1 { work() }
            for (Point{X: 1, Y: 2}).X == 1 { work() }
            for var p = Point{X: 0, Y: 0}; p.X < 3; p.X += 1 {}
            for p = Point{X: 0, Y: 0}; p.X < 3; p.X += 1 {}
        }",
    );
    case.assert_clean();
    let Item::Struct(user) = &case.parsed.file.items[0] else {
        panic!()
    };
    let fields: Vec<(&str, &str)> = user
        .fields
        .iter()
        .map(|f| (f.name.text.as_str(), type_name(&f.ty)))
        .collect();
    assert_eq!(fields, [("Name", "string"), ("age", "int")]);
    let Item::Struct(point) = &case.parsed.file.items[2] else {
        panic!()
    };
    assert_eq!(point.fields.len(), 2);
    assert_eq!(
        case.shape(),
        [
            "(let user (lit User Name:(call loadName) age:(call loadAge)))",
            "(let point (lit Point Y:2 X:1))",
            "(let empty (lit Empty))",
            "(call greet (lit User Name:\"x\" age:1))",
            "(if (== (. (paren (lit Point X:1 Y:2)) X) 1) {(call work)})",
            "(for (== (. (paren (lit Point X:1 Y:2)) X) 1) {(call work)})",
            "(for (var p (lit Point X:0 Y:0)); (< (. p X) 3); (+= (. p X) 1) {})",
            "(for (= p (lit Point X:0 Y:0)); (< (. p X) 3); (+= (. p X) 1) {})",
        ]
    );
}

#[test]
fn retried_loop_initializer_reads_the_original_tokens() {
    let case = Case::body("for a, b = Array<Array<int>>{}, Point{X: 1}; a.len() < 1; a = a {}");
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(for (= a,b (lit Array<Array<int>>),(lit Point X:1)); (< (call (. a len)) 1); (= a a) {})"
        ]
    );
}

#[test]
fn invalid_struct_syntax_is_rejected() {
    for (text, message) in [
        ("type User struct {\n own string\n}", "`own` is a keyword"),
        (
            "type User struct {\n Name string, age int\n}",
            "expected newline or `;` after field",
        ),
        ("type User struct {\n Name\n}", "expected a type"),
        ("type Alias int", "only struct type declarations"),
    ] {
        rejects_file(&format!("package main\n{text}\n"), message);
    }
    for (body, message) in [
        ("let u = User{\"John\"}", "must be named"),
        (
            "let u = User{Name: \"x\"\n}",
            "missing trailing comma after the last field",
        ),
        ("let u = User{Name \"x\"}", "must be named"),
        ("if Point{X: 1}.X == 1 { work() }", "expected"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn arrays_and_indexing() {
    let case = Case::body(
        "let xs [int; 3] = [int; 3]{1, 2, 3}
        let first = xs[0]
        xs[0] = 4
        xs[0] += 1
        _ = xs[0] + xs[1]
        let grid = [int; 2]{
            1,
            2,
        }
        if ([int; 1]{1})[0] == 1 { work() }",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let xs [int ; 3] (lit [int ; 3] 1 2 3))",
            "(let first (index xs 0))",
            "(= (index xs 0) 4)",
            "(+= (index xs 0) 1)",
            "(= _ (+ (index xs 0) (index xs 1)))",
            "(let grid (lit [int ; 2] 1 2))",
            "(if (== (index (paren (lit [int ; 1] 1)) 0) 1) {(call work)})",
        ]
    );
}

#[test]
fn slice_types_and_slicing_expressions() {
    let case = Case::body(
        "let a []int = xs[1:3]
        var b mut []int = xs[:2]
        let c = xs[1:]
        let d = xs[:]
        edit(xs[:])
        let nested [][]int = grid[:]
        let rows [[]int; 2] = rows
        _ = c[0]",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let a []int (slice xs 1 3))",
            "(var b mut []int (slice xs _ 2))",
            "(let c (slice xs 1 _))",
            "(let d (slice xs _ _))",
            "(call edit (slice xs _ _))",
            "(let nested [][]int (slice grid _ _))",
            "(let rows [[]int ; 2] rows)",
            "(= _ (index c 0))",
        ]
    );
}

#[test]
fn dynamic_array_types_and_literals() {
    let case = Case::body(
        "let a Array<int> = Array<int>{1, 2}
        let nested Array<Array<int>>= Array<Array<int>>{}
        let views Array<[]int> = views
        let fixed [Array<int>; 2] = fixed
        let rows mut []Array<int> = rows
        let grid = Array<int>{
            1,
            2,
        }
        if Array<int>{1}[0] == 1 { work() }
        _ = a < b",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let a Array<int> (lit Array<int> 1 2))",
            "(let nested Array<Array<int>> (lit Array<Array<int>>))",
            "(let views Array<[]int> views)",
            "(let fixed [Array<int> ; 2] fixed)",
            "(let rows mut []Array<int> rows)",
            "(let grid (lit Array<int> 1 2))",
            "(if (== (index (lit Array<int> 1) 0) 1) {(call work)})",
            "(= _ (< a b))",
        ]
    );
}

#[test]
fn task_types_and_go_expressions() {
    let case = Case::body(
        "let a Task<int> = go work(1)
        let b Task<User, error> = go load()
        let c Task = go log()
        let d Array<Task<int>> = d
        go log()
        let e = go object.method(x)
        let f = (go work())
        let g = a + go work()",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let a Task<int> (go (call work 1)))",
            "(let b Task<User, error> (go (call load)))",
            "(let c Task<> (go (call log)))",
            "(let d Array<Task<int>> d)",
            "(go (call log))",
            "(let e (go (call (. object method) x)))",
            "(let f (paren (go (call work))))",
            "(let g (+ a (go (call work))))",
        ]
    );
}

#[test]
fn channel_types_and_creation() {
    let case = Case::body(
        "let a = channel<int>()
        let b = channel<Array<int>>(n + 1)
        let c channel<channel<int>> = c
        let d Array<channel<User>> = d
        a.send(1)
        let v, ok = a.receive()
        a.close()
        let e = [channel<int>; 2]{a, b}",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let a (make channel<int>))",
            "(let b (make channel<Array<int>> (+ n 1)))",
            "(let c channel<channel<int>> c)",
            "(let d Array<channel<User>> d)",
            "(call (. a send) 1)",
            "(let v,ok (call (. a receive)))",
            "(call (. a close))",
            "(let e (lit [channel<int> ; 2] a b))",
        ]
    );
}

#[test]
fn mutex_types_and_calls() {
    let case = Case::body(
        "let a = mutex(0)
        let b Mutex<int> = a
        let c Mutex<Array<string>> = c
        let d Array<Mutex<int>> = d
        let e channel<Mutex<int>> = e
        a.withLock(func(value mut int) { value += 1 })
        let f = a.isPoisoned()",
    );
    case.assert_clean();
    assert_eq!(
        case.shape()[..5],
        [
            "(let a (call mutex 0))",
            "(let b Mutex<int> a)",
            "(let c Mutex<Array<string>> c)",
            "(let d Array<Mutex<int>> d)",
            "(let e channel<Mutex<int>> e)",
        ]
    );
}

#[test]
fn invalid_mutex_syntax_is_rejected() {
    for (body, message) in [
        ("let m Mutex<> = m", "expected a type"),
        ("let m Mutex<int = m", "expected `>`"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn invalid_channel_syntax_is_rejected() {
    for (body, message) in [
        ("let c = channel<int>", "expected `(`"),
        ("let c = channel(1)", "expected `<`"),
        ("let c channel = c", "expected `<`"),
        ("let c = channel<>()", "expected a type"),
        ("let c = channel<int>(1, 2)", "expected `)`"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn invalid_task_syntax_is_rejected() {
    for (body, message) in [
        ("let t Task<> = t", "expected a type"),
        ("let t Task<int = t", "expected `>`"),
        ("let t = go", "expected"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn invalid_dynamic_array_syntax_is_rejected() {
    for (body, message) in [
        ("let xs = Array<int>(1)", "expected `{` after `Array<T>`"),
        ("let xs Foo<int> = xs", "`Foo` does not take type arguments"),
        ("let xs Array<int = xs", "expected `>`"),
        ("let xs = Array<int>{1,\n2\n}", "missing trailing comma"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn a_closing_type_argument_ends_a_line_like_an_identifier() {
    let case = Case::new(
        "package main
type Bag struct {
    Items Array<int>
    Nested Array<Array<int>> // trailing comment
    Views Array<[]int> /* block
    comment */ Count int
    Last Array<int>
}
func main() {
    let more = count >
        limit
}
",
    );
    case.assert_clean();
    let Item::Struct(bag) = &case.parsed.file.items[0] else {
        panic!("expected a struct");
    };
    let fields: Vec<String> = bag
        .fields
        .iter()
        .map(|f| format!("{} {}", f.name.text, ty(&case, &f.ty)))
        .collect();
    assert_eq!(
        fields,
        [
            "Items Array<int>",
            "Nested Array<Array<int>>",
            "Views Array<[]int>",
            "Count int",
            "Last Array<int>",
        ]
    );
    assert_eq!(case.shape(), ["(let more (> count limit))"]);
    rejects_file(
        "package main\nfunc f(\n    xs Array<int>\n) {}\n",
        "missing trailing comma",
    );
    rejects("let xs Array<int>\n= other", "expected");
    rejects("let xs Array<Array<int>\n> = other", "expected `>`");
    rejects_file(
        "package main\nfunc f() Array<int>\n{ return xs }\n",
        "expected",
    );
    Case::new("package main\nfunc f(\n    xs Array<int>,\n) {}\n").assert_clean();
    let case = Case::new("package main\nfunc main() {\n    let xs = Array<int>\r\n{1}\n}\n");
    let at_line_end = case.render();
    assert!(at_line_end.contains("test.ore:3:24"), "{at_line_end}");
}

#[test]
fn map_types_and_literals() {
    let case = Case::body(
        "let scores map[string]int = map[string]int{\"Ada\": 10, \"Lin\": 20}
        let empty = map[string]int{}
        let nested map[string]Array<int> = nested
        let many Array<map[bool]rune> = many
        let grid = map[int]int{
            1: 2,
            3: 4,
        }
        if map[int]bool{1: true}[1] == true { work() }
        let found, value = scores[\"Ada\"]
        scores[\"Ada\"] = 30
        let removed, old = scores.remove(\"Ada\")",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let scores map[string]int (lit map[string]int \"Ada\":10 \"Lin\":20))",
            "(let empty (lit map[string]int))",
            "(let nested map[string]Array<int> nested)",
            "(let many Array<map[bool]rune> many)",
            "(let grid (lit map[int]int 1:2 3:4))",
            "(if (== (index (lit map[int]bool 1:true) 1) true) {(call work)})",
            "(let found,value (index scores \"Ada\"))",
            "(= (index scores \"Ada\") 30)",
            "(let removed,old (call (. scores remove) \"Ada\"))",
        ]
    );
    for (body, message) in [
        (
            "let m = map[string]int{\"a\": 1,\n\"b\": 2\n}",
            "missing trailing comma",
        ),
        ("let m = map[string]int{\"a\" 1}", "expected `:`"),
        ("let m = map[string]{}", "expected a type"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn mut_before_a_slice_type_is_part_of_the_type() {
    let case = Case::new(
        "package main
        type View struct { Items []int }
        func edit(items mut []int, fixed mut [int; 3], again mut mut []int) mut []int { return items }
        ",
    );
    case.assert_clean();
    let Item::Func(edit) = &case.parsed.file.items[1] else {
        panic!("expected a function");
    };
    let params: Vec<(ParamMode, String)> = edit
        .params
        .iter()
        .map(|p| (p.mode, ty(&case, &p.ty)))
        .collect();
    assert_eq!(
        params,
        [
            (ParamMode::Borrow, "mut []int".to_string()),
            (ParamMode::Mut, "[int ; 3]".to_string()),
            (ParamMode::Mut, "mut []int".to_string()),
        ]
    );
    assert_eq!(ty(&case, &edit.results[0]), "mut []int");
}

#[test]
fn invalid_array_syntax_is_rejected() {
    for (body, message) in [
        ("let xs [int 3] = xs", "`;`"),
        ("_ = xs[0:1:2]", "at most two bounds"),
        ("_ = xs[::2]", "at most two bounds"),
        ("let s = []int{1, 2}", "slice literals are not part of Zore"),
        (
            "var x mut [int; 3] = xs",
            "`mut` in a type only forms a mutable slice type",
        ),
        (
            "let xs = [int; 3]{1,\n2,\n3\n}\n_ = xs",
            "missing trailing comma",
        ),
    ] {
        rejects(body, message);
    }
}

#[test]
fn control_flow_statements() {
    let case = Case::body(
        "if ready { work() }
        if (ready) { work() } else { rest() }
        if a { x() } else if b { y() } else { z() }
        for { break }
        for ready() { continue }
        for var i = 0; i < limit; i += 1 { work() }
        for i = 0; i < limit; i += 1 { work() }
        for reset(); running(); step() { work() }
        { let inner = 1 }
        return",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(if ready {(call work)})",
            "(if (paren ready) {(call work)} else {(call rest)})",
            "(if a {(call x)} else (if b {(call y)} else {(call z)}))",
            "(for {break})",
            "(for (call ready) {continue})",
            "(for (var i 0); (< i limit); (+= i 1) {(call work)})",
            "(for (= i 0); (< i limit); (+= i 1) {(call work)})",
            "(for (call reset); (call running); (call step) {(call work)})",
            "{(let inner 1)}",
            "(return)",
        ]
    );
}

#[test]
fn collection_loops_parse() {
    let case = Case::body(
        "for item in items { work(item) }
        for i, item in list[1:] { work(i) }
        for _, value in scores { work(value) }
        for key, _ in Array<int>{1, 2} { }
        for ready() { }",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(for item in items {(call work item)})",
            "(for i,item in (slice list 1 _) {(call work i)})",
            "(for _,value in scores {(call work value)})",
            "(for key,_ in (lit Array<int> 1 2) {})",
            "(for (call ready) {})",
        ]
    );
}

#[test]
fn package_qualified_names_parse() {
    let case = Case::body(
        "let a pkg.User = pkg.User{Name: \"x\", Tags: [pkg.Tag; 0]{}}
        let b = pkg.make(1)
        let c map[string]pkg.User = map[string]pkg.User{}
        var d Array<pkg.User> = Array<pkg.User>{}
        if pkg.Ready { work() }
        for _, user in d { work(user) }",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let a pkg.User (lit pkg.User Name:\"x\" Tags:(lit [pkg.Tag ; 0])))",
            "(let b (call (. pkg make) 1))",
            "(let c map[string]pkg.User (lit map[string]pkg.User))",
            "(var d Array<pkg.User> (lit Array<pkg.User>))",
            "(if (. pkg Ready) {(call work)})",
            "(for _,user in d {(call work user)})",
        ]
    );
}

#[test]
fn invalid_control_flow_syntax_is_rejected() {
    for (body, message) in [
        ("if ready work()", "expected `{`"),
        ("if ready\n{ work() }", "same line as the `if` condition"),
        (
            "if a { x() }\nelse { y() }",
            "`else` must be on the same line",
        ),
        ("if let x = 1; x { }", "expected an expression"),
        ("for ;; { }", "cannot be empty"),
        (
            "for var i = 0; i < n; var j = 1 { }",
            "cannot be a declaration",
        ),
        ("for var i = 0; i < n\n i += 1 { }", "on one line"),
        ("for i = 0 { }", "`;` after the loop initializer"),
        ("for var i = 0; ; i += 1 { }", "expected an expression"),
        ("for i = 0; i < n; { }", "expected an expression"),
        ("for ready()\n{ }", "same line as the `for` header"),
        ("for x in items\n{ }", "same line as the `for` header"),
        ("for x, y, z in items { }", "expected"),
        ("for in items { }", "expected"),
        ("let in = 1", "`in` is a keyword"),
        ("x++", "not a Zore operator"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn statement_boundaries_are_honored_by_the_parser() {
    Case::body("greet(\nuser,\n)").assert_clean();
    Case::new("package main\nfunc main() { greet(user) }").assert_clean();
    Case::body("let a = 1; let b = 2").assert_clean();
    Case::body("let total = first +\n second").assert_clean();
    rejects(
        "greet(\nuser\n)",
        "missing trailing comma after the last argument",
    );
    rejects("let total = first\n+ second", "not a statement");
    rejects("return\nvalue", "not a statement");
    rejects(
        "let a = 1 let b = 2",
        "expected newline or `;` after statement",
    );

    // The unsupported continuation must not turn into one expression.
    let case = Case::body("let total = first\n+ second");
    assert_eq!(case.shape()[0], "(let total first)");
}

#[test]
fn package_and_imports() {
    let case = Case::new("package main\nimport \"zore/fmt\"\nimport \"net\"\nfunc main() {}");
    case.assert_clean();
    let paths: Vec<&str> = case
        .parsed
        .file
        .imports
        .iter()
        .map(|i| i.path.as_str())
        .collect();
    assert_eq!(paths, ["zore/fmt", "net"]);
    assert_eq!(
        case.text(case.parsed.file.imports[0].span),
        "import \"zore/fmt\""
    );

    rejects_file("func main() {}", "expected `package` clause");
    rejects_file("// comment only\n", "expected `package` clause");
    rejects_file("package func\n", "`func` is a keyword");
    rejects_file(
        "package main\nfunc main() {}\nimport \"x\"\n",
        "imports must come before",
    );
    rejects_file("package main\nimport (\n\"x\"\n)\n", "grouped imports");
    rejects_file("package main\nimport `x`\n", "double-quoted import path");
    rejects_file("package main\nimport x\n", "double-quoted import path");
    rejects_file("package main extra\n", "after declaration");
    rejects_file("package main\nprintln(\"x\")\n", "expected a declaration");
}

#[test]
fn package_level_bindings_are_parsed() {
    let case = Case::new("package main\nconst limit = 10\nlet name = \"x\"\nvar count = 0\n");
    case.assert_clean();
    assert_eq!(case.parsed.file.items.len(), 3);
}

#[test]
fn reserved_words_are_rejected() {
    rejects("defer cleanup()", "`defer` is reserved");
    rejects("unsafe { }", "`unsafe` is reserved");
    rejects_file("package main\nenum Color {}\n", "`enum` is reserved");
}

#[test]
fn recovery_reports_each_statement_once_and_continues() {
    let case = Case::new(
        "package main
        func first() {
            let = 1
            let ok = 2
            greet(,)
            if { }
        }
        func second( {
        }
        func third() {
            1 + 2
        }",
    );
    let errors = case.errors();
    let messages: Vec<&str> = errors.iter().map(|(m, _)| *m).collect();
    assert_eq!(messages.len(), 5, "{errors:#?}");
    assert!(
        messages[0].contains("expected a binding name"),
        "{errors:#?}"
    );
    assert!(
        messages[1].contains("expected an expression"),
        "{errors:#?}"
    );
    assert!(
        messages[2].contains("expected an expression"),
        "{errors:#?}"
    );
    assert!(
        messages[3].contains("expected a parameter name"),
        "{errors:#?}"
    );
    assert_eq!(messages[4], "expression is not a statement");
    let names: Vec<&str> = case
        .parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) => Some(f.name.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["first", "third"]);
    let Item::Func(first) = &case.parsed.file.items[0] else {
        panic!()
    };
    assert_eq!(first.body.stmts.len(), 1);
}

#[test]
fn lexical_errors_are_not_reported_twice() {
    let case = Case::body("let x = @\nlet y = 0x\nlet z = \"open");
    let errors = case.errors();
    assert_eq!(errors.len(), 3, "{errors:#?}");
    assert_eq!(errors[0].0, "unexpected character '@'");
    assert!(errors[1].0.contains("hexadecimal digits"));
    assert_eq!(errors[2].0, "unterminated string literal");
    // Malformed literals still yield an expression so the binding is kept.
    assert_eq!(case.shape()[0], "(let y <malformed>)");
}

#[test]
fn unclosed_brace_points_at_both_ends() {
    let case = Case::new("package main\nfunc main() {\n    work()\n");
    assert_eq!(case.errors().len(), 1, "{:#?}", case.errors());
    let rendered = case.render();
    assert!(rendered.contains("error: unclosed `{`"), "{rendered}");
    assert!(rendered.contains("test.ore:2:13"), "{rendered}");
    assert!(rendered.contains("opened here"), "{rendered}");
}

#[test]
fn parsing_terminates_on_arbitrary_token_sequences() {
    const PIECES: &[&str] = &[
        "package ", "main ", "func ", "f", "(", ")", "{", "}", "[", "]", "let ", "x", "=", "1",
        ",", ";", "\n", "if ", "for ", "else ", "return ", ".", "?", "await ", "+", "<", "_",
        "type ", "struct ", "\"s\"", "@", ":", "own ", "go ", "import ", "async ", "const ",
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..20000 {
        let mut text = String::new();
        for _ in 0..(state % 40) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            text.push_str(PIECES[(state % PIECES.len() as u64) as usize]);
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let case = Case::new(&text);
        for diagnostic in &case.parsed.diagnostics {
            assert!(case.sources.slice(diagnostic.span()).is_some(), "{text:?}");
        }
    }
}

#[test]
fn function_literals_parse_as_expressions() {
    for (source, expected) in [
        ("func() {}", "(func () () {})"),
        (
            "func(a int, b mut int, c own User) int { return a }",
            "(func (a int, b mut int, c own User) (int) {(return a)})",
        ),
        (
            "func() (int, error) { return 1, nil }",
            "(func () (int, error) {(return 1 nil)})",
        ),
        ("func(s mut []int) {}", "(func (s mut []int) () {})"),
        (
            "func(x int) int { return x }(2)",
            "(call (func (x int) (int) {(return x)}) 2)",
        ),
        (
            "apply(func(v int) int { return v * 2 }, 3)",
            "(call apply (func (v int) (int) {(return (* v 2))}) 3)",
        ),
    ] {
        assert_eq!(expr_shape(source), expected, "{source}");
    }
    let case = Case::body("let f = func() {\n    work()\n    count += 1\n}\nf()");
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let f (func () () {(call work); (+= count 1)}))",
            "(call f)"
        ]
    );
    // A literal may appear in a condition header, since its body is braced.
    Case::body("if check(func() bool { return true }) {\n}").assert_clean();
}

#[test]
fn function_types_parse_in_type_positions() {
    let case = Case::new(
        "package main\nfunc run(f func(int, mut int, own User) (int, error), g func(), h func(func(int)) bool, s func(mut []int)) {}\n",
    );
    case.assert_clean();
    let Some(Item::Func(func)) = case.parsed.file.items.first() else {
        panic!("expected a function");
    };
    let types: Vec<String> = func.params.iter().map(|p| ty(&case, &p.ty)).collect();
    assert_eq!(
        types,
        [
            "func(int, mut int, own User) (int, error)",
            "func() ()",
            "func(func(int) ()) (bool)",
            "func(mut []int) ()",
        ]
    );
    let case = Case::body("let f func(int) int = g\nvar h func() = g");
    case.assert_clean();
    assert_eq!(
        case.shape(),
        ["(let f func(int) (int) g)", "(var h func() () g)"]
    );
}

#[test]
fn async_function_types_parse_in_type_positions() {
    let case = Case::new(
        "package main\ntype Route struct {\n    path string\n    handler async func(int) (string, error)\n}\nfunc run(op async func(), more async func(int) async func() int) async func(int) int {}\n",
    );
    case.assert_clean();
    let Some(Item::Struct(route)) = case.parsed.file.items.first() else {
        panic!("expected a struct");
    };
    let fields: Vec<String> = route.fields.iter().map(|f| ty(&case, &f.ty)).collect();
    assert_eq!(fields, ["string", "async func(int) (string, error)"]);
    let Some(Item::Func(func)) = case.parsed.file.items.get(1) else {
        panic!("expected a function");
    };
    let params: Vec<String> = func.params.iter().map(|p| ty(&case, &p.ty)).collect();
    assert_eq!(
        params,
        ["async func() ()", "async func(int) (async func() (int))"]
    );
    assert_eq!(
        func.results
            .iter()
            .map(|r| ty(&case, r))
            .collect::<Vec<_>>(),
        ["async func(int) (int)"]
    );
    let case = Case::body("let f async func(int) int = g");
    case.assert_clean();
    assert_eq!(case.shape(), ["(let f async func(int) (int) g)"]);
}

#[test]
fn a_named_parameter_of_async_function_type_is_rejected() {
    rejects_file(
        "package main\nfunc run(f async func(x int)) {}\n",
        "function type parameters have no names",
    );
    rejects_file(
        "package main\nfunc run(f func(x async func())) {}\n",
        "function type parameters have no names",
    );
}

#[test]
fn malformed_function_literals_and_types_are_rejected() {
    rejects("let f = func named() {}", "a function literal has no name");
    rejects("let f = func()", "function literals require a body");
    rejects("let f = func() {", "unclosed `{`");
    rejects("let f = func(a, b int) {}", "needs its own type");
    rejects_file(
        "package main\nfunc run(f func(x int)) {}\n",
        "function type parameters have no names",
    );
    rejects_file(
        "package main\nfunc run(f func() (int)) {}\n",
        "a single result type is written without parentheses",
    );
}

#[test]
fn select_arms_and_default() {
    let case = Case::body(
        "select {
            case let v, ok = a.receive() { use(v) }
            case b.receive() { }
            case c.send(1) { }
            default { }
        }",
    );
    case.assert_clean();
    let shape = case.shape();
    assert_eq!(shape.len(), 1);
    assert!(
        shape[0].starts_with("(select (case (let v,ok "),
        "{}",
        shape[0]
    );
    assert!(shape[0].contains("(default "), "{}", shape[0]);
}

#[test]
fn case_and_default_stay_identifiers_outside_select() {
    Case::body("let case = 1\nlet default = case + 1").assert_clean();
}

#[test]
fn malformed_select_is_rejected() {
    for source in [
        "select { }",
        "select { default { } default { } }",
        "select { foo { } }",
        "select { case a.receive() }",
    ] {
        let case = Case::body(source);
        assert!(!case.errors().is_empty(), "{source} parsed");
    }
}
