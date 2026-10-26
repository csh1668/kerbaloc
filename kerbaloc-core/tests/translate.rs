use kerbaloc_core::batching::BatchOptions;
use kerbaloc_core::llm::{Provider, TranslatedItem, Usage};
use kerbaloc_core::scan::{IdSource, ModUnit, VersionInfo};
use kerbaloc_core::translate::{method_slug, translate_mod, TranslateOptions};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// 번역은 "한"+원문, 용어집 분류는 고정 JSON을 돌려주는 가짜 공급자.
struct Fake {
    translate_calls: AtomicUsize,
    complete_calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for Fake {
    fn name(&self) -> &str {
        "gemini-3.1-flash-lite"
    }
    fn prices(&self) -> (f64, f64) {
        (1.0, 1.0)
    }
    async fn translate(
        &self,
        _system: &str,
        _context: &str,
        payload: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        self.translate_calls.fetch_add(1, Ordering::SeqCst);
        let items: Vec<serde_json::Value> = serde_json::from_str(payload).unwrap();
        let out = items
            .iter()
            .map(|it| TranslatedItem {
                i: it["i"].as_u64().unwrap() as usize,
                ko: format!("한{}", it["en"].as_str().unwrap()),
            })
            .collect();
        (
            Ok(out),
            Usage {
                input_tokens: 10,
                output_tokens: 10,
            },
        )
    }
    async fn complete(&self, _system: &str, _user: &str) -> (anyhow::Result<String>, Usage) {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        (
            Ok(r#"[{"term":"Karbonite","policy":"keep","ko":null,"why":"자원명"}]"#.into()),
            Usage {
                input_tokens: 5,
                output_tokens: 5,
            },
        )
    }
}

fn unit() -> ModUnit {
    let mut entries = BTreeMap::new();
    entries.insert(
        "#LOC_CRP_Karbonite_Title".to_string(),
        "Karbonite".to_string(),
    );
    entries.insert(
        "#LOC_CRP_Karbonite_Desc".to_string(),
        "Karbonite powers your Drill-O-Matic rig.".to_string(),
    );
    ModUnit {
        mod_id: "CRP".into(),
        id_source: IdSource::Folder,
        display_name: "Community Resource Pack".into(),
        version: VersionInfo {
            raw: None,
            source: "unknown",
        },
        files: vec![],
        source_hash: kerbaloc_core::hash::source_hash(&entries),
        keys_hash: kerbaloc_core::hash::keys_hash(&entries),
        official_langs: vec![],
        entries,
    }
}

fn opts(fresh: bool) -> TranslateOptions<'static> {
    TranslateOptions {
        nick: "tester",
        batch: BatchOptions::default(),
        max_retries: 2,
        fresh,
    }
}

#[test]
fn translate_mod_generates_glossary_builds_pack_and_resumes() {
    let root = tempfile::tempdir().unwrap();
    let fake = Fake {
        translate_calls: AtomicUsize::new(0),
        complete_calls: AtomicUsize::new(0),
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let run = |fresh: bool| {
        rt.block_on(translate_mod(
            root.path(),
            &unit(),
            &fake,
            &opts(fresh),
            Arc::new(Semaphore::new(4)),
            &|_, _, _| {},
        ))
        .map_err(|(e, _)| e)
        .unwrap()
    };

    let out = run(false);
    assert_eq!(out.report.ok.len(), 2);
    assert!(out.glossary_note.as_deref().unwrap().contains("자동 생성"));
    assert!(root
        .path()
        .join("KerbaLoc-glossaries/CRP.ko.json")
        .is_file());
    assert!(out.pack_dir.join("pack.json").is_file());
    assert!(
        out.pack_dir
            .to_string_lossy()
            .contains("gemini31flashlite-tester"),
        "변형 ID = 날짜-모델슬러그-닉네임"
    );
    assert!(out.cost > 0.0);
    let (t1, c1) = (
        fake.translate_calls.load(Ordering::SeqCst),
        fake.complete_calls.load(Ordering::SeqCst),
    );
    assert_eq!(c1, 1, "용어집 분류 1회");

    // 재실행: 용어집 파일이 있으니 재생성 없음, 같은 소스·모델 잡은 재개 → 번역 호출 없음
    let out = run(false);
    assert!(out.glossary_note.is_none());
    assert_eq!(out.report.ok.len(), 2);
    assert_eq!(fake.complete_calls.load(Ordering::SeqCst), c1);
    assert_eq!(
        fake.translate_calls.load(Ordering::SeqCst),
        t1,
        "재개 시 재번역 없음"
    );

    // fresh: 잡을 무시하고 다시 번역
    run(true);
    assert!(fake.translate_calls.load(Ordering::SeqCst) > t1);
}

#[test]
fn blacklisted_mod_is_refused_without_cost() {
    let root = tempfile::tempdir().unwrap();
    let mut u = unit();
    u.entries = BTreeMap::from([("#LOC_xSci_Title".to_string(), "Science".to_string())]);
    let fake = Fake {
        translate_calls: AtomicUsize::new(0),
        complete_calls: AtomicUsize::new(0),
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let r = rt.block_on(translate_mod(
        root.path(),
        &u,
        &fake,
        &opts(false),
        Arc::new(Semaphore::new(1)),
        &|_, _, _| {},
    ));
    let (e, cost) = r.err().unwrap();
    assert!(e.to_string().contains("번역 차단"));
    assert_eq!(cost, 0.0);
    assert_eq!(fake.complete_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn method_slug_normalizes_model_names() {
    assert_eq!(method_slug("gemini-3.1-flash-lite"), "gemini31flashlite");
    assert_eq!(method_slug(""), "custom");
    assert_eq!(method_slug("GPT-6.1-Sol"), "gpt61sol");
}
