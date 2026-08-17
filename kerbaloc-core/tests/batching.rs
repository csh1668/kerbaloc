use kerbaloc_core::batching::plan_batches;
use std::collections::{BTreeMap, BTreeSet};

fn entries(n: usize, val: &str) -> BTreeMap<String, String> {
    (0..n)
        .map(|i| (format!("#LOC_T_{i:04}"), val.to_string()))
        .collect()
}

#[test]
fn covers_all_keys_exactly_once() {
    let e = entries(137, "Hello world <<1>>");
    let batches = plan_batches(&e);
    let mut seen = BTreeSet::new();
    for b in &batches {
        for it in &b.items {
            assert!(seen.insert(it.key.clone()), "중복 키 {}", it.key);
        }
    }
    assert_eq!(seen.len(), e.len());
}

#[test]
fn respects_dual_caps() {
    let e = entries(500, "short");
    for b in plan_batches(&e) {
        assert!(b.items.len() <= 40, "키 수 상한 위반: {}", b.items.len());
        let tok: usize = b
            .items
            .iter()
            .map(|i| (i.en.len() + i.key.len()) / 4 + 8)
            .sum();
        assert!(tok <= 2000, "토큰 상한 위반: {tok}");
    }
}

#[test]
fn long_values_go_to_small_batches() {
    let mut e = entries(10, "short");
    e.insert("#LOC_LONG_1".into(), "x".repeat(500));
    e.insert("#LOC_LONG_2".into(), "y".repeat(600));
    e.insert("#LOC_LONG_3".into(), "z".repeat(700));
    e.insert("#LOC_LONG_4".into(), "w".repeat(800));
    let batches = plan_batches(&e);
    for b in &batches {
        let has_long = b.items.iter().any(|i| i.en.len() > 400);
        if has_long {
            assert!(b.items.len() <= 3, "긴 값 배치가 {}키", b.items.len());
            assert!(
                b.items.iter().all(|i| i.en.len() > 400),
                "긴 값과 짧은 값 혼합"
            );
        }
    }
}

#[test]
fn batch_ids_are_sequential() {
    let e = entries(90, "hello");
    let ids: Vec<String> = plan_batches(&e).into_iter().map(|b| b.id).collect();
    assert_eq!(ids[0], "b0001");
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
}
