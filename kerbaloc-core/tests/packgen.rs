use kerbaloc_core::packgen::{build_pack, make_variant_id};
use kerbaloc_core::scan::{IdSource, ModUnit, VersionInfo};
use kerbaloc_core::{cfg, loc, pack, parts};
use std::collections::BTreeMap;

fn unit() -> ModUnit {
    let mut entries = BTreeMap::new();
    entries.insert("#a".to_string(), "Hello <<1>>".to_string());
    entries.insert("#b".to_string(), "World".to_string());
    ModUnit {
        mod_id: "TestMod".into(),
        id_source: IdSource::Folder,
        display_name: "Test Mod".into(),
        version: VersionInfo {
            raw: None,
            source: "unknown",
        },
        files: vec![],
        source_hash: kerbaloc_core::hash::source_hash(&entries),
        keys_hash: kerbaloc_core::hash::keys_hash(&entries),
        official_langs: vec![],
        entries,
        part_fields: BTreeMap::new(),
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
        serde_json::from_str(&std::fs::read_to_string(d.path().join("pack.json")).unwrap())
            .unwrap();
    assert_eq!(meta["src_sha256"], u.source_hash.as_str());
    assert_eq!(meta["keys_translated"], 2);
    assert!(meta.get("part_fields").is_none());
    assert!(!d.path().join("Patches/parts.cfg").exists());
    assert!(!d.path().join("Localization/en-us.cfg").exists());
}

fn with_literal_parts(mut unit: ModUnit) -> ModUnit {
    for (field, source) in [("title", "Literal Engine <<1>>"), ("description", "Original description")] {
        let key = parts::key(&unit.mod_id, "literal.Part-1", field);
        unit.entries.insert(key.clone(), source.into());
        unit.part_fields.insert(
            key,
            parts::PartField {
                part_name: "literal.Part-1".into(),
                field: field.into(),
                source: source.into(),
            },
        );
    }
    unit.source_hash = kerbaloc_core::hash::source_hash(&unit.entries);
    unit.keys_hash = kerbaloc_core::hash::keys_hash(&unit.entries);
    unit
}

fn read_entries(dir: &std::path::Path, lang: &str) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(dir.join(format!("Localization/{lang}.cfg"))).unwrap();
    loc::extract_localization(&cfg::parse(&text).unwrap(), lang)
}

#[test]
fn partial_mixed_pack_only_patches_translated_part_fields() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let description = parts::key(&unit.mod_id, "literal.Part-1", "description");
    let translations = BTreeMap::from([
        ("#a".into(), "안녕 <<1>>".into()),
        (title.clone(), "리터럴 엔진 <<1>>".into()),
    ]);
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &translations, "2026-11-23-manual-test", "m").unwrap();
    assert_eq!(read_entries(dir.path(), "ko"), translations);
    let fallback = read_entries(dir.path(), "en-us");
    assert_eq!(fallback, BTreeMap::from([(title.clone(), "Literal Engine <<1>>".into())]));
    assert!(!fallback.contains_key("#a"));
    assert!(!fallback.contains_key(&description));
    let patch = std::fs::read_to_string(dir.path().join("Patches/parts.cfg")).unwrap();
    assert_eq!(patch, format!("@PART[literal.Part-1]\n{{\n\t@title = {title}\n}}\n"));
    assert!(!patch.contains("@description"));
    assert!(!patch.contains("@name"));
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("pack.json")).unwrap(),
    ).unwrap();
    assert_eq!(meta["part_fields"].as_object().unwrap().len(), 1);
    assert_eq!(meta["part_fields"][&title]["part_name"], "literal.Part-1");
    assert_eq!(meta["keys_target"], 4);
    assert!(pack::validate_pack(dir.path(), Some(&unit.entries)).errors.is_empty());
}

#[test]
fn part_only_pack_has_original_english_fallback_for_both_selected_fields() {
    let mut unit = with_literal_parts(unit());
    unit.entries.retain(|key, _| unit.part_fields.contains_key(key));
    unit.source_hash = kerbaloc_core::hash::source_hash(&unit.entries);
    unit.keys_hash = kerbaloc_core::hash::keys_hash(&unit.entries);
    let translations: BTreeMap<String, String> = unit.part_fields.iter().map(|(key, field)| {
        (key.clone(), if field.field == "title" { "리터럴 엔진 <<1>>" } else { "번역한 설명" }.into())
    }).collect();
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &translations, "2026-11-23-manual-test", "m").unwrap();
    assert_eq!(read_entries(dir.path(), "en-us"), unit.entries);
    assert_eq!(read_entries(dir.path(), "ko"), translations);
    let patch = cfg::parse(&std::fs::read_to_string(dir.path().join("Patches/parts.cfg")).unwrap()).unwrap();
    assert_eq!(patch.children.len(), 1);
    assert_eq!(patch.children[0].name, "@PART[literal.Part-1]");
    assert_eq!(patch.children[0].values.len(), 2);
    assert_eq!(patch.children[0].values[0].0, "@title");
    assert_eq!(patch.children[0].values[1].0, "@description");
    assert!(patch.children[0].children.is_empty());
}

