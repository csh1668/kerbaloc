use regex::RegexBuilder;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    Translate,
    Keep,
}

#[derive(Debug, Deserialize)]
pub struct GlossaryEntry {
    pub en: String,
    pub ko: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub policy: Policy,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
struct GlossaryFile {
    schema: String,
    entries: Vec<GlossaryEntry>,
}

pub struct Glossary(Vec<GlossaryEntry>);

const MAX_MATCHES: usize = 60;

impl Glossary {
    pub fn load(path: &Path) -> anyhow::Result<Glossary> {
        Self::load_str(&std::fs::read_to_string(path)?)
    }

    pub fn load_str(text: &str) -> anyhow::Result<Glossary> {
        let f: GlossaryFile = serde_json::from_str(text)?;
        anyhow::ensure!(
            f.schema == "kerbaloc/glossary@1",
            "알 수 없는 용어집 스키마: {}",
            f.schema
        );
        Ok(Glossary(f.entries))
    }

    /// 바이너리에 내장된 코어 용어집 — 배포된 exe에서 리포 경로 의존 제거.
    pub fn embedded_core() -> Glossary {
        Self::load_str(include_str!("../../glossary/core.ko.json")).expect("내장 용어집 파싱")
    }

    /// 모든 용어·별칭 소문자 목록 (모드 용어집 후보 추출 시 중복 제외용).
    pub fn terms_lowercase(&self) -> Vec<String> {
        self.0
            .iter()
            .flat_map(|e| std::iter::once(&e.en).chain(e.aliases.iter()))
            .map(|t| t.to_lowercase())
            .collect()
    }

    /// 모드 용어집 등 추가 항목 병합.
    pub fn extend_entries(&mut self, extra: Vec<GlossaryEntry>) {
        self.0.extend(extra);
    }

    /// 대소문자 무시 + 단어 경계 매칭. 상한 60개.
    pub fn matches(&self, texts: &[&str]) -> Vec<&GlossaryEntry> {
        let joined = texts.join("\n");
        let mut out = vec![];
        for e in &self.0 {
            let mut terms: Vec<&str> = vec![e.en.as_str()];
            terms.extend(e.aliases.iter().map(String::as_str));
            let hit = terms.iter().any(|t| {
                let pat = format!(r"(?i)\b{}\b", regex::escape(t));
                RegexBuilder::new(&pat)
                    .build()
                    .map(|re| re.is_match(&joined))
                    .unwrap_or(false)
            });
            if hit {
                out.push(e);
                if out.len() >= MAX_MATCHES {
                    break;
                }
            }
        }
        out
    }

    /// "delta-v => 델타-V" / "m/s => (영어 유지)" 형식의 프롬프트 블록.
    pub fn prompt_block(entries: &[&GlossaryEntry]) -> String {
        let mut out = String::new();
        for e in entries {
            match (&e.policy, &e.ko) {
                (Policy::Keep, _) | (Policy::Translate, None) => {
                    out.push_str(&format!("{} => (영어 유지)\n", e.en));
                }
                (Policy::Translate, Some(ko)) => {
                    out.push_str(&format!("{} => {ko}\n", e.en));
                }
            }
        }
        out
    }
}
