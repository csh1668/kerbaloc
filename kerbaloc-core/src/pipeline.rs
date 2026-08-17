use crate::batching::{plan_batches_with, Batch, BatchOptions};
use crate::glossary::Glossary;
use crate::jobstore::{JobStore, Manifest};
use crate::llm::{Provider, TranslateItem, Usage};
use crate::prompt::{mod_context, payload_json, system_prompt};
use crate::validate::{validate_key_translation, Severity};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
    make_manifest_with(mod_id, src_hash, model, prices, entries, BatchOptions::default())
}

pub fn make_manifest_with(
    mod_id: &str,
    src_hash: &str,
    model: &str,
    prices: (f64, f64),
    entries: &BTreeMap<String, String>,
    opts: BatchOptions,
) -> Manifest {
    Manifest {
        mod_id: mod_id.into(),
        src_hash: src_hash.into(),
        model: model.into(),
        prices,
        batches: plan_batches_with(entries, opts)
            .into_iter()
            .map(|b| (b.id, b.items.into_iter().map(|i| i.key).collect()))
            .collect(),
    }
}

/// LLM이 리터럴 \n·\t 대신 실제 제어문자를 내는 경우가 있다 — cfg 값은 한 줄이어야
/// 하므로 리터럴로 정규화한다 (의도가 명백한 자동 수리).
fn normalize_value(ko: &str) -> String {
    ko.replace("\r\n", "\\n")
        .replace('\n', "\\n")
        .replace('\r', "\\n")
        .replace('\t', "\\t")
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

enum FailKind {
    Validation,
    Api,
    Missing,
}

struct Unresolved {
    key: String,
    en: String,
    candidates: Vec<String>,
    errs: Vec<String>,
    kind: FailKind,
}

/// 청크(배치) 단위 파이프라인: 각 청크가 독립적으로
/// 번역 → 키별 검증 → 실패 키만 위반 피드백(prev/fix) 주입 재시도(최대 max_retries회).
/// 재시도를 소진한 키만 review/failed로 빠지고 성공 키는 항상 부분 적용된다.
/// sem은 동시 LLM 요청 수 제한 — 여러 모드가 하나의 세마포어를 공유할 수 있다.
#[allow(clippy::too_many_arguments)] // 내부 API — 파라미터 구조체는 스튜디오 도입 시 정리
pub async fn translate_job(
    provider: &dyn Provider,
    entries: &BTreeMap<String, String>,
    glossary: &Glossary,
    mod_name: &str,
    store: &JobStore,
    manifest: &Manifest,
    sem: Arc<Semaphore>,
    max_retries: u32,
    progress: &(dyn Fn(usize, usize, f64) + Sync), // (완료 배치, 전체 배치, 누적 비용)
) -> anyhow::Result<TranslationReport> {
    let (already, base_usage, done_ids) = store.completed()?;
    // 재개 정화: 과거 실행(다른 검증 규칙 시절 포함)이 남긴 기록을 현재 규칙으로
    // 재검증 — 불량 키는 버려서 아래 pending 계산이 자동으로 재번역하게 한다.
    let already: BTreeMap<String, String> = already
        .into_iter()
        .filter(|(k, ko)| {
            entries
                .get(k)
                .is_some_and(|en| check(k, en, ko).is_ok())
        })
        .collect();
    let batches = batches_from_manifest(manifest, entries);
    let total_batches = batches.len();
    // 재개: 미완료 청크 전체 + 완료 청크 중 번역 안 된 키만 재시도
    let pending: Vec<Batch> = batches
        .iter()
        .filter_map(|b| {
            let items: Vec<TranslateItem> = if done_ids.contains(&b.id) {
                b.items
                    .iter()
                    .filter(|i| !already.contains_key(&i.key))
                    .cloned()
                    .collect()
            } else {
                b.items.clone()
            };
            (!items.is_empty()).then(|| Batch {
                id: b.id.clone(),
                items,
            })
        })
        .collect();

    let done_count = AtomicUsize::new(total_batches - pending.len());
    let usage_total = Mutex::new(base_usage);
    let record_lock = Mutex::new(()); // JSONL append 직렬화

    let tasks = pending.into_iter().map(|b| {
        let sem = sem.clone();
        let mut items = b.items;
        let id = b.id;
        let done_count = &done_count;
        let usage_total = &usage_total;
        let record_lock = &record_lock;
        async move {
            let mut good: Vec<(String, String)> = vec![];
            let mut batch_usage = Usage::default();
            let mut cands: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut unresolved: Vec<Unresolved> = vec![];
            for attempt in 0..=max_retries {
                let texts: Vec<&str> = items.iter().map(|i| i.en.as_str()).collect();
                let ctx =
                    mod_context(mod_name, &Glossary::prompt_block(&glossary.matches(&texts)));
                let (r, u) = {
                    let _p = sem.acquire().await.expect("semaphore");
                    provider
                        .translate(system_prompt(), &ctx, &payload_json(&items))
                        .await
                };
                batch_usage.add(&u);
                unresolved = vec![];
                match r {
                    Err(e) => {
                        // 청크 전체 실패 → 같은 아이템 그대로 재시도
                        for it in &items {
                            unresolved.push(Unresolved {
                                key: it.key.clone(),
                                en: it.en.clone(),
                                candidates: cands.get(&it.key).cloned().unwrap_or_default(),
                                errs: vec![format!("API 오류: {e}")],
                                kind: FailKind::Api,
                            });
                        }
                    }
                    Ok(out) => {
                        let by_i: BTreeMap<usize, String> = out
                            .into_iter()
                            .map(|t| (t.i, normalize_value(&t.ko)))
                            .collect();
                        for it in &items {
                            match by_i.get(&it.i) {
                                None => unresolved.push(Unresolved {
                                    key: it.key.clone(),
                                    en: it.en.clone(),
                                    candidates: cands.get(&it.key).cloned().unwrap_or_default(),
                                    errs: vec!["응답 누락".into()],
                                    kind: FailKind::Missing,
                                }),
                                Some(ko) => match check(&it.key, &it.en, ko) {
                                    Ok(()) => good.push((it.key.clone(), ko.clone())),
                                    Err(errs) => {
                                        let c = cands.entry(it.key.clone()).or_default();
                                        c.push(ko.clone());
                                        unresolved.push(Unresolved {
                                            key: it.key.clone(),
                                            en: it.en.clone(),
                                            candidates: c.clone(),
                                            errs,
                                            kind: FailKind::Validation,
                                        });
                                    }
                                },
                            }
                        }
                    }
                }
                if unresolved.is_empty() || attempt == max_retries {
                    break;
                }
                // 다음 시도: 검증 실패 키에는 이전 후보(prev)와 위반 내용(fix)을 피드백으로 주입
                items = unresolved
                    .iter()
                    .enumerate()
                    .map(|(idx, u)| TranslateItem {
                        i: idx + 1,
                        key: u.key.clone(),
                        en: u.en.clone(),
                        prev: u.candidates.last().cloned(),
                        fix: matches!(u.kind, FailKind::Validation)
                            .then(|| u.errs.join("; ")),
                    })
                    .collect();
            }
            // good이 비어도 기록 — 실패 청크의 토큰도 재개 시 비용에 포함
            {
                let _l = record_lock.lock().expect("record lock");
                store.record(&id, &good, &batch_usage)?;
            }
            let cost = {
                let mut g = usage_total.lock().expect("usage lock");
                g.add(&batch_usage);
                g.cost_usd(manifest.prices.0, manifest.prices.1)
            };
            let dc = done_count.fetch_add(1, Ordering::SeqCst) + 1;
            progress(dc, total_batches, cost);
            anyhow::Ok((good, unresolved))
        }
    });
    let results = futures::future::join_all(tasks).await;

    let mut translated: BTreeMap<String, String> = already;
    let mut review: Vec<ReviewItem> = vec![];
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    for r in results {
        let (good, unresolved) = r?;
        for (k, v) in good {
            translated.insert(k, v);
        }
        for u in unresolved {
            match u.kind {
                FailKind::Validation => review.push(ReviewItem {
                    key: u.key,
                    en: u.en,
                    candidates: u.candidates,
                    violations: u.errs,
                }),
                FailKind::Api | FailKind::Missing => {
                    failed.insert(u.key, u.errs.join("; "));
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
        usage: usage_total.into_inner().expect("usage lock"),
    })
}
