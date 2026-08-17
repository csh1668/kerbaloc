use kerbaloc_core::llm::TranslateItem;
use kerbaloc_core::prompt::{mod_context, payload_json, system_prompt};

#[test]
fn system_prompt_contains_preservation_rules() {
    let s = system_prompt();
    for needle in ["<<1>>", "^N", "\\n", "｢", "합니다체", "few", "JSON"] {
        // few-shot 섹션 존재를 "예시"로 확인
        let _ = needle;
    }
    assert!(s.contains("<<1>>"));
    assert!(s.contains("^N"));
    assert!(s.contains("｢"));
    assert!(s.contains("합니다체") || s.contains("하십시오체"));
    assert!(s.contains("## 예시"));
    assert!(s.contains("JSON"));
    // 캐시 안정성: 시각/난수 미포함 (단순 호출 2회 동일성)
    assert_eq!(system_prompt(), system_prompt());
}

#[test]
fn payload_roundtrips() {
    let items = vec![
        TranslateItem {
            i: 1,
            key: "#a".into(),
            en: "Hello <<1>>".into(),
            prev: None,
            fix: None,
        },
        TranslateItem {
            i: 2,
            key: "#b".into(),
            en: "Bye".into(),
            prev: Some("안녕".into()),
            fix: Some("<<1>> 누락".into()),
        },
    ];
    let json = payload_json(&items);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v[0]["i"], 1);
    assert_eq!(v[0]["k"], "#a");
    assert!(v[0].get("prev").is_none());
    assert_eq!(v[1]["fix"], "<<1>> 누락");
}

#[test]
fn mod_context_embeds_glossary() {
    let ctx = mod_context("CommunityResourcePack", "Water => 식수\n");
    assert!(ctx.contains("CommunityResourcePack"));
    assert!(ctx.contains("Water => 식수"));
}
