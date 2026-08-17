use kerbaloc_core::ckan::CkanRegistry;
use std::fs;

fn make_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(
        d.path().join("CKAN/registry.json"),
        r#"{
        "installed_modules": {
            "Toolbar": {"source_module": {"identifier":"Toolbar","name":"Toolbar Continued","version":"1.8.1.2","localizations":["en-us","ja"]}}
        },
        "installed_files": {
            "GameData/000_Toolbar/Localization/en-us.cfg": "Toolbar",
            "GameData/000_Toolbar/Toolbar.dll": "Toolbar"
        }
    }"#,
    )
    .unwrap();
    d
}

#[test]
fn loads_and_resolves_owner_case_insensitive() {
    let d = make_root();
    let r = CkanRegistry::load(d.path()).unwrap();
    assert_eq!(
        r.owner_of("gamedata/000_toolbar/localization/EN-US.CFG"),
        Some("Toolbar")
    );
    assert_eq!(r.owner_of("GameData/Unknown/x.cfg"), None);
}

#[test]
fn module_metadata() {
    let d = make_root();
    let r = CkanRegistry::load(d.path()).unwrap();
    let m = r.module("Toolbar").unwrap();
    assert_eq!(m.version.as_deref(), Some("1.8.1.2"));
    assert_eq!(m.localizations, vec!["en-us", "ja"]);
}

#[test]
fn missing_or_broken_registry_is_none() {
    let d = tempfile::tempdir().unwrap();
    assert!(CkanRegistry::load(d.path()).is_none());
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(d.path().join("CKAN/registry.json"), "{broken").unwrap();
    assert!(CkanRegistry::load(d.path()).is_none());
}
