use kerbaloc_core::glossary::Glossary;
use kerbaloc_core::jobstore::JobStore;
use kerbaloc_core::llm::{Provider, TranslatedItem, Usage};
use kerbaloc_core::pipeline::{make_manifest, translate_job};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// (요청 아이템 JSON, 호출 번호) -> ko 규칙 함수. None이면 응답에서 누락.
type Rule = Box<dyn Fn(&serde_json::Value, usize) -> Option<String> + Send + Sync>;

/// 페이로드를 파싱해 규칙 함수로 응답을 만드는 가짜 공급자.
struct FakeProvider {
    name: String,
    rule: Rule,
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
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        let call_no = self.calls.fetch_add(1, Ordering::SeqCst);
        let items: Vec<serde_json::Value> = serde_json::from_str(payload).unwrap();
        let mut out = vec![];
        for it in &items {
            let i = it["i"].as_u64().unwrap() as usize;
            if let Some(ko) = (self.rule)(it, call_no) {
                out.push(TranslatedItem { i, ko });
            }
        }
        (
            Ok(out),
            Usage {
                input_tokens: 100,
                output_tokens: 50,
            },
        )
    }
}

/// 항상 API 오류를 내되 usage는 청구되는 공급자 (200 + 파싱 실패 시나리오).
struct ErrProvider {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for ErrProvider {
    fn name(&self) -> &str {
        "err"
    }
    fn prices(&self) -> (f64, f64) {
        (0.25, 1.50)
    }
    async fn translate(
        &self,
        _system: &str,
        _context: &str,
        _payload: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (
            Err(anyhow::anyhow!("모델 출력이 JSON 배열이 아님")),
            Usage {
                input_tokens: 100,
                output_tokens: 7,
            },
        )
    }
}

fn good_rule() -> Rule {
    Box::new(|it, _| Some(format!("한{}", it["en"].as_str().unwrap())))
}

fn entries(n: usize) -> BTreeMap<String, String> {
    (0..n)
        .map(|i| (format!("#K_{i:03}"), format!("Value {i} <<1>>")))
        .collect()
}

fn glossary() -> Glossary {
    Glossary::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../glossary/core.ko.json"
    )))
    .unwrap()
}

fn run(
    e: &BTreeMap<String, String>,
    p: &dyn Provider,
    dir: &std::path::Path,
    max_retries: u32,
) -> anyhow::Result<kerbaloc_core::pipeline::TranslationReport> {
    let m = make_manifest("TestMod", "v1:sha256:x", p.name(), p.prices(), e);
    let store = JobStore::create(dir, &m)?;
    let g = glossary();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(translate_job(
            p,
            e,
            &g,
            "TestMod",
            &store,
            &m,
            Arc::new(Semaphore::new(4)),
            max_retries,
            &|_, _, _| {},
        ))
}

#[test]
fn all_ok_when_provider_is_good() {
    let e = entries(50);
    let p = FakeProvider {
        name: "fake".into(),
        rule: good_rule(),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 2).unwrap();
    assert_eq!(r.ok.len(), 50);
    assert!(r.review.is_empty() && r.failed.is_empty());
}

#[test]
fn validation_failure_recovers_with_feedback_retry() {
    let e = entries(10);
    // 피드백(prev)이 없으면 토큰 위반, 피드백이 오면 올바르게 번역
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|it, _| {
            if it.get("prev").is_some() {
                Some(format!("한{}", it["en"].as_str().unwrap()))
            } else {
                Some("한글만".into())
            }
        }),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 2).unwrap();
    assert_eq!(r.ok.len(), 10, "재시도에서 전부 회복");
    assert!(r.review.is_empty() && r.failed.is_empty());
}

#[test]
fn exhausted_retries_land_in_review_with_partial_apply() {
    let e = entries(10);
    // 짝수 키만 항상 올바름, 홀수 키는 영원히 토큰 위반 → 부분 적용
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|it, _| {
            let key = it["k"].as_str().unwrap();
            let n: usize = key.trim_start_matches("#K_").parse().unwrap();
            if n % 2 == 0 {
                Some(format!("한{}", it["en"].as_str().unwrap()))
            } else {
                Some("한글만".into())
            }
        }),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 2).unwrap();
    assert_eq!(r.ok.len(), 5, "성공한 키는 부분 적용");
    assert_eq!(r.review.len(), 5, "실패 키만 검수로");
    assert_eq!(r.ok.len() + r.review.len() + r.failed.len(), 10, "불변식");
    assert!(r.review[0].candidates.len() >= 2, "재시도 후보 축적");
}

#[test]
fn missing_response_items_are_retried_not_lost() {
    let e = entries(10);
    // 첫 호출은 짝수 인덱스만 응답, 재시도부터는 전부 응답 (배치 1개라 호출 0 = 초기 시도)
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|it, call_no| {
            let i = it["i"].as_u64().unwrap() as usize;
            if call_no == 0 && i % 2 != 0 {
                None
            } else {
                Some(format!("한{}", it["en"].as_str().unwrap()))
            }
        }),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 2).unwrap();
    assert_eq!(r.ok.len() + r.review.len() + r.failed.len(), 10, "불변식");
    assert_eq!(r.failed.len(), 0);
    assert_eq!(r.ok.len(), 10, "누락분은 재시도에서 회복");
}

