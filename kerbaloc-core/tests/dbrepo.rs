use kerbaloc_core::dbrepo::{build_index, validate_repo};
use std::fs;

fn make_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (variant, val) in [
        ("2026-08-16-manual-a", "안녕"),
        ("2026-08-17-gemini-b", "여보세요"),
    ] {
        let vd = d.path().join("packs/TestMod/ko/variants").join(variant);
        fs::create_dir_all(vd.join("Localization")).unwrap();
        fs::write(
            vd.join("pack.json"),
            format!(
                r#"{{"schema":"kerbaloc/pack@1","lang":"ko","mod_id":"TestMod","variant_id":"{variant}","src_sha256":"v1:sha256:{}","keys_translated":1,"keys_target":1}}"#,
                "0".repeat(64)
            ),
        )
        .unwrap();
        fs::write(
            vd.join("Localization/ko.cfg"),
            format!("Localization\n{{\n\tko\n\t{{\n\t\t#a = {val}\n\t}}\n}}\n"),
        )
        .unwrap();
    }
    d
}

#[test]
fn index_is_sorted_and_deterministic() {
    let d = make_repo();
    let a = build_index(d.path()).unwrap();
    let b = build_index(d.path()).unwrap();
    assert_eq!(a, b, "결정성");
    assert_eq!(a["schema"], "kerbaloc/index@1");
    let packs = a["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 1);
    let variants = packs[0]["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0]["variantId"], "2026-08-16-manual-a");
    let f = &variants[0]["files"][0];
    assert_eq!(f["sha256"].as_str().unwrap().len(), 64);
    assert!(f["sizeB"].as_u64().unwrap() > 0);
    assert!(variants[0]["path"]
        .as_str()
        .unwrap()
        .starts_with("packs/TestMod/ko/variants/"));
}

#[test]
fn validate_repo_passes_good_and_fails_broken() {
    let d = make_repo();
    let r = validate_repo(d.path());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // mod_id ≠ 경로 → 오류
    let bad = d
        .path()
        .join("packs/OtherMod/ko/variants/2026-08-16-manual-x");
    fs::create_dir_all(bad.join("Localization")).unwrap();
    fs::write(
        bad.join("pack.json"),
        format!(
            r#"{{"schema":"kerbaloc/pack@1","lang":"ko","mod_id":"WrongName","variant_id":"2026-08-16-manual-x","src_sha256":"v1:sha256:{}","keys_translated":1,"keys_target":1}}"#,
            "0".repeat(64)
        ),
    )
    .unwrap();
    fs::write(
        bad.join("Localization/ko.cfg"),
        "Localization\n{\n\tko\n\t{\n\t\t#a = 가나\n\t}\n}\n",
    )
    .unwrap();
    let r = validate_repo(d.path());
    assert!(!r.errors.is_empty());
}

#[test]
fn bad_variant_id_format_is_error() {
    let d = make_repo();
    let bad = d.path().join("packs/TestMod/ko/variants/UPPER_case!");
    fs::create_dir_all(bad.join("Localization")).unwrap();
    fs::write(
        bad.join("pack.json"),
        format!(
            r#"{{"schema":"kerbaloc/pack@1","lang":"ko","mod_id":"TestMod","variant_id":"UPPER_case!","src_sha256":"v1:sha256:{}","keys_translated":1,"keys_target":1}}"#,
            "0".repeat(64)
        ),
    )
    .unwrap();
    fs::write(
        bad.join("Localization/ko.cfg"),
        "Localization\n{\n\tko\n\t{\n\t\t#a = 가나\n\t}\n}\n",
    )
    .unwrap();
    let r = validate_repo(d.path());
    assert!(r.errors.iter().any(|e| e.contains("variant")));
}
