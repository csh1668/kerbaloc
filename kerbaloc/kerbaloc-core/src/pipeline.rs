use crate::batching::{plan_batches, Batch};
use crate::glossary::Glossary;
use crate::jobstore::{JobStore, Manifest};
use crate::llm::{Provider, TranslateItem, Usage};
use crate::prompt::{mod_context, payload_json, system_prompt};
use crate::validate::{validate_key_translation, Severity};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct ReviewItem {
    pub key: String,
    pub en: String,
    pub candidates: Vec<String>,
    pub violations: Vec<String>,
}

pub struct TranslationReport {
    pub ok: BTreeMap<String, String>,
    pub review: Vec<ReviewItem>,
    pub failed: BTreeMap<String, String>, // key -> 사유
    pub usage: Usage,
}

/// 매니페스트의 배치 계획을 entries로 복원 (재개 시 재분할 금지).
pub fn batches_from_manifest(m: &Manifest, entries: &BTreeMap<String, String>) -> Vec<Batch> {
    m.batches
        .iter()
        .map(|(id, keys)| Batch {
            id: id.clone(),
            items: keys
                .iter()
                .enumerate()
                .filter_map(|(idx, k)| {
                    entries.get(k).map(|en| TranslateItem {
                        i: idx + 1,
                        key: k.clone(),
                        en: en.clone(),
                        prev: None,
                        fix: None,
                    })
                })
                .collect(),
        })
        .collect()
}

pub fn make_manifest(
    mod_id: &str,
    src_hash: &str,
    model: &str,
    prices: (f64, f64),
    entries: &BTreeMap<String, String>,
) -> Manifest {
    Manifest {
        mod_id: mod_id.into(),
        src_hash: src_hash.into(),
        model: model.into(),
        prices,
        batches: plan_batches(entries)
            .into_iter()
            .map(|b| (b.id, b.items.into_iter().map(|i| i.key).collect()))
            .collect(),
    }
}

