use kerbaloc_core::glossary::Glossary;
use kerbaloc_core::llm::{Provider, TranslatedItem, Usage};
use kerbaloc_core::modglossary::{classify_candidates, extract_candidates, ModGlossary};
use std::collections::BTreeMap;

fn entries() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(
        "#LOC_CRP_Karbonite_DisplayName".into(),
        "Karbonite".to_string(),
    );
    m.insert(
        "#LOC_CRP_Karbonite_Desc".into(),
        "A rich vein of Karbonite can power your Drill-O-Matic mining rig.".into(),
    );
    m.insert(
        "#LOC_CRP_Drill_Title".into(),
        "The Drill-O-Matic requires Electric Charge and produces Karbonite.".into(),
    );
    m.insert(
        "#LOC_CRP_Water_Note".into(),
        "Transfer Water to the tank.".into(), // Water는 코어 용어집에 이미 있음 → 제외돼야
    );
    m
}

#[test]
fn extracts_key_fragments_and_camelcase_excluding_core_terms() {
    let core = Glossary::embedded_core();
    let cands = extract_candidates(&entries(), &core, &[]);
    let terms: Vec<&str> = cands.iter().map(|c| c.term.as_str()).collect();
    assert!(terms.iter().any(|t| t.eq_ignore_ascii_case("Karbonite")));
    assert!(
        !terms.iter().any(|t| t.eq_ignore_ascii_case("Water")),
        "코어 용어집 기존 항목은 제외"
    );
    // 예시 문장 동봉
    let k = cands
        .iter()
        .find(|c| c.term.eq_ignore_ascii_case("Karbonite"))
        .unwrap();
    assert!(!k.examples.is_empty());
    assert!(k.count >= 2);
}

#[test]
fn existing_mod_terms_are_excluded() {
    let core = Glossary::embedded_core();
    let cands = extract_candidates(&entries(), &core, &["Karbonite".into()]);
    assert!(!cands.iter().any(|c| c.term.eq_ignore_ascii_case("Karbonite")));
}

/// complete()가 고정 분류 JSON을 돌려주는 가짜 공급자.
struct FakeClassifier;

#[async_trait::async_trait]
impl Provider for FakeClassifier {
    fn name(&self) -> &str {
        "fake"
    }
    fn prices(&self) -> (f64, f64) {
        (1.0, 1.0)
    }
    async fn translate(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        unimplemented!()
    }
    async fn complete(&self, _system: &str, _user: &str) -> (anyhow::Result<String>, Usage) {
        (
            Ok(r#"```json
[
  {"term": "Karbonite", "policy": "keep", "ko": null, "confidence": "high", "why": "창작 자원명"},
  {"term": "mining rig", "policy": "translate", "ko": "채굴 장비", "confidence": "medium", "why": "일반 기술 용어"},
  {"term": "rich vein", "policy": "noise", "ko": null, "confidence": "low", "why": "문장 조각"}
]
```"#
                .into()),
            Usage {
                input_tokens: 40,
                output_tokens: 30,
            },
        )
    }
}

#[test]
fn classify_drops_noise_and_tracks_usage() {
    let core = Glossary::embedded_core();
    let cands = extract_candidates(&entries(), &core, &[]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (r, usage) = rt.block_on(classify_candidates(&FakeClassifier, "CRP", &cands));
    let entries = r.unwrap();
    assert!(entries.iter().any(|e| e.term == "Karbonite" && e.policy == "keep"));
    assert!(entries
        .iter()
        .any(|e| e.term == "mining rig" && e.ko.as_deref() == Some("채굴 장비")));
    assert!(!entries.iter().any(|e| e.policy == "noise"), "noise 폐기");
    assert!(entries.iter().all(|e| !e.confirmed), "확정 전 상태");
    assert!(usage.input_tokens > 0);
}

#[test]
fn save_load_roundtrip_and_confirmed_filter() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("TestMod.ko.json");
    let g = ModGlossary {
        version: 1,
        entries: vec![
            kerbaloc_core::modglossary::ModGlossaryEntry {
                term: "Karbonite".into(),
                policy: "keep".into(),
                ko: None,
                aliases: vec![],
                why: None,
                count: 3,
                confirmed: true,
            },
            kerbaloc_core::modglossary::ModGlossaryEntry {
                term: "mining rig".into(),
                policy: "translate".into(),
                ko: Some("채굴 장비".into()),
                aliases: vec![],
                why: None,
                count: 2,
                confirmed: false, // 미확정 → 주입 제외
            },
        ],
    };
    g.save(&path).unwrap();
    let loaded = ModGlossary::load(&path).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    let confirmed = loaded.confirmed_entries();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].en, "Karbonite");

    // 병합 후 매칭에 실제 반영되는지
    let mut core = Glossary::embedded_core();
    core.extend_entries(confirmed);
    let block = Glossary::prompt_block(&core.matches(&["Deploy the Karbonite drill"]));
    assert!(block.contains("Karbonite => (영어 유지)"));
}
