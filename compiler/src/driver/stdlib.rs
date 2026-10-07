//! The standard packages the compiler bundles, as `(file name, source)` pairs.

const STRINGS: &[(&str, &str)] = &[("strings.ore", include_str!("stdlib/strings.ore"))];
const STRCONV: &[(&str, &str)] = &[("strconv.ore", include_str!("stdlib/strconv.ore"))];

pub fn package(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match name {
        "strings" => Some(STRINGS),
        "strconv" => Some(STRCONV),
        _ => None,
    }
}