/// 검증 결과: Ok(정상) / Err(오류 규칙 목록).
fn check(key: &str, src: &str, dst: &str) -> Result<(), Vec<String>> {
    let errs: Vec<String> = validate_key_translation(key, src, dst)
        .into_iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .map(|f| format!("[{}] {}", f.rule, f.message))
        .collect();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

#[allow(clippy::too_many_arguments)] // 내부 API — 파라미터 구조체는 스튜디오 도입 시 정리
pub async fn translate_job(
    provider: &dyn Provider,
    escalation: &dyn Provider,
    entries: &BTreeMap<String, String>,
    glossary: &Glossary,
    mod_name: &str,
    store: &JobStore,
    manifest: &Manifest,
    concurrency: usize,
    progress: &(dyn Fn(usize, usize, f64) + Sync), // (완료 배치, 전체 배치, 누적 비용)
) -> anyhow::Result<TranslationReport> {
    let (already, mut usage, done_ids) = store.completed()?;
    let batches = batches_from_manifest(manifest, entries);
    let total_batches = batches.len();
    let pending: Vec<&Batch> = batches
        .iter()
        .filter(|b| !done_ids.contains(&b.id))
        .collect();

    // ---- 시도 1: 배치 병렬 ----
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut handles = vec![];
    for b in &pending {
        let permit_sem = sem.clone();
        let texts: Vec<&str> = b.items.iter().map(|i| i.en.as_str()).collect();
        let gloss = Glossary::prompt_block(&glossary.matches(&texts));
        let ctx = mod_context(mod_name, &gloss);
        let items = b.items.clone();
        let id = b.id.clone();
        // provider는 &dyn — spawn 없이 순차/동시 혼합: JoinSet 대신 futures 수집
        handles.push(async move {
            let _p = permit_sem.acquire().await.expect("semaphore");
            let r = provider
                .translate(system_prompt(), &ctx, &payload_json(&items))
                .await;
            (id, items, r)
        });
    }
    let results = futures::future::join_all(handles).await;

    let mut translated: BTreeMap<String, String> = already;
    let mut retry: Vec<(String, String, Option<String>, Vec<String>)> = vec![]; // key, en, prev, violations
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    let mut done_count = done_ids.len();
    let mut error_count = 0usize;

    for (id, items, r) in results {
        match r {
            Err(e) => {
                // 배치 전체 실패 → 키들을 재시도 큐로
                for it in &items {
                    retry.push((
                        it.key.clone(),
                        it.en.clone(),
                        None,
                        vec![format!("API 오류: {e}")],
                    ));
                }
            }
            Ok((out, u)) => {
                usage.add(&u);
                let by_i: BTreeMap<usize, String> = out.into_iter().map(|t| (t.i, t.ko)).collect();
                let mut good: Vec<(String, String)> = vec![];
                for it in &items {
                    match by_i.get(&it.i) {
                        None => retry.push((
                            it.key.clone(),
                            it.en.clone(),
                            None,
                            vec!["응답 누락".into()],
                        )),
                        Some(ko) => match check(&it.key, &it.en, ko) {
                            Ok(()) => good.push((it.key.clone(), ko.clone())),
                            Err(errs) => {
                                error_count += 1;
                                retry.push((it.key.clone(), it.en.clone(), Some(ko.clone()), errs));
                            }
                        },
                    }
                }
                if !good.is_empty() {
                    store.record(&id, &good, &u)?;
                    for (k, v) in good {
                        translated.insert(k, v);
                    }
                }
                done_count += 1;
                progress(
                    done_count,
                    total_batches,
                    usage.cost_usd(manifest.prices.0, manifest.prices.1),
                );
            }
        }
    }

    // 서킷 브레이커: 오류율 30% 초과 && 규모 100키 이상
    let total_keys = entries.len();
    if total_keys >= 100 && error_count * 10 > total_keys * 3 {
        anyhow::bail!(
            "검증 오류율 {}/{} — 모델/키/프롬프트 설정 문제일 수 있어 잡을 중단합니다",
            error_count,
            total_keys
        );
    }

    // ---- 시도 2: 실패 키 ≤8 재묶음 + 피드백 ----
    let mut still: Vec<(String, String, Vec<String>, Vec<String>)> = vec![]; // key, en, candidates, violations
    for chunk in retry.chunks(8) {
        let items: Vec<TranslateItem> = chunk
            .iter()
            .enumerate()
            .map(|(idx, (k, en, prev, errs))| TranslateItem {
                i: idx + 1,
                key: k.clone(),
                en: en.clone(),
                prev: prev.clone(),
                fix: Some(errs.join("; ")),
            })
            .collect();
        let texts: Vec<&str> = items.iter().map(|i| i.en.as_str()).collect();
        let ctx = mod_context(mod_name, &Glossary::prompt_block(&glossary.matches(&texts)));
        match provider
            .translate(system_prompt(), &ctx, &payload_json(&items))
            .await
        {
            Err(e) => {
                for (k, en, prev, _) in chunk {
                    still.push((
                        k.clone(),
                        en.clone(),
                        prev.iter().cloned().collect(),
                        vec![format!("API 오류: {e}")],
                    ));
                }
            }
            Ok((out, u)) => {
                usage.add(&u);
                let by_i: BTreeMap<usize, String> = out.into_iter().map(|t| (t.i, t.ko)).collect();
                let mut good: Vec<(String, String)> = vec![];
                for it in &items {
                    let cand_prev: Vec<String> = it.prev.iter().cloned().collect();
                    match by_i.get(&it.i) {
                        None => still.push((
                            it.key.clone(),
                            it.en.clone(),
                            cand_prev,
                            vec!["응답 누락".into()],
                        )),
                        Some(ko) => match check(&it.key, &it.en, ko) {
                            Ok(()) => good.push((it.key.clone(), ko.clone())),
                            Err(errs) => {
                                let mut c = cand_prev;
                                c.push(ko.clone());
                                still.push((it.key.clone(), it.en.clone(), c, errs));
                            }
                        },
                    }
                }
                if !good.is_empty() {
                    store.record("retry2", &good, &u)?;
                    for (k, v) in good {
                        translated.insert(k, v);
                    }
                }
            }
        }
    }

    // ---- 시도 3: 단건 에스컬레이션 ----
    let mut review: Vec<ReviewItem> = vec![];
    for (key, en, mut candidates, violations) in still {
        let item = TranslateItem {
            i: 1,
            key: key.clone(),
            en: en.clone(),
            prev: candidates.last().cloned(),
            fix: Some(violations.join("; ")),
        };
        let ctx = mod_context(mod_name, &Glossary::prompt_block(&glossary.matches(&[&en])));
        match escalation
            .translate(system_prompt(), &ctx, &payload_json(&[item]))
            .await
        {
            Err(e) => {
                failed.insert(key, format!("에스컬레이션 API 오류: {e}"));
            }
            Ok((out, u)) => {
                usage.add(&u);
                match out.first() {
                    None => {
                        failed.insert(key, "에스컬레이션 응답 누락".into());
                    }
                    Some(t) => match check(&key, &en, &t.ko) {
                        Ok(()) => {
                            store.record("retry3", &[(key.clone(), t.ko.clone())], &u)?;
                            translated.insert(key, t.ko.clone());
                        }
                        Err(errs) => {
                            candidates.push(t.ko.clone());
                            review.push(ReviewItem {
                                key,
                                en,
                                candidates,
                                violations: errs,
                            });
                        }
                    },
                }
            }
        }
    }

    // 불변식: 모든 키 ∈ ok ∪ review ∪ failed
    let mut ok = BTreeMap::new();
    for k in entries.keys() {
        if let Some(v) = translated.get(k) {
            ok.insert(k.clone(), v.clone());
        }
    }
    for k in entries.keys() {
        let covered =
            ok.contains_key(k) || review.iter().any(|r| &r.key == k) || failed.contains_key(k);
        if !covered {
            failed.insert(k.clone(), "분류 누락 (내부 오류)".into());
        }
    }

    Ok(TranslationReport {
        ok,
        review,
        failed,
        usage,
    })
}
