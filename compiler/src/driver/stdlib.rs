//! The standard packages the compiler bundles, as `(file name, source)` pairs.

pub fn package(_name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    None
}
