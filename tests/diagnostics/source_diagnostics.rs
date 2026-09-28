use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use zore::diagnostic::{Diagnostic, Severity};
use zore::source::{LineColumn, SourceError, SourceMap};

static NEXT_PATH: AtomicUsize = AtomicUsize::new(0);

fn temporary_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "zore-m1-test-{}-{}.ore",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn empty_input_and_eof_are_valid_locations() {
    let mut sources = SourceMap::new();
    let id = sources.add("empty.ore", String::new()).unwrap();
    let file = sources.file(id).unwrap();
    assert!(file.is_empty());
    assert_eq!(file.line_count(), 1);
    assert_eq!(file.line(0), Some(""));
    assert_eq!(file.location(0), Some(LineColumn { line: 1, column: 1 }));
    let eof = sources.span(id, 0, 0).unwrap();
    assert!(eof.is_empty());
    assert_eq!(sources.slice(eof), Some(""));
    assert!(sources.span(id, 0, 1).is_none());

    let rendered = Diagnostic::new(Severity::Error, "unexpected EOF", eof)
        .render(&sources)
        .unwrap();
    assert!(rendered.contains("empty.ore:1:1"), "{rendered}");
    assert!(rendered.contains("1 | \n  | ^"), "{rendered}");
}

#[test]
fn utf8_spans_are_bytes_but_columns_are_unicode_scalars() {
    let mut sources = SourceMap::new();
    let id = sources.add("unicode.ore", "aé🙂z\n".into()).unwrap();
    let file = sources.file(id).unwrap();
    assert_eq!(file.len(), 9);
    assert_eq!(file.line_count(), 2);
    assert_eq!(file.location(3), Some(LineColumn { line: 1, column: 3 }));
    assert_eq!(file.location(9), Some(LineColumn { line: 2, column: 1 }));
    assert_eq!(file.line(1), Some(""));
    assert!(file.location(2).is_none());
    assert!(sources.span(id, 1, 2).is_none());
    assert!(sources.span(id, 3, 5).is_none());

    let emoji = sources.span(id, 3, 7).unwrap();
    assert_eq!(sources.slice(emoji), Some("🙂"));
    let rendered = Diagnostic::new(Severity::Error, "unexpected symbol", emoji)
        .primary_message("here")
        .render(&sources)
        .unwrap();
    assert!(rendered.contains("unicode.ore:1:3"), "{rendered}");
    assert!(rendered.contains("aé🙂z"), "{rendered}");
    assert!(rendered.contains("here"), "{rendered}");
}

#[test]
fn lf_crlf_and_lone_cr_have_distinct_line_behavior() {
    let mut sources = SourceMap::new();
    let id = sources
        .add("lines.ore", "one\r\ntwo\nthree\rfour".into())
        .unwrap();
    let file = sources.file(id).unwrap();
    assert_eq!(file.line_count(), 3);
    assert_eq!(file.line(0), Some("one"));
    assert_eq!(file.line(1), Some("two"));
    assert_eq!(file.line(2), Some("three\rfour"));
    assert_eq!(file.line(3), None);
    assert_eq!(file.location(5), Some(LineColumn { line: 2, column: 1 }));
    assert_eq!(
        file.location(file.len()),
        Some(LineColumn {
            line: 3,
            column: 11
        })
    );

    let cr = sources.span(id, 14, 15).unwrap();
    let rendered = Diagnostic::new(Severity::Warning, "CR is whitespace", cr)
        .render(&sources)
        .unwrap();
    assert!(rendered.contains("warning: CR is whitespace"));
    assert!(rendered.contains("three\\rfour"), "{rendered}");
}

#[test]
fn files_and_managers_keep_spans_distinct() {
    let mut sources = SourceMap::new();
    let first = sources.add("same.ore", "first".into()).unwrap();
    let second = sources.add("same.ore", "second".into()).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        sources.slice(sources.span(first, 0, 5).unwrap()),
        Some("first")
    );
    assert_eq!(
        sources.slice(sources.span(second, 0, 6).unwrap()),
        Some("second")
    );
    assert!(sources.span(second, 0, 7).is_none());

    let mut other = SourceMap::new();
    let other_id = other.add("same.ore", "different".into()).unwrap();
    assert_ne!(first, other_id);
    assert!(other.file(first).is_none());
    assert!(other.span(first, 0, 1).is_none());
    assert!(other.slice(sources.span(first, 0, 1).unwrap()).is_none());
    assert!(
        Diagnostic::new(
            Severity::Error,
            "foreign span",
            sources.span(first, 0, 1).unwrap()
        )
        .render(&other)
        .is_err()
    );
}

#[test]
fn diagnostics_retain_primary_and_related_locations_in_different_files() {
    let mut sources = SourceMap::new();
    let origin = sources
        .add("definition.ore", "let name = 1\n".into())
        .unwrap();
    let use_site = sources.add("use.ore", "\tname\n".into()).unwrap();
    let first = sources.span(origin, 4, 8).unwrap();
    let second = sources.span(use_site, 1, 5).unwrap();
    let rendered = Diagnostic::new(Severity::Error, "conflicting name", second)
        .primary_message("used here")
        .related(first, "defined here")
        .note("rename one binding")
        .render(&sources)
        .unwrap();
    assert!(rendered.contains("--> use.ore:1:2"), "{rendered}");
    assert!(rendered.contains("::: definition.ore:1:5"), "{rendered}");
    assert!(rendered.contains("    name"), "{rendered}");
    assert!(rendered.contains("used here"), "{rendered}");
    assert!(rendered.contains("defined here"), "{rendered}");
    assert!(rendered.contains("note: rename one binding"), "{rendered}");
}

#[test]
fn multiline_span_points_to_its_start_and_reports_its_extent() {
    let mut sources = SourceMap::new();
    let id = sources.add("multi.ore", "first\nsecond\n".into()).unwrap();
    let span = sources.span(id, 2, 10).unwrap();
    let rendered = Diagnostic::new(Severity::Error, "spans lines", span)
        .render(&sources)
        .unwrap();
    assert!(rendered.contains("multi.ore:1:3"), "{rendered}");
    assert!(
        rendered.contains("span continues through line 2"),
        "{rendered}"
    );
}

#[test]
fn loading_rejects_invalid_utf8_without_registering_a_file() {
    let path = temporary_path();
    fs::write(&path, b"valid\xffinvalid").unwrap();
    let mut sources = SourceMap::new();
    let result = sources.load(&path);
    fs::remove_file(&path).unwrap();
    match result {
        Err(SourceError::InvalidUtf8 {
            path: actual,
            valid_up_to,
        }) => {
            assert_eq!(actual, path);
            assert_eq!(valid_up_to, 5);
        }
        other => panic!("expected invalid UTF-8 error, got {other:?}"),
    }
    let id = sources.add("next.ore", "ok".into()).unwrap();
    assert_eq!(sources.file(id).unwrap().text(), "ok");
}

#[test]
fn loading_existing_file_preserves_its_path_and_io_errors_report_path() {
    let path = temporary_path();
    fs::write(&path, "é\r\nlast").unwrap();
    let mut sources = SourceMap::new();
    let id = sources.load(&path).unwrap();
    fs::remove_file(&path).unwrap();
    let file = sources.file(id).unwrap();
    assert_eq!(file.path(), path);
    assert_eq!(file.line(0), Some("é"));
    assert_eq!(
        file.location(file.len()),
        Some(LineColumn { line: 2, column: 5 })
    );

    match sources.load(&path) {
        Err(SourceError::Io { path: actual, .. }) => assert_eq!(actual, path),
        other => panic!("expected IO error, got {other:?}"),
    }
}
