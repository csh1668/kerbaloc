use kerbaloc_core::glossary::{Glossary, Policy};

fn core() -> Glossary {
    Glossary::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../glossary/core.ko.json"
    )))
    .unwrap()
}

#[test]
fn embedded_core_matches_file_version() {
    let g = Glossary::embedded_core();
    let hits = g.matches(&["Transfer Water to the tank"]);
    assert!(hits.iter().any(|e| e.en == "Water"), "내장 용어집 동작");
}

#[test]
fn loads_core_seed() {
    let g = core();
    let hits = g.matches(&["Transfer Water to the tank"]);
    let water = hits.iter().find(|e| e.en == "Water").unwrap();
    assert_eq!(water.ko.as_deref(), Some("식수"));
    assert!(matches!(water.policy, Policy::Translate));
}

#[test]
fn word_boundary_and_alias_matching() {
    let g = core();
    // "Apollo"의 "Ap"는 단어 경계가 아니므로 매칭 금지
    assert!(!g
        .matches(&["Apollo program"])
        .iter()
        .any(|e| e.en == "apoapsis"));
    // alias "dV"는 단어 경계로 매칭
    assert!(g
        .matches(&["Total dV required"])
        .iter()
        .any(|e| e.en == "delta-v"));
    // 대소문자 무시
    assert!(g
        .matches(&["THRUST limiter"])
        .iter()
        .any(|e| e.en == "thrust"));
}

#[test]
fn prompt_block_format() {
    let g = core();
    let hits = g.matches(&["Water and m/s"]);
    let block = Glossary::prompt_block(&hits);
    assert!(block.contains("Water => 식수"));
    assert!(block.contains("m/s => (영어 유지)"));
}
