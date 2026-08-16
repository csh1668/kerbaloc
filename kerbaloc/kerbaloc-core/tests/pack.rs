use kerbaloc_core::pack::{install_pack, remove_pack, validate_pack, PackMeta};
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
