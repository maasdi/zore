//! The standard packages the compiler bundles, as `(file name, source)` pairs.

const STRINGS: &[(&str, &str)] = &[(
    "strings.ore",
    include_str!("../../../std/strings/strings.ore"),
)];
const TIME: &[(&str, &str)] = &[("time.ore", include_str!("../../../std/time/time.ore"))];
const IO: &[(&str, &str)] = &[("io.ore", include_str!("../../../std/io/io.ore"))];
const OS: &[(&str, &str)] = &[("os.ore", include_str!("../../../std/os/os.ore"))];
const NET: &[(&str, &str)] = &[("net.ore", include_str!("../../../std/net/net.ore"))];
const CANCEL: &[(&str, &str)] = &[("cancel.ore", include_str!("../../../std/cancel/cancel.ore"))];
const STRCONV: &[(&str, &str)] = &[(
    "strconv.ore",
    include_str!("../../../std/strconv/strconv.ore"),
)];

pub fn package(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match name {
        "strings" => Some(STRINGS),
        "strconv" => Some(STRCONV),
        "time" => Some(TIME),
        "io" => Some(IO),
        "os" => Some(OS),
        "net" => Some(NET),
        "cancel" => Some(CANCEL),
        _ => None,
    }
}
