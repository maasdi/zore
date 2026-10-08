use zore::async_lowering::{self, Suspension};
use zore::source::SourceMap;
use zore::{check, dropck, hir, mir};

fn plan(source: &str) -> (hir::Package, mir::Program, async_lowering::Plan) {
    let mut sources = SourceMap::new();
    let id = sources.add("async.ore", source.to_owned()).unwrap();
    let checked = check::check_file(sources.file(id).unwrap());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let mut program = mir::lower::lower(&package);
    dropck::insert(&package, &mut program);
    let plan = async_lowering::lower(&package, &program);
    (package, program, plan)
}

#[test]
fn calls_and_task_awaits_keep_their_cleanup_edges_and_spans() {
    let (package, program, plan) = plan(
        "package main
async func leaf(t own Task<int>) int { return await t }
async func parent(t own Task<int>) int { return await leaf(t) }
func plain() int { return 1 }
func main() { let t = go parent(go plain()); println(t.wait()) }",
    );
    for (id, machine) in &plan.machines {
        let body = &program.bodies[id.0 as usize];
        assert!(package.function(*id).is_async);
        assert!(body.unwind.is_some());
        for (block, kind) in &machine.suspensions {
            let mir::Terminator::Call {
                callee,
                unwind,
                span,
                ..
            } = &body.blocks[block.0 as usize].terminator
            else {
                panic!("a suspension must be a call")
            };
            assert!(unwind.is_some());
            assert!(span.end() > span.start());
            match kind {
                Suspension::Call(id) => {
                    assert!(matches!(callee, mir::Callee::Function(callee) if id == callee))
                }
                Suspension::Task => assert!(matches!(callee, mir::Callee::TaskWait)),
            }
        }
    }
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"leaf"));
    assert!(names.contains(&"parent"));
    assert!(!names.contains(&"plain"));
    assert!(names.iter().any(|name| name.contains("$go")));
}

#[test]
fn fallback_propagates_through_awaits_but_not_spawned_or_plain_calls() {
    let (package, _, plan) = plan(
        "package main
async func channelWait(ch channel<int>) int { let n, _ = ch.receive(); return n }
async func fallback(ch channel<int>) int { return await channelWait(ch) }
async func simple(ch channel<int>) int { let task = go channelWait(ch); return await task }
func helper(ch channel<int>) int { let n, _ = ch.receive(); return n }
async func blockingHelper(ch channel<int>) int { return helper(ch) }
func main() {}",
    );
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(!names.contains(&"channelWait"));
    assert!(!names.contains(&"fallback"));
    assert!(names.contains(&"simple"));
    assert!(names.contains(&"blockingHelper"));
}

#[test]
fn recursive_calls_are_planned_and_nonblocking_natives_do_not_force_fibers() {
    let (package, _, plan) = plan("package main
import \"zore/strings\"
async func recurse(n int) string { if n == 0 { return strings.Upper(\"done\") }; return await recurse(n - 1) }
func main() { let t = go recurse(5); println(t.wait()) }");
    let (&id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "recurse")
        .unwrap();
    assert!(
        machine
            .suspensions
            .iter()
            .any(|(_, kind)| *kind == Suspension::Call(id))
    );
}
