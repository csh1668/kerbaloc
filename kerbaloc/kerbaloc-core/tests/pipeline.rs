use kerbaloc_core::glossary::Glossary;
use kerbaloc_core::jobstore::JobStore;
use kerbaloc_core::llm::{Provider, TranslatedItem, Usage};
use kerbaloc_core::pipeline::{make_manifest, translate_job};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 페이로드를 파싱해 규칙 함수로 응답을 만드는 가짜 공급자.
struct FakeProvider {
    name: String,
    rule: Box<dyn Fn(usize, &str, &str) -> Option<String> + Send + Sync>, // (i, key, en) -> ko
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn prices(&self) -> (f64, f64) {
        (0.25, 1.50)
    }
    async fn translate(
        &self,
        _system: &str,
        _context: &str,
        payload: &str,
    ) -> anyhow::Result<(Vec<TranslatedItem>, Usage)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let items: Vec<serde_json::Value> = serde_json::from_str(payload)?;
        let mut out = vec![];
        for it in &items {
            let i = it["i"].as_u64().unwrap() as usize;
            let key = it["k"].as_str().unwrap();
            let en = it["en"].as_str().unwrap();
            if let Some(ko) = (self.rule)(i, key, en) {
                out.push(TranslatedItem { i, ko });
            }
        }
        Ok((out, Usage { input_tokens: 100, output_tokens: 50 }))
    }
}

fn good_rule() -> Box<dyn Fn(usize, &str, &str) -> Option<String> + Send + Sync> {
    Box::new(|_, _, en| Some(format!("한{en}")))
}

fn entries(n: usize) -> BTreeMap<String, String> {
    (0..n)
        .map(|i| (format!("#K_{i:03}"), format!("Value {i} <<1>>")))
        .collect()
}

fn glossary() -> Glossary {
    Glossary::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../glossary/core.ko.json"
    )))
    .unwrap()
}

fn run(
    e: &BTreeMap<String, String>,
    p: &FakeProvider,
    esc: &FakeProvider,
    dir: &std::path::Path,
) -> anyhow::Result<kerbaloc_core::pipeline::TranslationReport> {
    let m = make_manifest("TestMod", "v1:sha256:x", p.name(), p.prices(), e);
    let store = JobStore::create(dir, &m)?;
    let g = glossary();
    tokio::runtime::Runtime::new().unwrap().block_on(translate_job(
        p, esc, e, &g, "TestMod", &store, &m, 4, &|_, _, _| {},
    ))
}

#[test]
fn all_ok_when_provider_is_good() {
    let e = entries(50);
    let p = FakeProvider { name: "fake".into(), rule: good_rule(), calls: AtomicUsize::new(0) };
    let esc = FakeProvider { name: "esc".into(), rule: good_rule(), calls: AtomicUsize::new(0) };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, &esc, d.path()).unwrap();
    assert_eq!(r.ok.len(), 50);
    assert!(r.review.is_empty() && r.failed.is_empty());
    assert_eq!(esc.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn token_violation_goes_through_retry_then_escalation() {
    let e = entries(10);
    // 기본 공급자는 항상 토큰을 빠뜨림 → 시도1·2 실패
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|_, _, _| Some("한글만".into())),
        calls: AtomicUsize::new(0),
    };
    // 에스컬레이션은 올바르게 번역
    let esc = FakeProvider { name: "esc".into(), rule: good_rule(), calls: AtomicUsize::new(0) };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, &esc, d.path()).unwrap();
    assert_eq!(r.ok.len(), 10, "에스컬레이션에서 전부 회복");
    assert!(esc.calls.load(Ordering::SeqCst) >= 10);
}

#[test]
fn permanent_fail_lands_in_review_and_invariant_holds() {
    let e = entries(10);
    let bad: Box<dyn Fn(usize, &str, &str) -> Option<String> + Send + Sync> =
        Box::new(|_, _, _| Some("한글만".into()));
    let p = FakeProvider { name: "fake".into(), rule: bad, calls: AtomicUsize::new(0) };
    let esc = FakeProvider {
        name: "esc".into(),
        rule: Box::new(|_, _, _| Some("한글만".into())),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, &esc, d.path()).unwrap();
    assert_eq!(r.ok.len() + r.review.len() + r.failed.len(), 10, "불변식");
    assert_eq!(r.review.len(), 10);
    assert!(r.review[0].candidates.len() >= 2, "후보 축적");
}

#[test]
fn missing_response_items_are_retried_not_lost() {
    let e = entries(10);
    // 짝수 인덱스만 응답 → 나머지는 재시도 경로로
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|i, _, en| if i % 2 == 0 { Some(format!("한{en}")) } else { None }),
        calls: AtomicUsize::new(0),
    };
    let esc = FakeProvider { name: "esc".into(), rule: good_rule(), calls: AtomicUsize::new(0) };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, &esc, d.path()).unwrap();
    assert_eq!(r.ok.len() + r.review.len() + r.failed.len(), 10, "불변식");
    assert_eq!(r.failed.len(), 0);
}

#[test]
fn circuit_breaker_trips_on_mass_errors() {
    let e = entries(150);
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|_, _, _| Some("한글만".into())), // 전부 토큰 위반
        calls: AtomicUsize::new(0),
    };
    let esc = FakeProvider { name: "esc".into(), rule: good_rule(), calls: AtomicUsize::new(0) };
    let d = tempfile::tempdir().unwrap();
    assert!(run(&e, &p, &esc, d.path()).is_err(), "서킷 브레이커 발동");
}
