use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::project::{FileSystem, Manifest};
use crate::async_lowering::NativeWait;
use crate::source::SourceError;

pub(super) const C_FLAGS: &[&str] = &["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"];
pub(super) const VENDORED_C_FLAGS: &[&str] = &["-std=c11", "-O2", "-w"];
const RECIPE: &str = "zore-native-package-static-c11-v1";

pub(super) struct Adapter {
    pub id: &'static str,
    pub project: &'static str,
    pub fingerprint: &'static str,
    pub c_flags: &'static [&'static str],
    pub symbols: &'static [(&'static str, &'static str, Option<NativeWait>)],
    pub mbedtls: bool,
}

const ADLER32: Adapter = Adapter {
    mbedtls: false,
    id: "adler32-v1",
    project: "adler32",
    fingerprint: "66280c83962934f0b1937cfc1633458324381c156fc575100d8f145433a871ab",
    c_flags: &[],
    symbols: &[
        ("nativeCreate", "zore_native_adler32_native_create", None),
        ("nativeWrite", "zore_native_adler32_native_write", None),
        ("nativeSum", "zore_native_adler32_native_sum", None),
        ("nativeClose", "zore_native_adler32_native_close", None),
    ],
};

const SQLITE: Adapter = Adapter {
    mbedtls: false,
    id: "sqlite-v1",
    project: "sqlite",
    fingerprint: "39079c16c327f06c66f667782653832f0ce4ac5741453fa8540cc5fda6fa5711",
    c_flags: &[
        "-Wno-error=sign-compare",
        "-Wno-error=unused-variable",
        "-Wno-error=unused-function",
        "-DSQLITE_THREADSAFE=1",
        "-DSQLITE_OMIT_LOAD_EXTENSION",
        "-DSQLITE_DQS=0",
        "-DSQLITE_DEFAULT_MEMSTATUS=0",
        "-DSQLITE_MAX_LENGTH=16777216",
        "-DSQLITE_MAX_SQL_LENGTH=1048576",
        "-D_DEFAULT_SOURCE",
    ],
    symbols: &[
        (
            "nativeOpen",
            "zore_native_sqlite_native_open",
            Some(NativeWait::Helper),
        ),
        (
            "nativePrepare",
            "zore_native_sqlite_native_prepare",
            Some(NativeWait::Helper),
        ),
        (
            "nativeBusy",
            "zore_native_sqlite_native_busy",
            Some(NativeWait::Helper),
        ),
        (
            "nativeBegin",
            "zore_native_sqlite_native_begin",
            Some(NativeWait::Helper),
        ),
        (
            "nativeFinish",
            "zore_native_sqlite_native_finish",
            Some(NativeWait::Helper),
        ),
        (
            "nativeDropDatabase",
            "zore_native_sqlite_native_drop_database",
            None,
        ),
        (
            "nativeBind",
            "zore_native_sqlite_native_bind",
            Some(NativeWait::Helper),
        ),
        (
            "nativeStep",
            "zore_native_sqlite_native_step",
            Some(NativeWait::Helper),
        ),
        (
            "nativeReset",
            "zore_native_sqlite_native_reset",
            Some(NativeWait::Helper),
        ),
        (
            "nativeColumn",
            "zore_native_sqlite_native_column",
            Some(NativeWait::Helper),
        ),
        (
            "nativeFloat",
            "zore_native_sqlite_native_float",
            Some(NativeWait::Helper),
        ),
        (
            "nativeBytes",
            "zore_native_sqlite_native_bytes",
            Some(NativeWait::Helper),
        ),
        (
            "nativeFinalize",
            "zore_native_sqlite_native_finalize",
            Some(NativeWait::Helper),
        ),
    ],
};

