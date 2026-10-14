use super::{accepts, let_value, program, rejects};
use zore::hir::ExprKind;

#[test]
fn async_literals_keep_async_types_and_independent_nested_body_contexts() {
    let case = accepts(&program(
        r#"async func pause() int { return 1 }
async func outer() int {
    let plain = func() int {
        let nested = async func() int { return await pause() }
        let task = go nested()
        return task.wait()
    }
    let job = async func(n int) int { return n + await pause() }
    return plain() + await job(2)
}
func acceptsAsync(f async func(int) int) {}
func create() { let f = async func(n int) int { return n }; acceptsAsync(f) }
"#,
    ));
    let value = let_value(case.function("outer"), "job");
    let signature = case.package().types.func_signature(value.ty()).unwrap();
    assert!(signature.is_async);
    let ExprKind::Closure { function, .. } = value.kind else {
        panic!("expected closure")
    };
    assert!(case.package().function(function).is_async);
    for (source, expected) in [
        (
            "async func outer() { let f = func() { let n = await pause() }; f() }",
            "this function literal is not async",
        ),
        (
            "func outer() { let f = async func() int { return 1 }; let n = f() }",
            "neither awaited nor spawned",
        ),
        (
            "async func outer() { let f = async func() int { return 1 }; let g func() int = f }",
            "expected",
        ),
        (
            "async func outer() { let f = func() int { return 1 }; let g async func() int = f }",
            "expected",
        ),
        (
            "func outer() { let f = async func() { await plain() } }\nfunc plain() {}",
            "`await` needs",
        ),
        (
            "func outer() { let f = async func() int { await pause() } }",
            "without returning",
        ),
    ] {
        rejects(
            &program(&format!("async func pause() int {{ return 1 }}\n{source}")),
            expected,
        );
    }
}

#[test]
fn async_method_values_keep_receiver_modes_and_existing_place_restrictions() {
    let prelude = r#"type Counter struct { N int }
type Owner struct { Counter Counter }
async func (c Counter) read() int { return c.N }
async func (c mut Counter) bump(n int) int { c.N += n; return c.N }
func make() Counter { return Counter{N: 1} }
"#;
    let case = accepts(&program(&format!(
        r#"{prelude}
async func work() int {{
    var owner = Owner{{Counter: Counter{{N: 1}}}}
    let read = owner.Counter.read
    let n = await read()
    let bump = owner.Counter.bump
    return await bump(n)
}}"#
    )));
    for name in ["read", "bump"] {
        let value = let_value(case.function("work"), name);
        assert!(
            case.package()
                .types
                .func_signature(value.ty())
                .unwrap()
                .is_async
        );
        let ExprKind::Closure { function, .. } = value.kind else {
            panic!("expected method wrapper")
        };
        assert!(case.package().function(function).is_async);
    }
    accepts(&program(
        "type Counter int\nasync func (c mut Counter) bump() { c += Counter(1) }\nasync func work() { var c = Counter(1); let f = c.bump; await f() }",
    ));
    for (body, expected) in [
        ("let f = make().read", "receiver of a method value"),
        ("let c = Counter{N: 1}; let f = c.bump", "immutable"),
        (
            "let c = Counter{N: 1}; let f = c.read; f()",
            "neither awaited nor spawned",
        ),
        (
            "let values = Array<Counter>{Counter{N: 1}}; let f = values[0].read",
            "receiver of a method value",
        ),
    ] {
        rejects(
            &program(&format!("{prelude}\nfunc work() {{ {body} }}")),
            expected,
        );
    }
    rejects(
        &program(
            "type Box<T any> struct { Value T }\nasync func (b Box<T>) read() T { return b.Value }\nfunc work() { let b = Box<int>{Value: 1}; let f = b.read }",
        ),
        "generic type",
    );
}
