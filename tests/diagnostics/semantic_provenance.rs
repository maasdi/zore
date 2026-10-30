use std::fs;
use std::process::Command;

use zore::build::TempDir;
use zore::check::{Checked, check_project};
use zore::diagnostic::{Diagnostic, Severity};
use zore::driver::project::{Disk, LoadError, load_project};
use zore::source::{FileId, LineColumn, SourceMap, Span};

struct Case {
    directory: TempDir,
    sources: SourceMap,
    checked: Checked,
    main: FileId,
}

impl Case {
    fn new(main: &str, helpers: &str) -> Self {
        let directory = TempDir::new().unwrap();
        let main = main.replace('\n', "\r\n");
        let helpers = helpers.replace('\n', "\r\n");
        fs::write(directory.path().join("main.ore"), &main).unwrap();
        fs::write(directory.path().join("helpers.ore"), &helpers).unwrap();
        let mut sources = SourceMap::new();
        let project = match load_project(&mut sources, &Disk, &directory.path().join("main.ore")) {
            Ok(project) => project,
            Err(LoadError::Source(error)) => panic!("fixture did not load: {error}"),
            Err(LoadError::Diagnostics(diagnostics)) => {
                panic!("fixture did not parse: {diagnostics:?}")
            }
        };
        let units = project.units();
        let files = &units.last().unwrap().files;
        assert_eq!(files.len(), 2);
        let file_id = |name| {
            files
                .iter()
                .map(|unit| unit.file.package.as_ref().unwrap().span.file())
                .find(|&id| sources.file(id).unwrap().path().file_name().unwrap() == name)
                .unwrap()
        };
        let main_id = file_id("main.ore");
        let helpers_id = file_id("helpers.ore");
        assert_ne!(main_id, helpers_id);
        for (id, name, text) in [
            (main_id, "main.ore", &main),
            (helpers_id, "helpers.ore", &helpers),
        ] {
            let file = sources.file(id).unwrap();
            assert_eq!(file.path(), directory.path().join(name));
            assert_eq!(file.text(), text);
        }
        let checked = check_project(&project, &sources);
        assert!(checked.package.is_none(), "invalid fixture produced HIR");
        Self {
            directory,
            sources,
            checked,
            main: main_id,
        }
    }

    fn diagnostic(&self, message: &str) -> &Diagnostic {
        let matching: Vec<_> = self
            .checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message().contains(message))
            .collect();
        assert_eq!(matching.len(), 1, "{:#?}", self.checked.diagnostics);
        assert_eq!(matching[0].severity(), Severity::Error);
        matching[0]
    }

    fn assert_span(&self, span: Span, context: &str, token: &str, line: usize, column: usize) {
        assert_eq!(span.file(), self.main);
        let file = self.sources.file(self.main).unwrap();
        let start = file.text().find(context).unwrap() + context.find(token).unwrap();
        assert_eq!(span.start() as usize, start);
        assert_eq!(span.end() as usize, start + token.len());
        assert_eq!(self.sources.slice(span), Some(token));
        assert_eq!(
            file.location(span.start()),
            Some(LineColumn { line, column })
        );
    }

    fn related(&self, diagnostic: &Diagnostic, message: &str) -> Span {
        let matching: Vec<_> = diagnostic
            .related_labels()
            .filter(|(_, label)| label.contains(message))
            .collect();
        assert_eq!(matching.len(), 1, "{diagnostic:?}");
        matching[0].0
    }
}

