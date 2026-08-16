use kerbaloc_core::validate::{validate_translation, Severity};

fn errors(src: &str, dst: &str) -> Vec<String> {
    validate_translation(src, dst)
        .into_iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .map(|f| f.rule.to_string())
        .collect()
}

#[test]
fn ok_translation_has_no_errors() {
    assert!(errors("Stage <<1>> ready.\\nGo", "<<1>>단 준비 완료.\\n출발").is_empty());
}

#[test]
fn missing_substitution_token() {
    assert_eq!(
        errors("Stage <<1>> of <<2>>", "<<1>>단"),
        vec!["token-mismatch"]
    );
}

#[test]
fn caret_marker_preserved() {
    assert!(errors("Kerbal^N", "커발^N").is_empty());
    assert_eq!(errors("Kerbal^N", "커발"), vec!["caret-mismatch"]);
}

#[test]
fn raw_brace_is_error() {
    assert!(errors("x", "한{글}").contains(&"raw-brace".to_string()));
}

#[test]
fn escaped_braces_ok_but_must_match() {
    assert!(errors("｢x｣", "｢한글｣").is_empty());
    assert!(errors("｢x｣", "한글").iter().any(|r| r == "token-mismatch"));
}

#[test]
fn literal_newline_count_is_warning() {
    // \n은 장식용 — 게임을 깨지 않으므로 Warning (Dobie 실측 225건 근거)
    let findings = validate_translation("a\\nb", "가나");
    let f = findings
        .iter()
        .find(|f| f.rule == "newline-mismatch")
        .unwrap();
    assert!(matches!(f.severity, Severity::Warning));
    assert!(errors("a\\nb", "가나").is_empty());
}

#[test]
fn lingoona_select_inner_text_is_translatable() {
    // <<1[On/Off]>> → <<1[활성/비활성]>> 은 정상 번역 (구조만 보존)
    assert!(errors("Default <<1[On/Off]>>.", "<<1[활성/비활성]>> 기본.").is_empty());
    // 선택지 개수가 다르면 오류
    assert!(!errors("<<1[a/b/c]>>", "<<1[가/나]>>").is_empty());
}

#[test]
fn comment_in_value_is_error() {
    assert!(errors("see docs", "참고: https://x.y//z").contains(&"comment-in-value".to_string()));
}

#[test]
fn richtext_tags_must_balance() {
    assert!(errors("<b>Hi</b>", "<b>안녕</b>").is_empty());
    assert!(errors("<b>Hi</b>", "<b>안녕")
        .iter()
        .any(|r| r == "richtext-mismatch"));
}

#[test]
fn empty_translation_is_error() {
    assert!(errors("Hi", "  ").contains(&"empty".to_string()));
}

#[test]
fn golden_dobie_dictionary_passes() {
    // 사람이 검증한 번역(Dobie)이 우리 검증기를 통과하지 못하면 검증기가 틀린 것 (부록 B §6)
    use kerbaloc_core::{cfg::parse, loc::extract_localization};
    let stock = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../research/stock-dictionary/en-us.cfg"
    ))
    .unwrap();
    let dobie_path = r"C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program\GameData\Squad\Localization\dictionary.cfg";
    let Ok(dobie) = std::fs::read_to_string(dobie_path) else {
        return; // 게임 미설치 환경은 스킵
    };
    let en = extract_localization(&parse(&stock).unwrap(), "en-us");
    let ko = extract_localization(&parse(&dobie).unwrap(), "en-us"); // Dobie는 en-us 노드에 한국어
                                                                     // 검증기가 잡아낸 Dobie 번역의 실제 결함 (2026-08-16 전수 확인):
                                                                     // 토큰 오타(<<2>]), 토큰 누락/오용, 선택 토큰 닫힘 누락(<<1[자동/수동>>),
                                                                     // richtext 태그 누락, 키 밀림 등. 검증기의 정탐이므로 골든에서 제외한다.
    const KNOWN_DOBIE_DEFECTS: &[&str] = &[
        "#autoLOC_250813",
        "#autoLOC_284439",
        "#autoLOC_307865",
        "#autoLOC_313543",
        "#autoLOC_6001095",
        "#autoLOC_6100032",
        "#autoLOC_7000003",
        "#autoLOC_7001003",
        "#autoLOC_8001031",
        "#autoLOC_8002137",
        "#autoLOC_901065", // <colorred> — '=' 누락 오타
        "#autoLOC_901066",
        "#autoLOC_901067",
    ];
    let mut failures: Vec<String> = vec![];
    for (k, dst) in &ko {
        if KNOWN_DOBIE_DEFECTS.contains(&k.as_str()) {
            continue;
        }
        if let Some(src) = en.get(k) {
            for rule in errors(src, dst) {
                failures.push(format!("{k}: {rule} (src={src:?} dst={dst:?})"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Dobie 번역에서 검증기 오탐 {}건:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
