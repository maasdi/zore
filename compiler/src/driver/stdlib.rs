//! The standard packages the compiler bundles, as `(file name, source)` pairs.

macro_rules! bundled {
    ($($path:literal => [$($file:literal),+ $(,)?]),+ $(,)?) => {
        const PACKAGES: &[(&str, &[(&str, &str)])] = &[
            $(($path, &[$(($file, include_str!(concat!("../../../std/", $path, "/", $file)))),+])),+
        ];
    };
}

bundled! {
    "bufio" => ["bufio.ore"],
    "bytes" => ["bytes.ore", "buffer.ore"],
    "context" => ["context.ore"],
    "errors" => ["errors.ore"],
    "io" => ["io.ore"],
    "maps" => ["maps.ore"],
    "math" => ["math.ore"],
    "net" => ["net.ore"],
    "os" => ["os.ore", "file.ore"],
    "os/exec" => ["exec.ore"],
    "path" => ["path.ore"],
    "path/filepath" => ["filepath.ore"],
    "slices" => ["slices.ore"],
    "sort" => ["sort.ore"],
    "strconv" => ["strconv.ore"],
    "strings" => ["strings.ore", "builder.ore"],
    "sync" => ["sync.ore"],
    "time" => ["time.ore"],
    "unicode" => ["unicode.ore"],
    "unicode/utf8" => ["utf8.ore"],
}

pub fn package(path: &str) -> Option<&'static [(&'static str, &'static str)]> {
    PACKAGES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, files)| *files)
}
