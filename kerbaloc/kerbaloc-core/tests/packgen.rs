use kerbaloc_core::packgen::{build_pack, make_variant_id};
use kerbaloc_core::scan::{IdSource, ModUnit, VersionInfo};
use kerbaloc_core::{cfg, pack};
use std::collections::BTreeMap;

fn unit() -> ModUnit {
    let mut entries = BTreeMap::new();
    entries.insert("#a".to_string(), "Hello <<1>>".to_string());
    entries.insert("#b".to_string(), "World".to_string());
    ModUnit {
        mod_id: "TestMod".into(),
        id_source: IdSource::Folder,
        display_name: "Test Mod".into(),
        version: VersionInfo { raw: None, source: "unknown" },
        files: vec![],
        source_hash: kerbaloc_core::hash::source_hash(&entries),
        keys_hash: kerbaloc_core::hash::keys_hash(&entries),
        official_langs: vec![],
        entries,
    }
}

#[test]
fn built_pack_passes_validation_and_roundtrips() {
    let u = unit();
    let mut tr = BTreeMap::new();
    tr.insert("#a".to_string(), "안녕 <<1>>".to_string());
    tr.insert("#b".to_string(), "세계".to_string());
    let d = tempfile::tempdir().unwrap();
    build_pack(d.path(), &u, &tr, "2026-08-16-test-nick", "fake-model").unwrap();

    let r = pack::validate_pack(d.path(), Some(&u.entries));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let text = std::fs::read_to_string(d.path().join("Localization/ko.cfg")).unwrap();
    assert!(cfg::roundtrip_ok(&text).unwrap());
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("pack.json")).unwrap()).unwrap();
    assert_eq!(meta["src_sha256"], u.source_hash.as_str());
    assert_eq!(meta["keys_translated"], 2);
}

#[test]
fn broken_translation_makes_build_fail() {
    let u = unit();
    let mut tr = BTreeMap::new();
    tr.insert("#a".to_string(), "안녕".to_string()); // <<1>> 누락
    let d = tempfile::tempdir().unwrap();
    assert!(build_pack(d.path(), &u, &tr, "2026-08-16-test-nick", "m").is_err());
}

#[test]
fn variant_id_format() {
    let id = make_variant_id("gemini31flashlite", "김서현!");
    // 날짜-슬러그-닉 (비ASCII 닉은 anon)
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}-gemini31flashlite-anon$").unwrap();
    assert!(re.is_match(&id), "{id}");
    let id2 = make_variant_id("manual", "Nick_01");
    assert!(id2.ends_with("-manual-nick01"), "{id2}");
}
