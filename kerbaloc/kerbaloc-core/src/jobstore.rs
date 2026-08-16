use crate::llm::Usage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub mod_id: String,
    pub src_hash: String,
    pub model: String,
    pub prices: (f64, f64),
    pub batches: Vec<(String, Vec<String>)>, // (batch_id, keys)
}

#[derive(Serialize, Deserialize)]
struct ResultLine {
    batch_id: String,
    items: Vec<(String, String)>, // (key, ko)
    input_tokens: u64,
    output_tokens: u64,
}

pub struct JobStore {
    dir: PathBuf,
}

impl JobStore {
    pub fn create(dir: &Path, m: &Manifest) -> anyhow::Result<JobStore> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(m)?)?;
        std::fs::write(dir.join("results.jsonl"), "")?;
        Ok(JobStore {
            dir: dir.to_path_buf(),
        })
    }

    pub fn open(dir: &Path) -> anyhow::Result<(JobStore, Manifest)> {
        let m: Manifest =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)?;
        Ok((
            JobStore {
                dir: dir.to_path_buf(),
            },
            m,
        ))
    }

    /// 배치 완료 기록 (JSONL append, 줄 단위 원자성).
    pub fn record(
        &self,
        batch_id: &str,
        items: &[(String, String)],
        usage: &Usage,
    ) -> anyhow::Result<()> {
        let line = ResultLine {
            batch_id: batch_id.to_string(),
            items: items.to_vec(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        };
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(self.dir.join("results.jsonl"))?;
        writeln!(f, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }

    /// 완료된 (key→ko), 누적 사용량, 완료 batch_id 목록. 깨진 마지막 줄은 무시.
    #[allow(clippy::type_complexity)]
    pub fn completed(&self) -> anyhow::Result<(BTreeMap<String, String>, Usage, Vec<String>)> {
        let text = std::fs::read_to_string(self.dir.join("results.jsonl"))?;
        let mut done = BTreeMap::new();
        let mut usage = Usage::default();
        let mut ids = vec![];
        for line in text.lines() {
            let Ok(r) = serde_json::from_str::<ResultLine>(line) else {
                continue; // 손상 줄(중단 시 마지막 줄) 무시
            };
            for (k, ko) in r.items {
                done.insert(k, ko);
            }
            usage.add(&Usage {
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
            });
            ids.push(r.batch_id);
        }
        Ok((done, usage, ids))
    }
}