const CRYPTO: Adapter = Adapter {
    mbedtls: true,
    id: "crypto-v1",
    project: "crypto",
    fingerprint: "a9ce277a13826cd9b135f7f6931453e61677dc8424e42a5e0172596d93d9a385",
    c_flags: &[],
    symbols: &[
        ("nativeRandomBytes", "zore_native_crypto_random_bytes", None),
        ("nativeSHA256", "zore_native_crypto_sha256", None),
        ("nativeHMACSHA256", "zore_native_crypto_hmac_sha256", None),
        ("nativeEqual", "zore_native_crypto_equal", None),
    ],
};

const TLS: Adapter = Adapter {
    mbedtls: true,
    id: "tls-mbedtls-v1",
    project: "tls",
    fingerprint: "3081dddda354e3055b565f187267c4013af2f77525aab9bd802265b9b5f0d8c6",
    c_flags: &[],
    symbols: &[
        ("nativeConfig", "zore_native_tls_native_config", None),
        ("nativeSession", "zore_native_tls_native_session", None),
        ("nativeHandshake", "zore_native_tls_native_handshake", None),
        ("nativeRead", "zore_native_tls_native_read", None),
        ("nativeWrite", "zore_native_tls_native_write", None),
        ("nativeFeed", "zore_native_tls_native_feed", None),
        ("nativeOutput", "zore_native_tls_native_output", None),
        ("nativeShutdown", "zore_native_tls_native_shutdown", None),
        ("nativeClose", "zore_native_tls_native_close", None),
        ("nativeStatus", "zore_native_tls_native_status", None),
        (
            "nativeSystemRoots",
            "zore_native_tls_native_system_roots",
            None,
        ),
    ],
};

pub(super) fn find(id: &str) -> Option<&'static Adapter> {
    [&ADLER32, &SQLITE, &CRYPTO, &TLS]
        .into_iter()
        .find(|adapter| adapter.id == id)
}

pub(super) struct Provider {
    pub adapter: &'static Adapter,
    pub bindings: String,
    pub source: String,
    pub header: String,
    pub bridge: String,
}

pub(super) type Providers = BTreeMap<String, Provider>;

pub(super) fn load(files: &dyn FileSystem, manifest: &Manifest) -> Result<Provider, SourceError> {
    let adapter = find(manifest.native.as_deref().expect("native project"))
        .expect("validated native adapter");
    let read = |name: &str| {
        let path = manifest.root.join("native").join(name);
        let canonical = files.canonical(&path);
        if !canonical.starts_with(files.canonical(&manifest.root)) {
            return Err(SourceError::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "native source must stay inside its declaring project",
                ),
            });
        }
        files.read(&canonical)
    };
    let provider = Provider {
        adapter,
        bindings: read("bindings.abi")?,
        source: read(&format!("{}.c", adapter.project))?,
        header: read(&format!("{}.h", adapter.project))?,
        bridge: read("bridge.rs")?,
    };
    if provider.fingerprint() != adapter.fingerprint {
        return Err(SourceError::Io {
            path: manifest.root.join("zore.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "native sources do not match reviewed adapter `{}`; restore its approved package sources; regenerating the lock cannot approve native code",
                    adapter.id
                ),
            ),
        });
    }
    Ok(provider)
}

impl Provider {
    fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        let mut part = |text: &str| {
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        };
        for text in [
            RECIPE,
            self.adapter.id,
            self.adapter.project,
            "native/bindings.abi",
            &self.bindings,
            &format!("native/{}.c", self.adapter.project),
            &self.source,
            &format!("native/{}.h", self.adapter.project),
            &self.header,
            "native/bridge.rs",
            &self.bridge,
        ] {
            part(text);
        }
        for flag in C_FLAGS.iter().chain(self.adapter.c_flags) {
            part(flag);
        }
        for (name, symbol, _) in self.adapter.symbols {
            part(name);
            part(symbol);
        }
        if self.adapter.mbedtls {
            part(super::mbedtls::FINGERPRINT);
            part(super::mbedtls::RECIPE);
            for flag in VENDORED_C_FLAGS {
                part(flag);
            }
        }
        format!("{:x}", hash.finalize())
    }
}