#[test]
fn disk_project_move_diagnostic_preserves_origin_and_unicode_columns_in_cli() {
    let case = Case::new(
        "package main\nfunc main() {\n    let guard = Guard{id: 1}\n    consume(guard)\n    println(\"é\"); println(guard.id)\n}\n",
        "package main\ntype Guard struct { id int }\nfunc (g mut Guard) drop() {}\nfunc consume(g own Guard) {}\n",
    );
    assert_eq!(case.checked.diagnostics.len(), 1);
    let diagnostic = case.diagnostic("use of moved value `guard.id`");
    case.assert_span(diagnostic.span(), "println(guard.id)", "guard.id", 5, 27);
    case.assert_span(
        case.related(diagnostic, "value moved here"),
        "consume(guard)",
        "consume(guard)",
        4,
        5,
    );
    let rendered = diagnostic.render(&case.sources).unwrap();
    assert!(rendered.contains("main.ore:5:27"), "{rendered}");
    assert!(rendered.contains("main.ore:4:5"), "{rendered}");
    assert!(rendered.contains("value moved here"), "{rendered}");
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .arg("check")
        .arg(case.directory.path().join("main.ore"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    for fragment in [
        "use of moved value `guard.id`",
        "main.ore:5:27",
        "main.ore:4:5",
        "println(\"é\"); println(guard.id)",
        "value moved here",
        "check failed with 1 error",
    ] {
        assert!(stderr.contains(fragment), "missing {fragment:?}: {stderr}");
    }
}

#[test]
fn disk_project_mutable_loan_diagnostic_preserves_origin_and_whole_owner_reason() {
    let case = Case::new(
        "package main\nfunc main() {\n    var data = [int; 4]{1, 2, 3, 4}\n    var low mut []int = data[0:2]\n    println(\"é\"); var high mut []int = data[2:4]\n    edit(low)\n    edit(high)\n}\n",
        "package main\nfunc edit(values mut []int) { values[0] = 0 }\n",
    );
    assert_eq!(case.checked.diagnostics.len(), 1);
    let diagnostic = case.diagnostic("cannot borrow `data` as mutable");
    case.assert_span(diagnostic.span(), "data[2:4]", "data[2:4]", 5, 40);
    case.assert_span(
        case.related(diagnostic, "mutable borrow of `data` created here"),
        "data[0:2]",
        "data[0:2]",
        4,
        25,
    );
    let rendered = diagnostic.render(&case.sources).unwrap();
    for fragment in [
        "main.ore:5:40",
        "main.ore:4:25",
        "mutable borrow of `data` created here",
        "the view `low` is used later",
        "whole originating place",
    ] {
        assert!(rendered.contains(fragment), "{rendered}");
    }
}

#[test]
fn disk_project_view_escape_diagnostics_preserve_owner_and_borrow_lifetime() {
    let case = Case::new(
        "package main\nfunc escape() []int {\n    let data = [int; 2]{1, 2}\n    println(\"é\"); return data[:]\n}\nfunc main() { inspect(escape()) }\n",
        "package main\nfunc inspect(values []int) { println(values[0]) }\n",
    );
    let owner = case
        .checked
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .message()
                .contains("cannot return a view of local `data`")
                && diagnostic.related_labels().next().is_some()
        })
        .unwrap();
    case.assert_span(owner.span(), "let data", "data", 3, 9);
    case.assert_span(
        case.related(owner, "`data` borrowed here"),
        "data[:]",
        "data[:]",
        4,
        26,
    );
    let rendered = owner.render(&case.sources).unwrap();
    for fragment in [
        "cannot return a view of local `data`",
        "main.ore:3:9",
        "main.ore:4:26",
        "`data` borrowed here",
        "keeps this borrow live",
    ] {
        assert!(rendered.contains(fragment), "{rendered}");
    }
    let returned = case
        .checked
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .notes()
                .iter()
                .any(|note| note.contains("storage borrowed from a parameter"))
        })
        .unwrap();
    case.assert_span(returned.span(), "data[:]", "data[:]", 4, 26);
    assert!(
        returned
            .message()
            .contains("cannot return a view of local `data`")
    );
}

#[test]
fn disk_project_spawned_view_diagnostic_explains_independent_task_storage() {
    let case = Case::new(
        "package main\nfunc main() {\n    let data = [int; 2]{1, 2}\n    let view = data[:]\n    println(\"é\"); let task = go inspect(view)\n    task.wait()\n}\n",
        "package main\nfunc inspect(values []int) { println(values[0]) }\n",
    );
    assert_eq!(case.checked.diagnostics.len(), 1);
    let diagnostic = case.diagnostic("a spawned call cannot take a view");
    case.assert_span(diagnostic.span(), "inspect(view)", "view", 5, 41);
    let rendered = diagnostic.render(&case.sources).unwrap();
    assert!(rendered.contains("main.ore:5:41"), "{rendered}");
    assert!(
        rendered.contains("may borrow storage the task does not own"),
        "{rendered}"
    );
}
