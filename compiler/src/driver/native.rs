use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::project::{FileSystem, Manifest};
use crate::source::SourceError;

pub(super) const C_FLAGS: &[&str] = &["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"];
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
    fingerprint: "c5030f47bb846f722176eb0a9eb062869ac7084d3e802a484b69f51fa4acaaeb",
    symbols: &[
        ("nativeRandomBytes", "zore_native_crypto_random_bytes"),
        ("nativeSHA256", "zore_native_crypto_sha256"),
        ("nativeHMACSHA256", "zore_native_crypto_hmac_sha256"),
        ("nativeEqual", "zore_native_crypto_equal"),
    ],
};

pub(super) fn find(id: &str) -> Option<&'static Adapter> {
    [&ADLER32, &CRYPTO]
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
        }
        format!("{:x}", hash.finalize())
    }
}