#[test]
fn rebuilding_removes_only_stale_generated_sidefiles() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([(title, "엔진 <<1>>".into())]),
        "2026-11-23-manual-test", "m").unwrap();
    std::fs::write(dir.path().join("Patches/custom.cfg"), "// user-owned\n").unwrap();
    std::fs::write(dir.path().join("Localization/notes.md"), "user notes\n").unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([("#b".into(), "세계".into())]),
        "2026-11-23-manual-test", "m").unwrap();
    assert!(!dir.path().join("Patches/parts.cfg").exists());
    assert!(!dir.path().join("Localization/en-us.cfg").exists());
    assert_eq!(std::fs::read_to_string(dir.path().join("Patches/custom.cfg")).unwrap(), "// user-owned\n");
    assert!(dir.path().join("Localization/notes.md").is_file());
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("pack.json")).unwrap(),
    ).unwrap();
    assert!(meta.get("part_fields").is_none());
}

#[test]
fn rebuilding_a_partial_pack_drops_unselected_part_fields() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let description = parts::key(&unit.mod_id, "literal.Part-1", "description");
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([
        (title.clone(), "엔진 <<1>>".into()), (description.clone(), "설명".into())]),
        "2026-11-23-manual-test", "m").unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([(description.clone(), "새 설명".into())]),
        "2026-11-23-manual-test", "m").unwrap();
    let fallback = read_entries(dir.path(), "en-us");
    assert!(!fallback.contains_key(&title));
    assert_eq!(fallback.len(), 1);
    assert!(fallback.contains_key(&description));
    let patch = std::fs::read_to_string(dir.path().join("Patches/parts.cfg")).unwrap();
    assert!(!patch.contains("@title"));
    assert!(patch.contains("@description"));
}

#[test]
fn unsafe_or_mismatched_part_data_is_rejected_before_writing() {
    for (part_name, field, source) in [
        ("literal*", "title", "Literal Engine <<1>>"),
        ("literal.Part-1", "name", "Literal Engine <<1>>"),
        ("literal.Part-1", "title", "Changed source"),
    ] {
        let mut unit = with_literal_parts(unit());
        let key = parts::key(&unit.mod_id, "literal.Part-1", "title");
        let data = unit.part_fields.get_mut(&key).unwrap();
        data.part_name = part_name.into();
        data.field = field.into();
        data.source = source.into();
        let dir = tempfile::tempdir().unwrap();
        assert!(build_pack(dir.path(), &unit, &BTreeMap::from([(key, "엔진 <<1>>".into())]),
            "2026-11-23-manual-test", "m").is_err());
        assert!(!dir.path().join("Localization/ko.cfg").exists());
    }
}

#[test]
fn unknown_synthetic_key_and_duplicate_engine_tokens_are_rejected() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let dir = tempfile::tempdir().unwrap();
    assert!(build_pack(dir.path(), &unit, &BTreeMap::from([(title, "엔진 <<1>> <<1>>".into())]),
        "2026-11-23-manual-test", "m").is_err());
    let unknown = parts::key(&unit.mod_id, "otherPart", "title");
    assert!(build_pack(dir.path(), &unit, &BTreeMap::from([(unknown, "엔진".into())]),
        "2026-11-23-manual-test", "m").is_err());
}

#[test]
fn empty_or_invalid_rebuild_preserves_previous_pack_files() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([(title.clone(), "엔진 <<1>>".into())]),
        "2026-11-23-manual-test", "m").unwrap();
    let files = ["Localization/ko.cfg", "Localization/en-us.cfg", "Patches/parts.cfg", "pack.json"];
    let before: Vec<_> = files.iter().map(|file| std::fs::read(dir.path().join(file)).unwrap()).collect();
    for translations in [BTreeMap::new(), BTreeMap::from([(title, "누락한 토큰".into())])] {
        assert!(build_pack(dir.path(), &unit, &translations, "2026-11-23-manual-test", "m").is_err());
        for (file, bytes) in files.iter().zip(&before) {
            assert_eq!(&std::fs::read(dir.path().join(file)).unwrap(), bytes, "{file}");
        }
    }
}

#[test]
fn unchanged_part_field_is_not_counted_as_changed_translation() {
    let unit = with_literal_parts(unit());
    let title = parts::key(&unit.mod_id, "literal.Part-1", "title");
    let dir = tempfile::tempdir().unwrap();
    build_pack(dir.path(), &unit, &BTreeMap::from([(title.clone(), unit.entries[&title].clone())]),
        "2026-11-23-manual-test", "m").unwrap();
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("pack.json")).unwrap(),
    ).unwrap();
    assert_eq!(meta["keys_translated"], 0);
    let report = pack::validate_pack(dir.path(), Some(&unit.entries));
    assert!(report.errors.is_empty());
    assert!(!report.warnings.iter().any(|warning| warning.contains("커버리지 재계산")));
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
