use sha2::{Digest, Sha256};

pub(super) const C_FLAGS: &[&str] = &["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"];

pub(super) struct Adapter {
    pub id: &'static str,
    pub project: &'static str,
    pub configuration: &'static str,
    pub bindings: &'static str,
    pub source: &'static str,
    pub header: &'static str,
    pub bridge: &'static str,
    pub symbols: &'static [(&'static str, &'static str)],
}

const ADLER32: Adapter = Adapter {
    id: "adler32-v1",
    project: "adler32",
    configuration: "zore_adapter_adler32_v1",
    bindings: include_str!("../../../runtime/native/adler32.ore"),
    source: include_str!("../../../runtime/native/adler32.c"),
    header: include_str!("../../../runtime/native/adler32.h"),
    bridge: include_str!("../../../runtime/src/adler32.rs"),
    symbols: &[
        ("nativeCreate", "zore_native_adler32_native_create"),
        ("nativeWrite", "zore_native_adler32_native_write"),
        ("nativeSum", "zore_native_adler32_native_sum"),
        ("nativeClose", "zore_native_adler32_native_close"),
    ],
};

pub(super) fn find(id: &str) -> Option<&'static Adapter> {
    (id == ADLER32.id).then_some(&ADLER32)
}

impl Adapter {
    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        for part in [
            "zore-native-static-c11-v1",
            self.id,
            self.configuration,
            self.bindings,
            self.source,
            self.header,
            self.bridge,
        ] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        for flag in C_FLAGS {
            hash.update((flag.len() as u64).to_le_bytes());
            hash.update(flag.as_bytes());
        }
        for (name, symbol) in self.symbols {
            for part in [name, symbol] {
                hash.update((part.len() as u64).to_le_bytes());
                hash.update(part.as_bytes());
            }
        }
        format!("{:x}", hash.finalize())
    }
}
