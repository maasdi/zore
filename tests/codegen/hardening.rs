use super::{TempDir, bounded_process, controlled_runtime, stderr, stdout};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::Command;
use std::time::Duration;
use zore::check::check_file;
use zore::source::SourceMap;

pub(super) struct Generator(u64);

impl Generator {
    pub(super) fn seeded(seed: u64) -> Self {
        let mut mixed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;
        Self(if mixed == 0 { 1 } else { mixed })
    }

    pub(super) fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }
}

pub(super) fn seeds() -> Vec<u64> {
    match std::env::var("ZORE_HARDENING_SEED") {
        Ok(value) => vec![value.parse().expect("ZORE_HARDENING_SEED must be a u64")],
        Err(_) => vec![158],
    }
}

fn failure_text(error: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = error.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = error.downcast_ref::<&str>() {
        (*message).into()
    } else {
        "non-string panic".into()
    }
}

fn verify(
    seed: u64,
    label: &str,
    atoms: Vec<usize>,
    render: impl Fn(&[usize]) -> String,
    assertion: impl Fn(&str, &[usize]),
) {
    let original = render(&atoms);
    let Err(error) = catch_unwind(AssertUnwindSafe(|| assertion(&original, &atoms))) else {
        return;
    };
    let directory = std::env::var_os("ZORE_HARDENING_FAILURES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../target/hardening-failures")
        })
        .join(format!("{label}-{seed}"));
    fs::create_dir_all(&directory).expect("create failure artifacts");
    fs::write(directory.join("original.ore"), original).unwrap();
    fs::write(
        directory.join("original-operations.txt"),
        format!("{atoms:?}\n"),
    )
    .unwrap();
    let failure = failure_text(error);
    fs::write(
        directory.join("failure.txt"),
        format!("seed={seed}\nlabel={label}\n{failure}\n"),
    )
    .unwrap();
    let expensive_failure = failure.contains("time limit") || failure.contains("output limit");
    let (reduced, attempts) = reduce(atoms, &failure, &render, &assertion);
    fs::write(directory.join("reduced.ore"), render(&reduced)).unwrap();
    fs::write(
        directory.join("reduced-operations.txt"),
        format!("{reduced:?}\n"),
    )
    .unwrap();
    fs::write(
        directory.join("reduction.txt"),
        format!("attempts={attempts}\nskipped_expensive_failure={expensive_failure}\n"),
    )
    .unwrap();
    panic!(
        "hardening failure: seed {seed}, {label}; artifacts {}; replay with ZORE_HARDENING_SEED={seed}",
        directory.display()
    );
}

fn failure_signature(failure: &str) -> Option<&str> {
    let failure = failure.lines().next()?;
    let start = failure.find("hardening[")?;
    let end = start + failure[start..].find(']')? + 1;
    Some(&failure[start..end])
}

fn reduce(
    atoms: Vec<usize>,
    failure: &str,
    render: &impl Fn(&[usize]) -> String,
    assertion: &impl Fn(&str, &[usize]),
) -> (Vec<usize>, usize) {
    let mut reduced = atoms;
    let mut attempts = 0;
    let mut index = reduced.len();
    let expensive_failure = failure.contains("time limit") || failure.contains("output limit");
    let signature = failure_signature(failure);
    while index > 0 && attempts < 4 && !expensive_failure && signature.is_some() {
        index -= 1;
        let mut candidate = reduced.clone();
        candidate.remove(index);
        attempts += 1;
        if let Err(error) = catch_unwind(AssertUnwindSafe(|| {
            assertion(&render(&candidate), &candidate)
        })) {
            let candidate_failure = failure_text(error);
            if failure_signature(&candidate_failure) == signature {
                reduced = candidate;
            }
        }
    }
    (reduced, attempts)
}

#[test]
fn reduction_preserves_failure_class_and_skips_timeouts() {
    let render = |atoms: &[usize]| format!("{atoms:?}");
    let assertion = |_: &str, atoms: &[usize]| {
        assert!(atoms.contains(&7), "hardening[validity/candidate]");
        panic!("hardening[execution/mismatch]");
    };
    let (reduced, attempts) = reduce(
        vec![1, 7, 2],
        "hardening[execution/mismatch]",
        &render,
        &assertion,
    );
    assert_eq!(reduced, vec![7]);
    assert_eq!(attempts, 3);
    let (unchanged, attempts) = reduce(vec![1, 7, 2], "time limit", &render, &assertion);
    assert_eq!(unchanged, vec![1, 7, 2]);
    assert_eq!(attempts, 0);
}