#[test]
fn mass_errors_partial_apply_instead_of_abort() {
    // 서킷 브레이커 제거: 대량 검증 실패여도 잡은 성공하고 실패 키만 검수로
    let e = entries(150);
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|_, _| Some("한글만".into())), // 전부 토큰 위반
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 1).unwrap();
    assert_eq!(r.ok.len(), 0);
    assert_eq!(r.review.len(), 150);
}

#[test]
fn failed_requests_still_count_usage() {
    let e = entries(10);
    let p = ErrProvider {
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 2).unwrap();
    assert_eq!(r.ok.len(), 0);
    assert_eq!(r.failed.len(), 10);
    let calls = p.calls.load(Ordering::SeqCst) as u64;
    assert!(calls >= 3, "초기 + 재시도 2회");
    assert_eq!(r.usage.input_tokens, calls * 100, "실패 요청 토큰도 전부 집계");
}

#[test]
fn raw_newline_output_is_normalized_to_literal() {
    let e = entries(3);
    // LLM이 실제 줄바꿈 문자를 반환 → 리터럴 \n으로 정규화되어 통과해야
    let p = FakeProvider {
        name: "fake".into(),
        rule: Box::new(|it, _| Some(format!("한\n{}", it["en"].as_str().unwrap()))),
        calls: AtomicUsize::new(0),
    };
    let d = tempfile::tempdir().unwrap();
    let r = run(&e, &p, d.path(), 0).unwrap();
    assert_eq!(r.ok.len(), 3);
    for v in r.ok.values() {
        assert!(!v.contains('\n'), "실제 줄바꿈 없음");
        assert!(v.contains("\\n"), "리터럴로 변환됨");
    }
}

#[test]
fn poisoned_resume_cache_is_revalidated_and_retranslated() {
    let e = entries(5);
    let m = make_manifest("TestMod", "v1:sha256:x", "fake", (0.25, 1.50), &e);
    let d = tempfile::tempdir().unwrap();
    let store = JobStore::create(d.path(), &m).unwrap();
    // 과거 실행이 남긴 오염 기록: 실제 줄바꿈 문자가 든 값 (당시 규칙은 통과시켰음)
    let poisoned: Vec<(String, String)> = e
        .iter()
        .map(|(k, en)| (k.clone(), format!("한\n{en}")))
        .collect();
    store
        .record(
            &m.batches[0].0,
            &poisoned,
            &Usage { input_tokens: 10, output_tokens: 5 },
        )
        .unwrap();

    let p = FakeProvider {
        name: "fake".into(),
        rule: good_rule(),
        calls: AtomicUsize::new(0),
    };
    let g = glossary();
    let r = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(translate_job(
            &p,
            &e,
            &g,
            "TestMod",
            &store,
            &m,
            Arc::new(Semaphore::new(4)),
            0,
            &|_, _, _| {},
        ))
        .unwrap();
    assert_eq!(r.ok.len(), 5, "오염 키 전부 재번역");
    for v in r.ok.values() {
        assert!(!v.contains('\n'), "재번역 결과에 실제 줄바꿈 없음");
    }
    assert!(
        p.calls.load(Ordering::SeqCst) >= 1,
        "재검증 실패분에 대해 LLM 재호출"
    );
}

#[test]
fn resume_reuses_recorded_batches_and_usage() {
    let e = entries(10);
    let m = make_manifest("TestMod", "v1:sha256:x", "fake", (0.25, 1.50), &e);
    let d = tempfile::tempdir().unwrap();
    let g = glossary();
    let rt = tokio::runtime::Runtime::new().unwrap();

    // 1차 실행: 전부 실패 (usage는 기록됨)
    let p_bad = ErrProvider {
        calls: AtomicUsize::new(0),
    };
    let store = JobStore::create(d.path(), &m).unwrap();
    let r1 = rt
        .block_on(translate_job(
            &p_bad,
            &e,
            &g,
            "TestMod",
            &store,
            &m,
            Arc::new(Semaphore::new(4)),
            0,
            &|_, _, _| {},
        ))
        .unwrap();
    assert_eq!(r1.ok.len(), 0);
    assert!(r1.usage.input_tokens > 0);

    // 2차 실행(재개): 실패했던 키만 다시 시도해 전부 회복, 1차 usage 계승
    let p_good = FakeProvider {
        name: "fake".into(),
        rule: good_rule(),
        calls: AtomicUsize::new(0),
    };
    let (store2, m2) = JobStore::open(d.path()).unwrap();
    let r2 = rt
        .block_on(translate_job(
            &p_good,
            &e,
            &g,
            "TestMod",
            &store2,
            &m2,
            Arc::new(Semaphore::new(4)),
            0,
            &|_, _, _| {},
        ))
        .unwrap();
    assert_eq!(r2.ok.len(), 10, "재개에서 실패 키만 재시도해 회복");
    assert!(
        r2.usage.input_tokens > r1.usage.input_tokens,
        "1차 실행 usage 계승 + 2차 추가"
    );
}
