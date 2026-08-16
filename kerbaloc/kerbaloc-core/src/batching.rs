use crate::llm::TranslateItem;
use std::collections::BTreeMap;

pub struct Batch {
    pub id: String,
    pub items: Vec<TranslateItem>,
}

const MAX_KEYS: usize = 40;
const MAX_PAYLOAD_TOKENS: usize = 2000;
const LONG_VALUE_CHARS: usize = 400;
const LONG_BATCH_KEYS: usize = 3;

fn est_tokens(item_key: &str, item_en: &str) -> usize {
    (item_en.len() + item_key.len()) / 4 + 8 // 4자=1tok 근사 + JSON 오버헤드
}

/// 키 정렬(BTreeMap → 접두 응집) 후 이중 상한으로 절단. 긴 값은 별도 소배치.
pub fn plan_batches(entries: &BTreeMap<String, String>) -> Vec<Batch> {
    let mut short: Vec<(&String, &String)> = vec![];
    let mut long: Vec<(&String, &String)> = vec![];
    for (k, v) in entries {
        if v.len() > LONG_VALUE_CHARS {
            long.push((k, v));
        } else {
            short.push((k, v));
        }
    }

    let mut batches: Vec<Vec<TranslateItem>> = vec![];
    let mut cur: Vec<TranslateItem> = vec![];
    let mut cur_tok = 0usize;
    for (k, v) in short {
        let t = est_tokens(k, v);
        if !cur.is_empty() && (cur.len() >= MAX_KEYS || cur_tok + t > MAX_PAYLOAD_TOKENS) {
            batches.push(std::mem::take(&mut cur));
            cur_tok = 0;
        }
        cur.push(TranslateItem {
            i: cur.len() + 1,
            key: k.clone(),
            en: v.clone(),
            prev: None,
            fix: None,
        });
        cur_tok += t;
    }
    if !cur.is_empty() {
        batches.push(cur);
    }
    for chunk in long.chunks(LONG_BATCH_KEYS) {
        batches.push(
            chunk
                .iter()
                .enumerate()
                .map(|(idx, (k, v))| TranslateItem {
                    i: idx + 1,
                    key: (*k).clone(),
                    en: (*v).clone(),
                    prev: None,
                    fix: None,
                })
                .collect(),
        );
    }

    batches
        .into_iter()
        .enumerate()
        .map(|(n, items)| Batch {
            id: format!("b{:04}", n + 1),
            items,
        })
        .collect()
}