fn checked(source: &str, expected: Option<&str>) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("check.ore");
    fs::write(&path, source).unwrap();
    let output = bounded_process::run(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hardening::generated_checker_child",
                "--nocapture",
            ])
            .env("ZORE_GENERATED_CHECK_FILE", path)
            .env("ZORE_GENERATED_CHECK_REJECTION", expected.unwrap_or("")),
        Duration::from_secs(15),
    )
    .expect("bounded generated checker");
    assert!(
        output.status.success(),
        "hardening[checker/expected-outcome]: {}",
        stderr(&output)
    );
}

#[test]
fn generated_checker_child() {
    let Some(path) = std::env::var_os("ZORE_GENERATED_CHECK_FILE") else {
        return;
    };
    let source = fs::read_to_string(path).unwrap();
    let expectation = std::env::var("ZORE_GENERATED_CHECK_REJECTION").unwrap();
    let expected = (!expectation.is_empty()).then_some(expectation.as_str());
    let mut sources = SourceMap::new();
    let id = sources.add("generated.ore", source).unwrap();
    let result = check_file(sources.file(id).unwrap());
    let messages: Vec<_> = result.diagnostics.iter().map(|d| d.message()).collect();
    match expected {
        None => assert!(
            messages.is_empty() && result.package.is_some(),
            "hardening[checker/acceptance]: {messages:?}"
        ),
        Some(expected) => assert!(
            result.package.is_none() && messages.iter().any(|message| message.contains(expected)),
            "hardening[checker/rejection]: expected {expected:?}, actual {messages:?}"
        ),
    }
}

fn run(source: &str) -> std::process::Output {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("generated");
    let optimization = match std::env::var("ZORE_HARDENING_OPTIMIZE").as_deref() {
        Ok("2") => "-O2",
        Ok("1") => "-O1",
        _ => "-O0",
    };
    controlled_runtime::build_with_optimization(source, &executable, 2, optimization);
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .expect("bounded generated program")
}

fn success(source: &str, expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "hardening[execution/status]: {}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), expected, "hardening[execution/stdout]");
    assert!(
        output.stderr.is_empty(),
        "hardening[execution/stderr]: {}",
        stderr(&output)
    );
}

#[test]
fn seeded_cloned_view_move_chains_preserve_acceptance_and_conflict_rejection() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        for shape in 0..3 {
            let (ty, value, read) = match shape {
                0 => (
                    "Array<View>",
                    "Array<View>{View{items: data[:]}}",
                    "chainLAST[0].items[0]",
                ),
                1 => (
                    "Array<Array<View>>",
                    "Array<Array<View>>{Array<View>{View{items: data[:]}}}",
                    "chainLAST[0][0].items[0]",
                ),
                _ => (
                    "Box",
                    "Box{view: View{items: data[:]}, owned: Array<int>{1}}",
                    "chainLAST.view.items[0]",
                ),
            };
            let atoms = (0..1 + generator.next(6))
                .map(|_| generator.next(2))
                .collect::<Vec<_>>();
            for rejected in [false, true] {
                verify(
                    seed,
                    &format!("view-{shape}-{rejected}"),
                    atoms.clone(),
                    |atoms| {
                        let mut chain = String::new();
                        for (index, operation) in atoms.iter().enumerate() {
                            if *operation == 0 {
                                chain.push_str(&format!("let chain{} = chain{index}\n", index + 1));
                            } else {
                                chain.push_str(&format!(
                                    "let chain{} = clone(chain{index}); drop(chain{index})\n",
                                    index + 1
                                ));
                            }
                        }
                        let read = read.replace("LAST", &atoms.len().to_string());
                        let conflict = if rejected { "data[0] = 9;" } else { "" };
                        format!(
                            "package main\ntype View struct {{ items []int }}\ntype Box struct {{ view View; owned Array<int> }}\nfunc main() {{ var data = [int; 1]{{7}}; {{ let original {ty} = {value}; let chain0 = clone(original); drop(original); {chain} {conflict} println({read}) }}; data[0] = 9 }}"
                        )
                    },
                    |source, _| checked(source, rejected.then_some("borrowed")),
                );
            }
        }
    }
}

