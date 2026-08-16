use kerbaloc_core::scan::{scan_gamedata, slug};
use std::fs;

fn write(p: &std::path::Path, rel: &str, content: &str) {
    let f = p.join(rel);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(f, content).unwrap();
}

const LOC: &str = "Localization\n{\n\ten-us\n\t{\n\t\t#LOC_A_x = Hello\n\t}\n\tja\n\t{\n\t\t#LOC_A_x = こんにちは\n\t}\n}\n";

fn make_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    // CKAN 소유 모드 (폴더명 ≠ identifier)
    write(d.path(), "GameData/000_Toolbar/Localization/en-us.cfg", LOC);
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(
        d.path().join("CKAN/registry.json"),
        r#"{
      "installed_modules": {"Toolbar": {"source_module": {"identifier":"Toolbar","name":"Toolbar Continued","version":"1.8.1.2","localizations":["en-us","ja"]}}},
      "installed_files": {"GameData/000_Toolbar/Localization/en-us.cfg": "Toolbar"}
    }"#,
    )
    .unwrap();
    // 수동 설치 + .version(GITHUB) — .version이 별도 하위 폴더에 있는 실측 패턴
    write(d.path(), "GameData/MyMod/Lang/strings.cfg", LOC);
    write(
        d.path(),
        "GameData/MyMod/Versioning/MyMod.version",
        r#"{"NAME":"MyMod","VERSION":"2.0.0","GITHUB":{"USERNAME":"u","REPOSITORY":"CoolMod"}}"#,
    );
    // 스톡
    write(d.path(), "GameData/Squad/Localization/dictionary.cfg", LOC);
    // .version 없는 수동 설치 (폴더명 폴백)
    write(d.path(), "GameData/zzz_Plain Mod/en-us.cfg", LOC);
    // 우리 산출물(제외 대상)
    write(
        d.path(),
        "GameData/KerbaLoc/ko/X/Localization/ko.cfg",
        "Localization\n{\n\tko\n\t{\n\t\t#a = 가\n\t}\n}\n",
    );
    d
}

#[test]
fn resolves_modids_by_priority() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let ids: Vec<&str> = units.iter().map(|u| u.mod_id.as_str()).collect();
    assert!(ids.contains(&"Toolbar"), "{ids:?}"); // CKAN identifier
    assert!(ids.contains(&"local.CoolMod"), "{ids:?}"); // AVC GITHUB repo
    assert!(ids.contains(&"Squad"), "{ids:?}"); // 예약 스톡
    assert!(ids.contains(&"local.Plain-Mod"), "{ids:?}"); // 폴더명 slug 폴백
    assert!(!ids.iter().any(|i| i.contains("KerbaLoc")), "{ids:?}"); // 자기 산출물 제외
}

#[test]
fn unit_carries_hash_version_and_official_langs() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let t = units.iter().find(|u| u.mod_id == "Toolbar").unwrap();
    assert!(t.source_hash.starts_with("v1:sha256:"));
    assert_eq!(t.version.raw.as_deref(), Some("1.8.1.2"));
    assert_eq!(t.version.source, "ckan");
    assert!(t.official_langs.contains(&"ja".to_string()));
    assert_eq!(t.entries.len(), 1);

    let m = units.iter().find(|u| u.mod_id == "local.CoolMod").unwrap();
    assert_eq!(m.version.raw.as_deref(), Some("2.0.0"));
    assert_eq!(m.version.source, "avc");
}

#[test]
fn slug_rules() {
    assert_eq!(slug("000_Toolbar"), "Toolbar");
    assert_eq!(slug("zzz_Deferred"), "Deferred");
    assert_eq!(slug("Adiri's TUFX Profiles"), "Adiris-TUFX-Profiles");
    assert_eq!(slug("[x]_Science!"), "x-Science");
}
