//! The standard packages the compiler bundles, as `(file name, source)` pairs.

macro_rules! bundled {
    ($($path:literal => [$($file:literal),+ $(,)?]),+ $(,)?) => {
        const PACKAGES: &[(&str, &[(&str, &str)])] = &[
            $(($path, &[$(($file, include_str!(concat!("../../../std/", $path, "/", $file)))),+])),+
        ];
    };
}

bundled! {
    "archive" => ["archive.ore", "extract.ore", "tar.ore", "zip.ore"],
    "bufio" => ["bufio.ore"],
    "bytes" => ["bytes.ore", "buffer.ore"],
    "compress/flate" => ["deflate.ore", "reader.ore", "writer.ore"],
    "compress/gzip" => ["deflate.ore", "reader.ore", "writer.ore"],
    "context" => ["context.ore"],
    "crypto/hmac" => ["hmac.ore"],
    "crypto/rand" => ["rand.ore"],
    "crypto/sha1" => ["sha1.ore"],
    "crypto/sha256" => ["sha256.ore"],
    "crypto/sha512" => ["sha512.ore"],
    "crypto/subtle" => ["subtle.ore"],
    "errors" => ["errors.ore"],
    "encoding/base64" => ["base64.ore"],
    "encoding/binary" => ["binary.ore"],
    "encoding/csv" => ["csv.ore"],
    "encoding/hex" => ["hex.ore"],
    "encoding/html" => ["html.ore"],
    "encoding/json" => ["encoder.ore", "decoder.ore", "value.ore", "codec.ore"],
    "flag" => ["flag.ore"],
    "fmt" => ["fmt.ore"],
    "hash/crc32" => ["crc32.ore"],
    "io" => ["io.ore"],
    "log" => ["log.ore"],
    "log/rolling" => ["rolling.ore"],
    "maps" => ["maps.ore"],
    "mime" => ["mime.ore"],
    "math" => ["math.ore"],
    "math/rand" => ["rand.ore"],
    "net" => ["net.ore", "ip.ore", "dns.ore", "udp.ore"],
    "net/url" => ["url.ore", "absolute.ore", "reference.ore"],
    "net/http" => ["header.ore", "cookie.ore", "form.ore", "file.ore", "multipart.ore", "router.ore", "exchange.ore", "server.ore", "convenience.ore", "client.ore"],
    "os" => ["os.ore", "file.ore", "temp.ore", "tree.ore"],
    "os/exec" => ["exec.ore"],
    "os/signal" => ["signal.ore"],
    "path" => ["path.ore"],
    "path/filepath" => ["filepath.ore", "walk.ore"],
    "regexp" => ["regexp.ore"],
    "slices" => ["slices.ore"],
    "sort" => ["sort.ore"],
    "strconv" => ["strconv.ore"],
    "strings" => ["strings.ore", "builder.ore"],
    "sync" => ["sync.ore"],
    "testing" => ["testing.ore"],
    "time" => ["time.ore"],
    "unicode" => ["unicode.ore"],
    "unicode/utf8" => ["utf8.ore"],
}

pub fn package(path: &str) -> Option<&'static [(&'static str, &'static str)]> {
    if cfg!(windows) && path == "path/filepath" {
        return Some(&[
            (
                "windows.ore",
                include_str!("../../../std/path/filepath/windows.ore"),
            ),
            (
                "walk.ore",
                include_str!("../../../std/path/filepath/walk.ore"),
            ),
        ]);
    }
    PACKAGES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, files)| *files)
}