fn scoped_program(seed: u64, asynchronous: bool, exit: usize, atoms: &[usize]) -> String {
    let value = seed % 97 + 1;
    let mode = if asynchronous { "async" } else { "" };
    let checkpoint = if asynchronous {
        "waitPending(2)"
    } else {
        "waitBlocked(2)"
    };
    let mut chain = String::new();
    for (index, operation) in atoms.iter().enumerate() {
        if *operation == 0 {
            chain.push_str(&format!("let chain{} = chain{index}\n", index + 1));
        } else {
            chain.push_str(&format!(
                "let chain{} = clone(chain{index}); drop(chain{index})\n",
                index + 1
            ));
        }
    }
    let leave = match exit {
        0 => "return nil",
        1 => "fail()?; return nil",
        2 => "panic(\"parent panic\")",
        _ => "return nil",
    };
    let join = if exit < 2 {
        format!(
            "let err = task.wait(); println(err == {});",
            if exit == 0 {
                "nil"
            } else {
                "error(\"propagated\")"
            }
        )
    } else {
        "drop(task);".into()
    };
    format!(
        r#"package main
type Resource struct {{ id int; explode bool }}
func (r mut Resource) drop() {{ println(r.id); if r.explode {{ panic("destructor panic") }} }}
type Pair struct {{ first Resource; second Resource }}
type View struct {{ items []int }}
func fail() error {{ return error("propagated") }}
{mode} func parent(ready channel<bool>, gate channel<bool>) error {{
    var pair = Pair{{first: Resource{{id: 10, explode: false}}, second: Resource{{id: 20, explode: {explode}}}}}
    let moved = pair.first
    pair.first = Resource{{id: 30, explode: false}}
    let backing = [int; 1]{{{value}}}
    let original = Array<Array<View>>{{Array<View>{{View{{items: backing[:]}}}}}}
    let chain0 = clone(original)
    drop(original)
    {chain}
    scope {{
        go {mode} func() {{
            let _, _ = gate.receive()
            println(chain{last}[0][0].items[0] + moved.id)
        }}()
        ready.send(true)
        {leave}
    }}
}}
func main() {{
    let ready = channel<bool>()
    let gate = channel<bool>()
    let task = go parent(ready, gate)
    let _, _ = ready.receive()
    {checkpoint}
    gate.close()
    {join}
    waitIdle(0)
    println("quiescent")
}}
"#,
        explode = exit == 3,
        last = atoms.len()
    )
}

#[test]
fn seeded_scoped_captures_match_plain_async_cleanup_after_observed_join() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..1 + generator.next(4))
            .map(|_| generator.next(2))
            .collect::<Vec<_>>();
        for exit in 0..4 {
            for asynchronous in [false, true] {
                verify(
                    seed,
                    &format!("scope-{exit}-{asynchronous}"),
                    atoms.clone(),
                    |atoms| scoped_program(seed, asynchronous, exit, atoms),
                    |source, _| {
                        let output = run(source);
                        assert!(
                            output.status.success(),
                            "hardening[scope/status]: {}",
                            stderr(&output)
                        );
                        assert_eq!(
                            stdout(&output),
                            format!(
                                "{}\n10\n20\n30\n{}quiescent\n",
                                seed % 97 + 11,
                                if exit < 2 { "true\n" } else { "" }
                            ),
                            "hardening[scope/cleanup-trace]"
                        );
                        if exit < 2 {
                            assert!(
                                output.stderr.is_empty(),
                                "hardening[scope/no-panic]: {}",
                                stderr(&output)
                            );
                        } else {
                            assert!(
                                stderr(&output).contains(if exit == 2 {
                                    "parent panic"
                                } else {
                                    "destructor panic"
                                }),
                                "hardening[scope/expected-panic]: {}",
                                stderr(&output)
                            );
                            assert!(
                                !stderr(&output).contains("leak:"),
                                "hardening[scope/no-leak]: {}",
                                stderr(&output)
                            );
                        }
                    },
                );
            }
        }
    }
}

