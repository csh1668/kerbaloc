use kerbaloc_core::pack::{install_pack, remove_pack, validate_pack, PackMeta};
use kerbaloc_core::{cfg, loc, parts};
use std::collections::BTreeMap;
use std::fs;

fn make_pack(dir: &std::path::Path, cfg_body: &str) {
    fs::create_dir_all(dir.join("Localization")).unwrap();
    let meta = PackMeta {
        schema: "kerbaloc/pack@1".to_string(),
        lang: "ko".to_string(),
        mod_id: "TestMod".to_string(),
        variant_id: "2026-08-16-manual-test".to_string(),
        src_sha256: format!("v1:sha256:{}", "0".repeat(64)),
        keys_translated: 1,
        keys_target: 1,
    };
    fs::write(
        dir.join("pack.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("Localization/ko.cfg"), cfg_body).unwrap();
}

const GOOD: &str = "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕\n\t}\n}\n";
const OTHER_LANG: &str =
    "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕\n\t}\n\tja\n\t{\n\t\t#a = x\n\t}\n}\n";

#[test]
fn valid_pack_passes() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), GOOD);
    let r = validate_pack(d.path(), None);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn other_language_node_is_error() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), OTHER_LANG);
    assert!(!validate_pack(d.path(), None).errors.is_empty());
}

#[test]
fn bom_is_error() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), GOOD);
    let with_bom = [b"\xef\xbb\xbf".to_vec(), GOOD.as_bytes().to_vec()].concat();
    fs::write(d.path().join("Localization/ko.cfg"), with_bom).unwrap();
    assert!(!validate_pack(d.path(), None).errors.is_empty());
}

#[test]
fn raw_brace_in_value_line_is_error_before_parsing() {
    // 값의 원시 중괄호는 파싱 시 구조로 흡수되어 사라지므로(게임도 동일하게 오파싱),
    // 파싱 전 원문 줄 검사로 잡아야 한다.
    let d = tempfile::tempdir().unwrap();
    make_pack(
        d.path(),
        "Localization\n{\n\tko\n\t{\n\t\t#a = {안녕}\n\t}\n}\n",
    );
    let r = validate_pack(d.path(), None);
    assert!(
        r.errors.iter().any(|e| e.contains("중괄호")),
        "{:?}",
        r.errors
    );
}

#[test]
fn token_check_against_source() {
    use std::collections::BTreeMap;
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), GOOD);
    let mut src = BTreeMap::new();
    src.insert("#a".to_string(), "Hi <<1>>".to_string()); // 원문 토큰이 번역에 없음
    let r = validate_pack(d.path(), Some(&src));
    assert!(!r.errors.is_empty());
}

