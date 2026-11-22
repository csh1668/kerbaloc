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
        part_fields: BTreeMap::new(),
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

#[test]
fn literal_parts_use_the_translation_install_and_sharing_pipeline() {
    use kerbaloc_core::{cfg, dbrepo, game, loc, pack, parts, scan, share};
    use std::fs;

    let root = tempfile::tempdir().unwrap();
    let original = "PART\n{\nname = demo-engine\ntitle = Demo engine\ndescription = A small engine.\n}\n";
    let source_path = root.path().join("GameData/Demo/engine.cfg");
    fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    fs::write(&source_path, original).unwrap();
    fs::write(root.path().join("buildID64.txt"), "language = en-us\n").unwrap();
    // Presence stub: no game or ModuleManager process is launched by this test.
    fs::write(root.path().join("GameData/ModuleManager.4.2.3.dll"), []).unwrap();
    let u = scan::scan_gamedata(root.path()).remove(0);
    assert_eq!(u.entries.len(), 2);
    assert_eq!(u.part_fields.len(), 2);
    let fake = Fake {
        translate_calls: AtomicUsize::new(0),
        complete_calls: AtomicUsize::new(0),
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let run = || {
        rt.block_on(translate_mod(
            root.path(), &u, &fake, &opts(false), Arc::new(Semaphore::new(1)),
            &|_, _, _| {},
        )).map_err(|(e, _)| e).unwrap()
    };
    let out = run();
    assert_eq!(out.report.ok.len(), 2);
    let calls = fake.translate_calls.load(Ordering::SeqCst);
    run();
    assert_eq!(fake.translate_calls.load(Ordering::SeqCst), calls);
    assert!(pack::validate_pack(&out.pack_dir, Some(&u.entries)).errors.is_empty());
    let title = parts::key(&u.mod_id, "demo-engine", "title");
    let english = fs::read_to_string(out.pack_dir.join("Localization/en-us.cfg")).unwrap();
    assert_eq!(loc::extract_localization(&cfg::parse(&english).unwrap(), "en-us")[&title], "Demo engine");
    let dest = pack::install_pack(root.path(), &out.pack_dir).unwrap();
    assert!(dest.join("Patches/parts.cfg").is_file());
    game::set_language(root.path(), "ko").unwrap();
    assert_eq!(game::read_language(root.path()).as_deref(), Some("ko"));
    game::set_language(root.path(), "en-us").unwrap();
    assert_eq!(game::read_language(root.path()).as_deref(), Some("en-us"));
    assert!(dest.join("Localization/en-us.cfg").is_file());
    assert_eq!(fs::read_to_string(&source_path).unwrap(), original);
    assert_eq!(scan::scan_gamedata(root.path())[0].source_hash, u.source_hash);

    let payload = share::build_payload(&out.pack_dir, "tester").unwrap();
    let names: Vec<_> = payload["files"].as_array().unwrap().iter()
        .map(|f| f["path"].as_str().unwrap()).collect();
    for required in ["pack.json", "Localization/ko.cfg", "Localization/en-us.cfg", "Patches/parts.cfg"] {
        assert!(names.contains(&required), "share payload lost {required}");
    }
    let db = tempfile::tempdir().unwrap();
    let meta: serde_json::Value = serde_json::from_str(&fs::read_to_string(out.pack_dir.join("pack.json")).unwrap()).unwrap();
    let variant = db.path().join("packs").join(&u.mod_id).join("ko/variants")
        .join(meta["variant_id"].as_str().unwrap());
    for name in &names {
        let target = variant.join(name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(out.pack_dir.join(name), target).unwrap();
    }
    assert!(dbrepo::validate_repo(db.path()).errors.is_empty());
    let index = dbrepo::build_index(db.path()).unwrap();
    assert_eq!(index["packs"][0]["variants"][0]["files"].as_array().unwrap().len(), 4);
    assert!(pack::remove_pack(root.path(), "ko", &u.mod_id).unwrap());
    assert!(!dest.exists());
    assert_eq!(fs::read_to_string(source_path).unwrap(), original);
}
