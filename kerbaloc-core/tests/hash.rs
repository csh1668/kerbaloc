use kerbaloc_core::hash::{key_fingerprint, keys_hash, normalize_value, source_hash};
use std::collections::BTreeMap;

fn m(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn hash_has_version_prefix() {
    let h = source_hash(&m(&[("#a", "x")]));
    assert!(h.starts_with("v1:sha256:"));
    assert_eq!(h.len(), "v1:sha256:".len() + 64);
}

#[test]
fn insensitive_to_crlf_and_trailing_ws() {
    let a = m(&[("#a", "line1\r\nline2  ")]);
    let b = m(&[("#a", "line1\nline2")]);
    assert_eq!(source_hash(&a), source_hash(&b));
}

#[test]
fn preserves_literal_backslash_n_and_tokens() {
    // 리터럴 "\n"(역슬래시+n)과 실제 개행은 다른 값이어야 함
    let a = m(&[("#a", "x\\ny <<1>> 커발^N ｢z｣")]);
    let b = m(&[("#a", "x\ny <<1>> 커발^N ｢z｣")]);
    assert_ne!(source_hash(&a), source_hash(&b));
}

#[test]
fn keys_hash_ignores_values() {
    assert_eq!(keys_hash(&m(&[("#a", "1")])), keys_hash(&m(&[("#a", "2")])));
    assert_ne!(keys_hash(&m(&[("#a", "1")])), keys_hash(&m(&[("#b", "1")])));
}

#[test]
fn fingerprint_is_8_hex() {
    let f = key_fingerprint("Hello");
    assert_eq!(f.len(), 8);
    assert!(f.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn normalize_keeps_internal_spaces() {
    assert_eq!(normalize_value("a  b  "), "a  b");
}