#[test]
fn displayname_shorter_than_2_chars_is_error() {
    // PartResourceDefinition.GetShortName()이 displayName.Substring(0, 2)를 호출하므로
    // 2자 미만 displayName은 로딩 프리즈를 유발한다 (2026-08-16 실게임 재현: Karbonite).
    use std::collections::BTreeMap;
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("Localization")).unwrap();
    let meta = PackMeta {
        schema: "kerbaloc/pack@1".to_string(),
        lang: "ko".to_string(),
        mod_id: "TestMod".to_string(),
        variant_id: "2026-08-16-manual-test".to_string(),
        src_sha256: format!("v1:sha256:{}", "0".repeat(64)),
        keys_translated: 1,
        keys_target: 1,
    };
    fs::write(
        d.path().join("pack.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .unwrap();
    fs::write(
        d.path().join("Localization/ko.cfg"),
        "Localization\n{\n\tko\n\t{\n\t\t#LOC_X_Ore_DisplayName = 광\n\t}\n}\n",
    )
    .unwrap();
    let mut src = BTreeMap::new();
    src.insert("#LOC_X_Ore_DisplayName".to_string(), "Ore".to_string());
    let r = validate_pack(d.path(), Some(&src));
    assert!(r.errors.iter().any(|e| e.contains("2자")), "{:?}", r.errors);
}

#[test]
fn install_and_remove() {
    let ksp = tempfile::tempdir().unwrap();
    fs::create_dir_all(ksp.path().join("GameData")).unwrap();
    let pack = tempfile::tempdir().unwrap();
    make_pack(pack.path(), GOOD);
    let dest = install_pack(ksp.path(), pack.path()).unwrap();
    assert!(dest.join("Localization/ko.cfg").is_file());
    assert!(dest
        .to_string_lossy()
        .replace('\\', "/")
        .contains("GameData/KerbaLoc/ko/TestMod"));
    assert!(remove_pack(ksp.path(), "ko", "TestMod").unwrap());
    assert!(!dest.exists());
    assert!(!remove_pack(ksp.path(), "ko", "TestMod").unwrap()); // 멱등
}

fn part_pack(dir: &std::path::Path) -> (String, BTreeMap<String, String>) {
    let key = parts::key("TestMod", "literalPart", "title");
    let source = "Original English title";
    make_pack(
        dir,
        &format!("Localization\n{{\n\tko\n\t{{\n\t\t{key} = 번역한 부품 제목\n\t}}\n}}\n"),
    );
    let path = dir.join("pack.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let field = serde_json::to_value(parts::PartField {
        part_name: "literalPart".into(),
        field: "title".into(),
        source: source.into(),
    })
    .unwrap();
    let mut fields = serde_json::Map::new();
    fields.insert(key.clone(), field);
    meta["part_fields"] = serde_json::Value::Object(fields);
    fs::write(path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    fs::create_dir_all(dir.join("Patches")).unwrap();
    fs::write(
        dir.join("Patches/parts.cfg"),
        format!("@PART[literalPart]\n{{\n\t@title = {key}\n}}\n"),
    )
    .unwrap();
    fs::write(
        dir.join("Localization/en-us.cfg"),
        format!("Localization\n{{\n\ten-us\n\t{{\n\t\t{key} = {source}\n\t}}\n}}\n"),
    )
    .unwrap();
    (key.clone(), BTreeMap::from([(key, source.into())]))
}

fn change_meta(dir: &std::path::Path, change: impl FnOnce(&mut serde_json::Value)) {
    let path = dir.join("pack.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut meta);
    fs::write(path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
}

#[test]
fn generated_part_files_validate_and_keep_english_separate_from_ko() {
    let dir = tempfile::tempdir().unwrap();
    let (key, source) = part_pack(dir.path());
    assert!(validate_pack(dir.path(), None).errors.is_empty());
    assert!(validate_pack(dir.path(), Some(&source)).errors.is_empty());
    let ko =
        cfg::parse(&fs::read_to_string(dir.path().join("Localization/ko.cfg")).unwrap()).unwrap();
    assert!(loc::extract_localization(&ko, "en-us").is_empty());
    let en = cfg::parse(&fs::read_to_string(dir.path().join("Localization/en-us.cfg")).unwrap())
        .unwrap();
    assert_eq!(loc::extract_localization(&en, "en-us"), source);
    assert_eq!(
        loc::extract_localization(&en, "en-us")[&key],
        "Original English title"
    );
    assert!(loc::extract_localization(&en, "ko").is_empty());
}

#[test]
fn generated_files_accept_crlf_without_relaxing_structure() {
    let dir = tempfile::tempdir().unwrap();
    let (_, source) = part_pack(dir.path());
    for relative in [
        "Localization/ko.cfg",
        "Localization/en-us.cfg",
        "Patches/parts.cfg",
    ] {
        let path = dir.path().join(relative);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(path, text.replace('\n', "\r\n")).unwrap();
    }
    assert!(validate_pack(dir.path(), Some(&source)).errors.is_empty());
}

#[test]
fn generated_pack_requires_both_sidefiles_and_corresponding_ko_key() {
    for missing in [
        "Patches/parts.cfg",
        "Localization/en-us.cfg",
        "Localization/ko.cfg",
    ] {
        let dir = tempfile::tempdir().unwrap();
        part_pack(dir.path());
        fs::remove_file(dir.path().join(missing)).unwrap();
        assert!(
            !validate_pack(dir.path(), None).errors.is_empty(),
            "{missing}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    part_pack(dir.path());
    fs::write(dir.path().join("Localization/ko.cfg"), GOOD).unwrap();
    assert!(validate_pack(dir.path(), None)
        .errors
        .iter()
        .any(|error| error.contains("ko 번역 키")));
}

#[test]
fn generated_patch_rejects_wildcard_extra_fields_reference_changes_and_injection() {
    for change in ["wildcard", "field", "reference", "node", "pass"] {
        let dir = tempfile::tempdir().unwrap();
        let (key, _) = part_pack(dir.path());
        let path = dir.path().join("Patches/parts.cfg");
        let text = fs::read_to_string(&path).unwrap();
        let corrupt = match change {
            "wildcard" => text.replace("@PART[literalPart]", "@PART[*]"),
            "field" => text.replace("@title", "@name"),
            "reference" => text.replace(&key, "#LOC_OtherMod_title"),
            "node" => text + "@PART[anotherPart]\n{\n\t@mass = 0\n}\n",
            "pass" => text.replace("@PART[literalPart]", "@PART[literalPart]:FINAL"),
            _ => unreachable!(),
        };
        fs::write(path, corrupt).unwrap();
        assert!(
            !validate_pack(dir.path(), None).errors.is_empty(),
            "{change}"
        );
    }
}

#[test]
fn generated_metadata_rejects_unsafe_targets_fields_keys_and_unknown_properties() {
    for change in ["part", "field", "key", "extra"] {
        let dir = tempfile::tempdir().unwrap();
        let (key, _) = part_pack(dir.path());
        change_meta(dir.path(), |meta| match change {
            "part" => meta["part_fields"][&key]["part_name"] = "literalPart,*".into(),
            "field" => meta["part_fields"][&key]["field"] = "mass".into(),
            "extra" => meta["part_fields"][&key]["pass"] = ":FINAL".into(),
            "key" => {
                let object = meta["part_fields"].as_object_mut().unwrap();
                let value = object.remove(&key).unwrap();
                object.insert("#KerbaLoc_Part_wrong_title".into(), value);
            }
            _ => unreachable!(),
        });
        assert!(
            !validate_pack(dir.path(), None).errors.is_empty(),
            "{change}"
        );
    }
}

#[test]
fn tampered_fallback_and_source_binding_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (key, source) = part_pack(dir.path());
    let path = dir.path().join("Localization/en-us.cfg");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("Original English title", "Changed English title"),
    )
    .unwrap();
    assert!(!validate_pack(dir.path(), None).errors.is_empty());
    change_meta(dir.path(), |meta| {
        meta["part_fields"][&key]["source"] = "Changed English title".into()
    });
    assert!(validate_pack(dir.path(), None).errors.is_empty());
    assert!(validate_pack(dir.path(), Some(&source))
        .errors
        .iter()
        .any(|error| error.contains("원문과 일치")));
}

#[test]
fn fallback_rejects_other_languages_extra_keys_and_injected_nodes() {
    for addition in ["other-language", "extra-key", "injection"] {
        let dir = tempfile::tempdir().unwrap();
        part_pack(dir.path());
        let path = dir.path().join("Localization/en-us.cfg");
        let text = fs::read_to_string(&path).unwrap();
        let corrupted = match addition {
            "other-language" => text.replace("en-us", "ko"),
            "extra-key" => text.replace("\t}\n", "\t\t#other = Other source\n\t}\n"),
            "injection" => text + "@PART[anotherPart]\n{\n\t@title = bad\n}\n",
            _ => unreachable!(),
        };
        fs::write(path, corrupted).unwrap();
        assert!(
            !validate_pack(dir.path(), None).errors.is_empty(),
            "{addition}"
        );
    }
}

#[test]
fn generated_sidefiles_or_synthetic_keys_without_metadata_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    part_pack(dir.path());
    change_meta(dir.path(), |meta| {
        meta.as_object_mut().unwrap().remove("part_fields");
    });
    assert!(!validate_pack(dir.path(), None).errors.is_empty());
    fs::remove_file(dir.path().join("Patches/parts.cfg")).unwrap();
    fs::remove_file(dir.path().join("Localization/en-us.cfg")).unwrap();
    assert!(!validate_pack(dir.path(), None).errors.is_empty());
}

#[test]
fn generated_pack_does_not_allow_english_inside_ko_cfg() {
    let dir = tempfile::tempdir().unwrap();
    part_pack(dir.path());
    let path = dir.path().join("Localization/ko.cfg");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        path,
        text + "Localization\n{\n\ten-us\n\t{\n\t\t#other = Source\n\t}\n}\n",
    )
    .unwrap();
    let report = validate_pack(dir.path(), None);
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("ko 노드만")));
}

