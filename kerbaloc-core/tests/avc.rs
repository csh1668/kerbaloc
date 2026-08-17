use kerbaloc_core::avc::parse_version_file;

#[test]
fn object_version() {
    let t = r#"{"NAME":"CRP","VERSION":{"MAJOR":1,"MINOR":4,"PATCH":2}}"#;
    let a = parse_version_file(t).unwrap();
    assert_eq!(a.version.as_deref(), Some("1.4.2"));
    assert_eq!(a.name.as_deref(), Some("CRP"));
}

#[test]
fn string_version_with_v_prefix() {
    let a = parse_version_file(r#"{"NAME":"X","VERSION":"v1.2.0"}"#).unwrap();
    assert_eq!(a.version_raw.as_deref(), Some("v1.2.0"));
    assert_eq!(a.version.as_deref(), Some("1.2.0"));
}

#[test]
fn trailing_commas_tolerated() {
    // 실측: ASET 등 10/162 파일
    let t = "{\"NAME\":\"A\",\"VERSION\":{\"MAJOR\":2,\"MINOR\":0,\"PATCH\":2,},}";
    let a = parse_version_file(t).unwrap();
    assert_eq!(a.version.as_deref(), Some("2.0.2"));
}

#[test]
fn bom_and_case_insensitive_keys() {
    let t = "\u{feff}{\"name\":\"A\",\"version\":\"3.3.1.0\",\"GITHUB\":{\"USERNAME\":\"u\",\"REPOSITORY\":\"r\"}}";
    let a = parse_version_file(t).unwrap();
    assert_eq!(a.version.as_deref(), Some("3.3.1.0"));
    assert_eq!(a.github, Some(("u".to_string(), "r".to_string())));
}

#[test]
fn excess_closing_brace_tolerated() {
    // 실측: SpaceTuxLibrary/VesselModuleSave.version
    let t = r#"{"NAME":"A","VERSION":"1.0.0"}}"#;
    assert!(parse_version_file(t).is_some());
}

#[test]
fn garbage_returns_none() {
    assert!(parse_version_file("not json at all").is_none());
}
