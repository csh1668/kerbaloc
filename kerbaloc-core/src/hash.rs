use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub fn normalize_value(v: &str) -> String {
    let v = v.replace("\r\n", "\n").replace('\r', "\n");
    let v: String = v.nfc().collect();
    let lines: Vec<&str> = v.split('\n').map(|l| l.trim_end()).collect();
    lines.join("\n").trim().to_string()
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn source_hash(entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    for (k, v) in entries {
        h.update(k.as_bytes());
        h.update([0x1f]);
        h.update(normalize_value(v).as_bytes());
        h.update([0x1e]);
    }
    format!("v1:sha256:{}", hex(&h.finalize()))
}

pub fn keys_hash(entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    for k in entries.keys() {
        h.update(k.as_bytes());
        h.update([0x1e]);
    }
    format!("v1:sha256:{}", hex(&h.finalize()))
}

pub fn key_fingerprint(value: &str) -> String {
    let mut h = Sha256::new();
    h.update(normalize_value(value).as_bytes());
    hex(&h.finalize())[..8].to_string()
}