#[test]
fn module_manager_required_before_replacing_existing_installation() {
    let ksp = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    make_pack(old.path(), GOOD);
    let dest = install_pack(ksp.path(), old.path()).unwrap();
    let replacement = tempfile::tempdir().unwrap();
    part_pack(replacement.path());
    let error = install_pack(ksp.path(), replacement.path()).unwrap_err();
    assert!(error.to_string().contains("ModuleManager"));
    assert_eq!(
        fs::read_to_string(dest.join("Localization/ko.cfg")).unwrap(),
        GOOD
    );
    assert!(!dest.join("Patches/parts.cfg").exists());
    fs::write(ksp.path().join("GameData/ModuleManager.4.2.3.dll"), []).unwrap();
    install_pack(ksp.path(), replacement.path()).unwrap();
    assert!(dest.join("Patches/parts.cfg").is_file());
    assert!(dest.join("Localization/en-us.cfg").is_file());
}

#[test]
fn corrupt_replacement_is_rejected_before_deleting_old_installation() {
    let ksp = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    make_pack(old.path(), GOOD);
    let dest = install_pack(ksp.path(), old.path()).unwrap();
    fs::write(ksp.path().join("GameData/ModuleManager.4.2.3.dll"), []).unwrap();
    let replacement = tempfile::tempdir().unwrap();
    part_pack(replacement.path());
    fs::write(
        replacement.path().join("Patches/parts.cfg"),
        "@PART[*]\n{\n\t@name = bad\n}\n",
    )
    .unwrap();
    assert_eq!(
        install_pack(ksp.path(), replacement.path())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        fs::read_to_string(dest.join("Localization/ko.cfg")).unwrap(),
        GOOD
    );
}

