use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::blocking;
use super::regexp_parse::MAX_PATTERN;
use super::regexp_vm::{Budget, MAX_INPUT, MAX_MATCHES, Program};
use super::strconv::ValueError;
use super::sys::{StringError, WordArray, WordArrayError};

struct Handles {
    next: i64,
    values: HashMap<i64, Arc<Program>>,
}

static HANDLES: Mutex<Option<Handles>> = Mutex::new(None);

fn retain(program: Arc<Program>) -> Result<i64, String> {
    let mut table = HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let table = table.get_or_insert_with(|| Handles {
        next: 1,
        values: HashMap::new(),
    });
    let id = table.next;
    table.next = id.checked_add(1).ok_or("handle limit exceeded")?;
    table.values.insert(id, program);
    Ok(id)
}

fn program(id: i64) -> Result<Arc<Program>, String> {
    HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|table| table.values.get(&id))
        .cloned()
        .ok_or_else(|| "closed pattern".into())
}

pub(super) fn live_handles() -> usize {
    HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map_or(0, |table| table.values.len())
}

fn text(bytes: &[u8], max: usize, kind: &str) -> Result<String, String> {
    if bytes.len() > max {
        return Err(format!("{kind} byte limit exceeded"));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8".into())
}

/// # Safety
/// Out must be writable and pattern must address its declared bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_regexp_compile(
    out: *mut ValueError,
    pattern: *const u8,
    len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let pattern = text(
        unsafe { super::bytes(pattern, len) },
        MAX_PATTERN,
        "pattern",
    );
    let result = pattern
        .and_then(|pattern| blocking::run(move || Program::compile(&pattern)))
        .and_then(|program| retain(Arc::new(program)));
    let result = match result {
        Ok(id) => ValueError::ok(id),
        Err(error) => ValueError::failed_text(&format!("regexp.Compile: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_regexp_group_count(id: i64) -> i64 {
    program(id).map_or(0, |program| program.captures as i64)
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_regexp_share(id: i64) -> i64 {
    program(id).and_then(retain).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_regexp_release(id: i64) {
    if let Some(table) = HANDLES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        table.values.remove(&id);
    }
}

/// # Safety
/// Out must be writable and both strings must address their declared bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_regexp_search(
    out: *mut WordArrayError,
    id: i64,
    input: *const u8,
    input_len: i64,
    limit: i64,
    operation: i64,
) {
    let operation = match operation {
        0 => "regexp.Matches",
        1 => "regexp.Find",
        _ => "regexp.FindAll",
    };
    let result = (|| {
        let program = program(id)?;
        if limit < 1 {
            return Err("invalid match limit".into());
        }
        let limit = limit.min(MAX_MATCHES as i64) as usize;
        // SAFETY: guaranteed by the caller.
        let input = text(
            unsafe { super::bytes(input, input_len) },
            MAX_INPUT,
            "input",
        )?;
        blocking::run(move || {
            let found = program.find_all(&input, limit, &mut Budget::new(input.len()))?;
            Ok::<_, String>(
                found
                    .iter()
                    .flat_map(|captures| program.offsets(captures))
                    .collect::<Vec<_>>(),
            )
        })
    })();
    let result = match result {
        Ok(values) => WordArrayError::ok(WordArray::ints(&values)),
        Err(error) => WordArrayError::failed(&format!("{operation}: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// Out must be writable and both strings must address their declared bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_regexp_replace(
    out: *mut StringError,
    id: i64,
    input: *const u8,
    input_len: i64,
    replacement: *const u8,
    replacement_len: i64,
    literal: bool,
) {
    let operation = if literal {
        "regexp.ReplaceAllLiteral"
    } else {
        "regexp.ReplaceAll"
    };
    let result = (|| {
        let program = program(id)?;
        // SAFETY: guaranteed by the caller.
        let input = text(
            unsafe { super::bytes(input, input_len) },
            MAX_INPUT,
            "input",
        )?;
        // SAFETY: guaranteed by the caller.
        let replacement = text(
            unsafe { super::bytes(replacement, replacement_len) },
            MAX_INPUT,
            "replacement",
        )?;
        blocking::run(move || program.replace(&input, &replacement, literal))
    })()
    .map_err(|error| format!("{operation}: {error}"));
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringError::from_text(result)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_handles_are_independent_and_inflight_calls_keep_the_program_alive() {
        let compiled = Arc::new(Program::compile("(ab)+").unwrap());
        let weak = Arc::downgrade(&compiled);
        let first = retain(compiled).unwrap();
        let second = retain(program(first).unwrap()).unwrap();
        let inflight = program(first).unwrap();
        zore_native_regexp_release(first);
        assert!(program(first).is_err());
        assert!(program(second).is_ok());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let inflight = Arc::clone(&inflight);
                scope.spawn(move || {
                    for _ in 0..100 {
                        assert_eq!(
                            inflight
                                .find_all("abab", 1, &mut Budget::new(0))
                                .unwrap()
                                .len(),
                            1
                        );
                    }
                });
            }
            zore_native_regexp_release(second);
            assert!(program(second).is_err());
            assert!(weak.upgrade().is_some());
        });
        drop(inflight);
        assert!(weak.upgrade().is_none());
        let third = retain(Arc::new(Program::compile("").unwrap())).unwrap();
        assert!(third > second);
        zore_native_regexp_release(third);
        zore_native_regexp_release(0);
    }

    #[test]
    fn text_limits_and_utf8_are_checked_without_lossy_rewriting() {
        assert!(
            text(&[255], MAX_INPUT, "input")
                .unwrap_err()
                .contains("UTF-8")
        );
        assert!(text(&vec![b'a'; MAX_INPUT], MAX_INPUT, "input").is_ok());
        assert!(
            text(&vec![b'a'; MAX_INPUT + 1], MAX_INPUT, "input")
                .unwrap_err()
                .contains("input byte limit")
        );
    }
}
