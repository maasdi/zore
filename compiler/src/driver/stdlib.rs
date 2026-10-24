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
    "encoding/json" => ["encoder.ore", "decoder.ore", "value.ore", "codec.ore"],
    "fmt" => ["fmt.ore"],
    "io" => ["io.ore"],
    "maps" => ["maps.ore"],
    "math" => ["math.ore"],
    "net" => ["net.ore"],
    "net/url" => ["url.ore", "absolute.ore"],
    "net/http" => ["header.ore", "router.ore", "exchange.ore", "server.ore", "convenience.ore", "client.ore"],
    "os" => ["os.ore", "file.ore"],
    "os/exec" => ["exec.ore"],
    "os/signal" => ["signal.ore"],
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
    if cfg!(windows) && path == "path/filepath" {
        return Some(&[(
            "windows.ore",
            include_str!("../../../std/path/filepath/windows.ore"),
        )]);
    }
    PACKAGES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, files)| *files)
}