#[test]
fn removal_restores_unmodified_literal_source_and_removes_generated_files() {
    let ksp = tempfile::tempdir().unwrap();
    let original = ksp.path().join("GameData/TestMod/Parts/part.cfg");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    let source = "PART\n{\n\tname = literalPart\n\ttitle = Original English title\n}\n";
    fs::write(&original, source).unwrap();
    fs::write(ksp.path().join("GameData/ModuleManager.4.2.3.dll"), []).unwrap();
    let pack = tempfile::tempdir().unwrap();
    part_pack(pack.path());
    let installed = install_pack(ksp.path(), pack.path()).unwrap();
    assert_eq!(fs::read_to_string(&original).unwrap(), source);
    assert!(remove_pack(ksp.path(), "ko", "TestMod").unwrap());
    assert!(!installed.exists());
    assert_eq!(fs::read_to_string(original).unwrap(), source);
}

#[test]
fn extra_generated_cfg_is_rejected_but_legacy_pack_behavior_is_retained() {
    let dir = tempfile::tempdir().unwrap();
    part_pack(dir.path());
    fs::write(
        dir.path().join("Patches/override.cfg"),
        "@PART[*]\n{\n\t@name = bad\n}\n",
    )
    .unwrap();
    assert!(validate_pack(dir.path(), None)
        .errors
        .iter()
        .any(|error| error.contains("추가 CFG")));
    let legacy = tempfile::tempdir().unwrap();
    make_pack(legacy.path(), GOOD);
    fs::create_dir_all(legacy.path().join("Patches")).unwrap();
    fs::write(
        legacy.path().join("Patches/custom.cfg"),
        "// existing legacy file\n",
    )
    .unwrap();
    assert!(validate_pack(legacy.path(), None).errors.is_empty());
}

#[cfg(unix)]
#[test]
fn failed_replacement_copy_preserves_working_installation() {
    use std::os::unix::fs::symlink;
    let ksp = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    make_pack(old.path(), GOOD);
    let dest = install_pack(ksp.path(), old.path()).unwrap();
    let replacement = tempfile::tempdir().unwrap();
    make_pack(replacement.path(), GOOD);
    symlink(
        replacement.path().join("missing-asset"),
        replacement.path().join("broken-asset"),
    )
    .unwrap();
    assert!(validate_pack(replacement.path(), None).errors.is_empty());
    assert!(install_pack(ksp.path(), replacement.path()).is_err());
    assert_eq!(
        fs::read_to_string(dest.join("Localization/ko.cfg")).unwrap(),
        GOOD
    );
    assert!(!dest.join("broken-asset").exists());
}
