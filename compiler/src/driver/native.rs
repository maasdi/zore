use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::project::{FileSystem, Manifest};
use crate::source::SourceError;

pub(super) const C_FLAGS: &[&str] = &["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"];
pub(super) const VENDORED_C_FLAGS: &[&str] = &["-std=c11", "-O2", "-w"];
const RECIPE: &str = "zore-native-package-static-c11-v1";

pub(super) struct Adapter {
    pub id: &'static str,
    pub project: &'static str,
    pub fingerprint: &'static str,
    pub symbols: &'static [(&'static str, &'static str)],
    pub mbedtls: bool,
}

const ADLER32: Adapter = Adapter {
    mbedtls: false,
    id: "adler32-v1",
    project: "adler32",
    fingerprint: "66280c83962934f0b1937cfc1633458324381c156fc575100d8f145433a871ab",
    symbols: &[
        ("nativeCreate", "zore_native_adler32_native_create"),
        ("nativeWrite", "zore_native_adler32_native_write"),
        ("nativeSum", "zore_native_adler32_native_sum"),
        ("nativeClose", "zore_native_adler32_native_close"),
    ],
};

const CRYPTO: Adapter = Adapter {
    mbedtls: true,
    id: "crypto-v1",
    project: "crypto",
    fingerprint: "9ada99205e00fb5b42c35d9259216f0ad95fad3f78e32cf099457b5557543ed2",
    symbols: &[
        ("nativeRandomBytes", "zore_native_crypto_random_bytes"),
        ("nativeSHA256", "zore_native_crypto_sha256"),
        ("nativeHMACSHA256", "zore_native_crypto_hmac_sha256"),
        ("nativeEqual", "zore_native_crypto_equal"),
    ],
};

const TLS: Adapter = Adapter {
    mbedtls: true,
    id: "tls-mbedtls-v1",
    project: "tls",
    fingerprint: "77f7d1052830e96a0cb41d6a21d1206aed0a4a60be15e42582ae217c382c5905",
    symbols: &[
        ("nativeConfig", "zore_native_tls_native_config"),
        ("nativeSession", "zore_native_tls_native_session"),
        ("nativeHandshake", "zore_native_tls_native_handshake"),
        ("nativeRead", "zore_native_tls_native_read"),
        ("nativeWrite", "zore_native_tls_native_write"),
        ("nativeFeed", "zore_native_tls_native_feed"),
        ("nativeOutput", "zore_native_tls_native_output"),
        ("nativeShutdown", "zore_native_tls_native_shutdown"),
        ("nativeClose", "zore_native_tls_native_close"),
        ("nativeStatus", "zore_native_tls_native_status"),
    ],
};

pub(super) fn find(id: &str) -> Option<&'static Adapter> {
    [&ADLER32, &CRYPTO, &TLS]
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
        for flag in C_FLAGS {
            part(flag);
        }
        for (name, symbol) in self.adapter.symbols {
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