#[test]
fn seeded_partial_construction_propagation_matches_plain_and_async() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..8).map(|_| generator.next(2)).collect::<Vec<_>>();
        for asynchronous in [false, true] {
            verify(
                seed,
                &format!("construction-{asynchronous}"),
                atoms.clone(),
                |atoms| {
                    let mode = if asynchronous { "async" } else { "" };
                    let call = if asynchronous { "await " } else { "" };
                    let mut cases = String::new();
                    for (index, fail) in atoms.iter().enumerate() {
                        cases.push_str(&format!("let err{index} = {call}construct({}, {}); println(err{index} != nil)\n", 10 + index * 10, fail != &0));
                    }
                    format!(
                        r#"package main
type Resource struct {{ id int; data Array<int> }}
func (r mut Resource) drop() {{ if r.id != 0 {{ println(r.id); if r.data[0] != r.id {{ panic("corrupt resource") }} }} }}
type Pair struct {{ first Resource; second Resource }}
func resource(id int) Resource {{ return Resource{{id: id, data: Array<int>{{id}}}} }}
{mode} func item(id int, fail bool) (Resource, error) {{ if fail {{ return resource(id), error("failure") }}; return resource(id), nil }}
{mode} func construct(id int, fail bool) error {{
    let partial = Pair{{first: resource(id), second: {call}item(id + 1, fail)?}}
    return nil
}}
{mode} func exercise() {{ {cases} }}
func main() {{ let task = go exercise(); task.wait(); waitIdle(0) }}
"#
                    )
                },
                |source, atoms| {
                    let output = run(source);
                    assert!(
                        output.status.success(),
                        "hardening[construction/status]: {}",
                        stderr(&output)
                    );
                    assert!(
                        output.stderr.is_empty(),
                        "hardening[construction/stderr]: {}",
                        stderr(&output)
                    );
                    let lines = stdout(&output);
                    let mut expected = String::new();
                    for (index, fail) in atoms.iter().enumerate() {
                        let base = 10 + index * 10;
                        expected.push_str(&format!("{}\n{base}\n{}\n", base + 1, *fail != 0));
                    }
                    assert_eq!(lines, expected, "hardening[construction/cleanup-trace]");
                },
            );
        }
    }
}

#[test]
fn seeded_json_nested_documents_reject_truncation_and_preserve_owned_values() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..12).map(|_| generator.next(1000)).collect::<Vec<_>>();
        verify(
            seed,
            "json",
            atoms,
            |atoms| {
                let mut cases = String::new();
                for number in atoms {
                    let depth = number % 4 + 1;
                    let leaf = format!("{{\"n\":{number},\"s\":\"x\\n\\u0041\"}}");
                    let document = format!("{}{}{}", "[".repeat(depth), leaf, "]".repeat(depth));
                    let malformed = [
                        document[..document.len() - 1].to_string(),
                        format!("{document}x"),
                        format!("[{document},]"),
                    ];
                    cases.push_str(&format!("valid({document:?}, {number}, {depth})?\n"));
                    for bad in malformed {
                        cases.push_str(&format!("invalid({bad:?})\n"));
                    }
                }
                format!(
                    r#"package main
import "std/encoding/json"
import "std/strings"
func require(ok bool) {{ if !ok {{ panic("JSON oracle") }} }}
func valid(document string, number int, depth int) error {{
    let data = strings.Bytes(document)
    require(json.Valid(data[:]))
    let tree = json.Parse(data[:])?
    var copied = clone(tree)
    drop(tree)
    for var index = 0; index < depth; index += 1 {{ copied = copied.Index(0) }}
    require(copied.Get("n").Int()? == number)
    require(copied.Get("s").String()? == "x\nA")
    return nil
}}
func invalid(document string) {{
    let data = strings.Bytes(document)
    require(!json.Valid(data[:]))
    let value, err = json.Parse(data[:])
    require(err != nil)
    require(value.IsNull())
}}
func exercise() error {{ {cases} return nil }}
func main() {{ let err = exercise(); if err != nil {{ panic("JSON error") }}; waitIdle(0); println("clean") }}"#
                )
            },
            |source, _| success(source, "clean\n"),
        );
    }
}

