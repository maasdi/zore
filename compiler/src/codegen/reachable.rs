use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

struct Definition<'a> {
    symbol: &'a str,
    range: Range<usize>,
    private: bool,
}

fn symbols(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices('@').filter_map(|(start, _)| {
        let tail = &text[start + 1..];
        let length = if let Some(quoted) = tail.strip_prefix('"') {
            quoted.find('"')? + 2
        } else {
            tail.bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || b"_$.-".contains(byte))
                .count()
        };
        (length > 0).then_some(&text[start..start + 1 + length])
    })
}

fn definitions(ir: &str) -> Option<Vec<Definition<'_>>> {
    let mut definitions = Vec::new();
    let mut pending = None;
    let mut offset = 0;
    for line in ir.split_inclusive('\n') {
        if line.starts_with("define ") {
            if pending.is_some() || !line.trim_end().ends_with('{') {
                return None;
            }
            pending = Some((
                offset,
                symbols(line).next()?,
                line.starts_with("define internal ") || line.starts_with("define private "),
            ));
        } else if line.trim_end() == "}" {
            let (start, symbol, private) = pending.take()?;
            definitions.push(Definition {
                symbol,
                range: start..offset + line.len(),
                private,
            });
        }
        offset += line.len();
    }
    pending.is_none().then_some(definitions)
}

fn symbol_key(symbol: &str) -> Option<Vec<u8>> {
    let name = symbol.strip_prefix('@')?;
    let Some(quoted) = name.strip_prefix('"') else {
        return Some(name.as_bytes().to_vec());
    };
    let mut bytes = quoted.strip_suffix('"')?.bytes();
    let mut key = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            key.push((high * 16 + low) as u8);
        } else {
            key.push(byte);
        }
    }
    Some(key)
}

fn references(text: &str, indices: &HashMap<Vec<u8>, usize>) -> Option<Vec<usize>> {
    let mut references = Vec::new();
    for symbol in symbols(text) {
        let key = symbol_key(symbol)?;
        if let Some(&index) = indices.get(&key) {
            references.push(index);
        }
    }
    Some(references)
}

pub(crate) fn retain_reachable_functions(ir: &str) -> Cow<'_, str> {
    reachable_functions(ir).unwrap_or(Cow::Borrowed(ir))
}

fn reachable_functions(ir: &str) -> Option<Cow<'_, str>> {
    let definitions = definitions(ir)?;
    let mut indices = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if indices
            .insert(symbol_key(definition.symbol)?, index)
            .is_some()
        {
            return None;
        }
    }
    let mut roots = Vec::new();
    let mut position = 0;
    for (index, definition) in definitions.iter().enumerate() {
        roots.extend(references(&ir[position..definition.range.start], &indices)?);
        if !definition.private {
            roots.push(index);
        }
        position = definition.range.end;
    }
    roots.extend(references(&ir[position..], &indices)?);
    let mut live = HashSet::new();
    while let Some(index) = roots.pop() {
        if live.insert(index) {
            roots.extend(references(&ir[definitions[index].range.clone()], &indices)?);
        }
    }
    if live.len() == definitions.len() {
        return Some(Cow::Borrowed(ir));
    }
    let mut output = String::with_capacity(ir.len());
    position = 0;
    for (index, definition) in definitions.iter().enumerate() {
        output.push_str(&ir[position..definition.range.start]);
        if live.contains(&index) {
            output.push_str(&ir[definition.range.clone()]);
        }
        position = definition.range.end;
    }
    output.push_str(&ir[position..]);
    Some(Cow::Owned(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prunes_dead_recursion_from_real_ir_without_changing_full_emission() {
        let mut sources = crate::source::SourceMap::new();
        let file = sources
            .add(
                "reachability.ore",
                include_str!("../../../tests/codegen/fixtures/reachability.ore").into(),
            )
            .unwrap();
        let ir = crate::driver::build::emit_llvm(sources.file(file).unwrap()).unwrap();
        assert!(ir.contains("define internal i64 @\"main.deadA\""));
        let pruned = retain_reachable_functions(&ir);
        assert!(!pruned.contains("@\"main.deadA\""));
        assert!(!pruned.contains("@\"main.deadB\""));
        assert!(pruned.contains("define void @zore_entry()"));
        assert!(pruned.contains("@\"main.seed\""));
        assert!(pruned.contains("@\"main.job"));
        assert_eq!(pruned, retain_reachable_functions(&ir));
    }

    #[test]
    fn follows_global_tables_escaped_symbols_and_address_taken_callbacks() {
        let ir = "@table = private constant ptr @\"callback\\22@\"\n\
!0 = !{ptr @metadata}\n\
define private void @metadata() {\n  ret void\n}\n\
define private void @\"callback\\22@\"() {\n  call void @cleanup()\n  ret void\n}\n\
define internal void @cleanup() {\n  ret void\n}\n\
define private void @dead() {\n  call void @other()\n  ret void\n}\n\
define internal void @other() {\n  call void @dead()\n  ret void\n}\n\
define void @zore_entry() {\n  ret void\n}\n";
        let pruned = retain_reachable_functions(ir);
        assert!(pruned.contains("@table ="));
        assert!(pruned.contains("define private void @metadata()"));
        assert!(pruned.contains("define private void @\"callback\\22@\""));
        assert!(pruned.contains("define internal void @cleanup()"));
        assert!(pruned.contains("define void @zore_entry()"));
        assert!(!pruned.contains("@dead"));
        assert!(!pruned.contains("@other"));
        assert_eq!(pruned, retain_reachable_functions(ir));
        assert_eq!(pruned, retain_reachable_functions(&pruned));
    }

    #[test]
    fn string_contents_cannot_hide_a_later_symbol_reference() {
        let ir = "@text = private constant [1 x i8] c\"@\"\n\
@alias = private alias void (), ptr @\"kept\"\n\
define private void @\"kept\"() {\n  ret void\n}\n\
define private void @dead() {\n  ret void\n}\n\
define void @external() {\n  ret void\n}\n";
        let pruned = retain_reachable_functions(ir);
        assert!(pruned.contains("define private void @\"kept\""));
        assert!(pruned.contains("define void @external()"));
        assert!(!pruned.contains("@dead"));
    }

    #[test]
    fn follows_equivalent_quoted_unquoted_and_hex_escaped_names() {
        let ir = "@table = private constant ptr @kept\n\
define private void @\"k\\65pt\"() {\n  call void @\"\\6Cive\"()\n  ret void\n}\n\
define private void @live() {\n  call void @\"\\6cive\"()\n  ret void\n}\n\
define private void @dead() {\n  ret void\n}\n";
        let pruned = retain_reachable_functions(ir);
        assert!(pruned.contains("define private void @\"k\\65pt\""));
        assert!(pruned.contains("define private void @live()"));
        assert!(!pruned.contains("@dead"));
    }

    #[test]
    fn unfamiliar_or_incomplete_definitions_keep_the_original_module() {
        for ir in [
            "define private void @missing() {\n  ret void\n",
            "define private void @inline() { ret void }\n",
            "define private void @same() {\n}\ndefine private void @same() {\n}\n",
            "define private void @\"bad\\GG\"() {\n}\n",
        ] {
            assert!(matches!(retain_reachable_functions(ir), Cow::Borrowed(_)));
        }
    }
}
