use kerbaloc_core::jobstore::{JobStore, Manifest};
use kerbaloc_core::llm::Usage;

fn manifest() -> Manifest {
    Manifest {
        mod_id: "TestMod".into(),
        src_hash: "v1:sha256:abc".into(),
        model: "gemini-3.1-flash-lite".into(),
        prices: (0.25, 1.50),
        batches: vec![
            ("b0001".into(), vec!["#a".into(), "#b".into()]),
            ("b0002".into(), vec!["#c".into()]),
        ],
    }
}

#[test]
fn create_record_reopen() {
    let d = tempfile::tempdir().unwrap();
    let store = JobStore::create(d.path(), &manifest()).unwrap();
    store
        .record(
            "b0001",
            &[("#a".into(), "가".into()), ("#b".into(), "나".into())],
            &Usage { input_tokens: 100, output_tokens: 50 },
        )
        .unwrap();

    let (store2, m2) = JobStore::open(d.path()).unwrap();
    assert_eq!(m2.mod_id, "TestMod");
    let (done, usage, batch_ids) = store2.completed().unwrap();
    assert_eq!(done.get("#a").unwrap(), "가");
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(batch_ids, vec!["b0001"]);
}

#[test]
fn broken_last_line_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    let store = JobStore::create(d.path(), &manifest()).unwrap();
    store
        .record("b0001", &[("#a".into(), "가".into())], &Usage::default())
        .unwrap();
    // 마지막 줄 손상 시뮬레이션
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(d.path().join("results.jsonl"))
        .unwrap();
    write!(f, "{{\"batch_id\":\"b0002\",\"items\"").unwrap();
    drop(f);
    let (store2, _) = JobStore::open(d.path()).unwrap();
    let (_, _, batch_ids) = store2.completed().unwrap();
    assert_eq!(batch_ids, vec!["b0001"]);
}

#[test]
fn open_missing_dir_fails() {
    let d = tempfile::tempdir().unwrap();
    assert!(JobStore::open(&d.path().join("nope")).is_err());
}