#[test]
fn seeded_http_fragment_boundaries_keep_framing_and_rejection_oracles() {
    let mut fixture = super::http::http_sources();
    for (pattern, replacement) in [
        (
            "connections int\n",
            "connections int\n    wire Array<byte>\n    width int\n    cursor int\n",
        ),
        (
            "connections: 0}",
            "connections: 0, wire: Array<byte>{}, width: 1, cursor: 0}",
        ),
        (
            "r.conn[0].SetReadDeadline(pollDeadline(r.deadline, r.config.AcceptPollInterval))?",
            "",
        ),
        (
            "let n, err = r.conn[0].Read(data[:])",
            "let n, err = fixtureRead(data[:], r.server)",
        ),
    ] {
        assert!(
            fixture.contains(pattern),
            "HTTP fixture pattern missing from the server source: {pattern}"
        );
        fixture = fixture.replace(pattern, replacement);
    }
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..8).map(|_| 1 + generator.next(17)).collect::<Vec<_>>();
        verify(
            seed,
            "http-fragments",
            atoms,
            |atoms| {
                let mut cases = String::new();
                for width in atoms {
                    for (wire, status, body) in [
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello",
                            0,
                            "hello",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n2;key=\"a;b\"\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n",
                            0,
                            "hello",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na",
                            400,
                            "",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
                            400,
                            "",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: +1\r\n\r\n",
                            400,
                            "",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 99999999999999999999\r\n\r\n",
                            400,
                            "",
                        ),
                        (
                            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 33\r\n\r\n",
                            413,
                            "",
                        ),
                        ("GET /echo HTTP/1.0\r\nHost: x\r\n\r\n", 505, ""),
                    ] {
                        cases.push_str(&format!(
                            "exercise({wire:?}, {width}, {status}, {body:?})?\n"
                        ));
                    }
                }
                format!(
                    r#"{fixture}
func require(ok bool) {{ if !ok {{ panic("HTTP oracle") }} }}
func fixtureRead(data mut []byte, server Server) (int, error) {{
    let count = server.state.withLock(func(state mut serverState) int {{
        var count = state.width
        if state.wire.len() - state.cursor < count {{ count = state.wire.len() - state.cursor }}
        if data.len() < count {{ count = data.len() }}
        for var i = 0; i < count; i += 1 {{ data[i] = state.wire[state.cursor + i] }}
        state.cursor += count
        return count
    }})
    if count == 0 {{ return 0, io.EOF }}
    return count, nil
}}
func exercise(wire string, width int, expected int, body string) error {{
    let builder = NewRouterBuilder()
    let router = builder.Build()?
    var config = DefaultConfig()
    config.MaxBodyBytes = 32
    let server = NewServer(router, config)?
    server.state.withLock(func(state mut serverState) {{ state.wire = strings.Bytes(wire); state.width = width }})
    var empty = Array<net.Conn>{{}}
    let _, zero = empty.pop()
    let sockets = [net.Conn; 1]{{zero}}
    var request = newRequest(sockets[:], server, Array<byte>{{}}, time.Now() + time.Hour)
    let status, err = request.parseHeaders()
    require(status == expected)
    require((err == nil) == (expected == 0))
    if expected == 0 {{
        let bytes = io.ReadAll(request)?
        require(strings.FromBytes(bytes[:])? == body)
    }}
    return nil
}}
func all() error {{ {cases} return nil }}
func main() {{ let err = all(); if err != nil {{ panic("HTTP error") }}; waitIdle(0); println("clean") }}
"#
                )
            },
            |source, _| success(source, "clean\n"),
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn seeded_cancellation_trees_release_timers_only_after_quiescence() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..8).map(|_| 1 + generator.next(4)).collect::<Vec<_>>();
        verify(
            seed,
            "cancellation",
            atoms,
            |atoms| {
                let mut cases = String::new();
                for depth in atoms {
                    cases.push_str("{\nlet ctx0, cancel0 = context.WithTimeout(context.Background(), time.Hour)\n");
                    for index in 1..=*depth {
                        cases.push_str(&format!("let ctx{index}, cancel{index} = context.WithTimeout(ctx{}, {} * time.Minute)\n", index - 1, 60 - index));
                    }
                    cases.push_str(&format!(
                        "waitPending({}); checkTimers({})\n",
                        depth * 2 + 1,
                        depth + 1
                    ));
                    if depth % 2 == 0 {
                        cases.push_str(&format!("cancel{depth}()\n"));
                    }
                    cases.push_str("cancel0()\n");
                    for index in (0..=*depth).rev() {
                        cases.push_str(&format!("cancel{index}(); cancel{index}()\n"));
                    }
                    cases.push_str("waitIdle(0); checkTimers(0)\n");
                    for index in 0..=*depth {
                        cases.push_str(&format!("if ctx{index}.Err() != error(\"context canceled\") {{ panic(\"cancellation reason\") }}\n"));
                    }
                    cases.push_str("}\n");
                }
                format!(
                    "package main\nimport \"std/context\"\nimport \"std/time\"\nfunc main() {{ {cases} waitIdle(0); checkTimers(0); println(\"clean\") }}"
                )
            },
            |source, _| success(source, "clean\n"),
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn seeded_tcp_partial_bodies_cancel_and_quiesce() {
    for seed in seeds() {
        let mut generator = Generator::seeded(seed);
        let atoms = (0..3).map(|_| 1 + generator.next(13)).collect::<Vec<_>>();
        verify(
            seed,
            "tcp-cancellation",
            atoms,
            |atoms| {
                let cases = atoms
                    .iter()
                    .map(|width| format!("exercise({width})?\n"))
                    .collect::<String>();
                format!(
                    r#"package main
import "std/net"
import "std/net/http"
import "std/io"
import "std/strings"
import "std/context"
import "std/time"
type Factory struct {{ entered channel<bool> }}
type Handler struct {{ entered channel<bool> }}
func (f Factory) NewHandler() (http.Handler, error) {{ return Handler{{entered: f.entered}}, nil }}
func (h Handler) Handle(w mut http.ResponseWriter, r mut http.Request) error {{
    h.entered.send(true)
    let _, err = io.ReadAll(r)
    if err == nil {{ panic("partial body accepted") }}
    let _, _ = r.Context().Done().receive()
    if r.Context().Err() == nil {{ panic("missing request cancellation") }}
    println("canceled")
    return nil
}}
func exercise(width int) error {{
    let entered = channel<bool>(1)
    var routes = http.NewRouterBuilder()
    routes.HandleFactory("POST /partial", Factory{{entered: entered}})?
    let router = routes.Build()?
    var config = http.DefaultConfig()
    config.AcceptPollInterval = time.Millisecond
    config.ReadBodyTimeout = time.Hour
    let server = http.NewServer(router, config)?
    let listener = net.Listen("tcp", "127.0.0.1:0")?
    let address = listener.Addr()
    let serving = go server.Serve(listener)
    let client = net.Dial("tcp", address)?
    let wire = strings.Bytes("POST /partial HTTP/1.1\r\nHost: x\r\nContent-Length: 8\r\n\r\nab")
    var position = 0
    for position < wire.len() {{
        var end = position + width
        if end > wire.len() {{ end = wire.len() }}
        let count = client.Write(wire[position:end])?
        if count == 0 {{ panic("zero socket write") }}
        position += count
    }}
    let _, _ = entered.receive()
    waitBlocked(2)
    let ctx, cancel = context.WithCancel(context.Background())
    cancel()
    let first = server.Shutdown(ctx)
    if first != error("context canceled") {{ panic("shutdown cancellation oracle") }}
    server.Shutdown(context.Background())?
    serving.wait()?
    return nil
}}
func all() error {{ {cases} return nil }}
func main() {{ let err = all(); if err != nil {{ panic("TCP cancellation error") }}; waitIdle(0); checkTimers(0); println("clean") }}"#
                )
            },
            |source, atoms| {
                success(
                    source,
                    &format!("{}clean\n", "canceled\n".repeat(atoms.len())),
                )
            },
        );
    }
}
