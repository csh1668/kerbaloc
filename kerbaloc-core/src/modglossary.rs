//! 모드별 용어집 (부록 B §5) — 휴리스틱 후보 추출 → LLM 분류 → 사용자 확정.
//! 공식 zh-cn/ja 대조는 구버전 번역 위험 때문에 사용하지 않는다.

use crate::glossary::{Glossary, GlossaryEntry, Policy};
use crate::llm::{Provider, Usage};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModGlossaryEntry {
    pub term: String,
    /// "keep" | "translate" | "translit"
    pub policy: String,
    pub ko: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub count: usize,
    /// 사용자 확정 여부 — 확정 항목만 번역 프롬프트에 주입
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModGlossary {
    pub version: u32,
    pub entries: Vec<ModGlossaryEntry>,
}

impl ModGlossary {
    pub fn load(path: &Path) -> anyhow::Result<ModGlossary> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// 확정된 항목을 프롬프트 매칭용 GlossaryEntry로 변환.
    pub fn confirmed_entries(&self) -> Vec<GlossaryEntry> {
        self.entries
            .iter()
            .filter(|e| e.confirmed)
            .map(|e| GlossaryEntry {
                en: e.term.clone(),
                ko: e.ko.clone(),
                aliases: e.aliases.clone(),
                policy: if e.policy == "keep" {
                    Policy::Keep
                } else {
                    Policy::Translate // translate·translit 모두 ko 사용
                },
                note: e.why.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub term: String,
    pub count: usize,
    pub examples: Vec<String>,
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "you", "your", "are", "was", "will", "can",
    "not", "all", "any", "has", "have", "its", "when", "where", "while", "from", "into", "onto",
    "over", "under", "more", "most", "less", "than", "then", "them", "they", "there", "here",
    "also", "only", "some", "such", "very", "each", "these", "those", "must", "may", "might",
    "should", "would", "could", "does", "did", "done", "being", "been", "but", "however", "which",
    "what", "who", "how", "why", "use", "used", "using", "new", "one", "two", "three", "first",
    "second", "next", "last", "per", "via", "off", "out", "own", "set", "get", "put", "note",
    "warning", "info", "yes", "no", "on", "in", "at", "to", "of", "by", "as", "is", "it", "an",
    "or", "if", "so", "do", "up", "we", "be", "no",
];

const KEY_SUFFIX_NOISE: &[&str] = &[
    "displayname", "display", "name", "desc", "description", "title", "text", "label", "tooltip",
    "msg", "message", "loc", "autoloc", "part", "short", "long", "info", "group", "button",
];

fn clean_value(v: &str) -> String {
    // <<..>>·태그·형식 지정자·숫자 제거
    let re_tokens = Regex::new(r"<<[^>]*>>|</?[A-Za-z][^>]*>|%\.?\d*[sdif]|\{\d+\}|\d+").unwrap();
    re_tokens.replace_all(v, " ").to_string()
}

/// §5.1 휴리스틱 후보 추출 (결정적, LLM 없음). 상위 150개.
pub fn extract_candidates(
    entries: &BTreeMap<String, String>,
    core: &Glossary,
    existing_terms: &[String],
) -> Vec<Candidate> {
    let mut skip: std::collections::HashSet<String> = core
        .terms_lowercase()
        .into_iter()
        .chain(existing_terms.iter().map(|t| t.to_lowercase()))
        .collect();
    for w in STOPWORDS {
        skip.insert((*w).to_string());
    }

    // term(lowercase) -> (표기, 출현 값 수, 가중치, 예시)
    struct Acc {
        display: String,
        count: usize,
        weight: usize,
        examples: Vec<String>,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let bump = |term: &str, weight: usize, example: &str, acc: &mut BTreeMap<String, Acc>| {
        let key = term.to_lowercase();
        if key.len() < 3 || skip.contains(&key) {
            return;
        }
        let e = acc.entry(key).or_insert_with(|| Acc {
            display: term.to_string(),
            count: 0,
            weight,
            examples: vec![],
        });
        e.count += 1;
        e.weight = e.weight.max(weight);
        if e.examples.len() < 3 {
            let ex: String = example.chars().take(120).collect();
            if !e.examples.contains(&ex) {
                e.examples.push(ex);
            }
        }
    };

    let re_camel = Regex::new(r"\b[A-Za-z]*[a-z][A-Z][A-Za-z]*\b").unwrap();
    let re_word = Regex::new(r"[A-Za-z][A-Za-z'-]*").unwrap();
    let threshold = 3.max(entries.len() / 200);

    for (key, value) in entries {
        // 키 이름 조각 — 신뢰도 최상 (가중치 4)
        for seg in key.trim_start_matches("#LOC_").split('_').skip(1) {
            let s = seg.trim();
            if s.len() >= 3
                && s.chars().all(|c| c.is_ascii_alphanumeric())
                && s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && !KEY_SUFFIX_NOISE.contains(&s.to_lowercase().as_str())
            {
                bump(s, 4, value, &mut acc);
            }
        }

        let cleaned = clean_value(value);
        // CamelCase / PascalCase (가중치 3)
        for m in re_camel.find_iter(&cleaned) {
            bump(m.as_str(), 3, value, &mut acc);
        }
        // 문장 단위: 첫 단어 제외한 대문자 시작 단어 + Title Case 구 (≤4단어)
        for sentence in cleaned.split(['.', '!', '?', '\n', ';', ':']) {
            let words: Vec<&str> = re_word.find_iter(sentence).map(|m| m.as_str()).collect();
            let mut phrase: Vec<&str> = vec![];
            for (i, w) in words.iter().enumerate() {
                let capitalized = w.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                if capitalized && i > 0 {
                    phrase.push(w);
                } else {
                    if phrase.len() >= 2 && phrase.len() <= 4 {
                        bump(&phrase.join(" "), 2, value, &mut acc); // Title Case 구
                    } else if phrase.len() == 1 {
                        bump(phrase[0], 1, value, &mut acc); // 문장 중간 대문자 단어
                    }
                    phrase.clear();
                }
            }
            if phrase.len() >= 2 && phrase.len() <= 4 {
                bump(&phrase.join(" "), 2, value, &mut acc);
            } else if phrase.len() == 1 {
                bump(phrase[0], 1, value, &mut acc);
            }
        }
    }

    let mut out: Vec<(usize, Candidate)> = acc
        .into_values()
        // 가중치 1(단순 대문자 단어)은 반복 임계값 이상일 때만
        .filter(|a| a.weight >= 2 || a.count >= threshold)
        .map(|a| {
            (
                a.count * a.weight,
                Candidate {
                    term: a.display,
                    count: a.count,
                    examples: a.examples,
                },
            )
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().take(150).map(|(_, c)| c).collect()
}

const CLASSIFY_SYSTEM: &str = r#"KSP(Kerbal Space Program) 모드의 로컬라이제이션에서 추출한 용어 후보를 분류합니다.
각 후보를 아래 정책 중 하나로 분류하고, 번역이 필요하면 한국어를 제시하십시오.

- keep      : 고유명사·식별자. 영어 유지 (모드명, 파트 코드명, 제품명, 창작 자원명, 인명, 행성/위성명)
- translate : 일반 기술 용어·명사. 모드 전체에서 일관되게 쓸 한국어 번역
- translit  : 음차가 관례인 고유명사 (Kerbal → 커벌)
- noise     : 용어가 아님 (일반 영단어, 추출 오류, 문장 조각)

한국어 게임 번역 관례를 따르십시오. 확신이 없으면 keep을 고르십시오.
출력은 JSON 배열만: [{"term": "...", "policy": "keep|translate|translit|noise", "ko": "..."|null, "confidence": "high|medium|low", "why": "한 줄 근거"}]"#;

#[derive(Deserialize)]
struct Classified {
    term: String,
    policy: String,
    #[serde(default)]
    ko: Option<String>,
    #[serde(default)]
    why: Option<String>,
}

fn parse_classified(text: &str) -> anyhow::Result<Vec<Classified>> {
    let t = text.trim();
    let t = if t.starts_with("```") {
        let t2 = t.trim_start_matches("```json").trim_start_matches("```");
        match t2.rfind("```") {
            Some(end) => &t2[..end],
            None => t2,
        }
    } else {
        t
    };
    let (Some(s), Some(e)) = (t.find('['), t.rfind(']')) else {
        anyhow::bail!("분류 출력에 JSON 배열 없음: {t:.200}");
    };
    Ok(serde_json::from_str(&t[s..=e])?)
}

/// §5.2 LLM 분류 — 후보를 75개씩 나눠 요청, noise는 폐기.
/// 반환 항목은 전부 confirmed=false (사용자 확정 전).
pub async fn classify_candidates(
    provider: &dyn Provider,
    mod_name: &str,
    candidates: &[Candidate],
) -> (anyhow::Result<Vec<ModGlossaryEntry>>, Usage) {
    let mut usage = Usage::default();
    let mut out: Vec<ModGlossaryEntry> = vec![];
    for chunk in candidates.chunks(75) {
        let user = format!(
            "모드 이름: {mod_name}\n\n후보 목록 (각 항목: term, count, examples):\n{}",
            serde_json::to_string_pretty(chunk).unwrap_or_default()
        );
        let (r, u) = provider.complete(CLASSIFY_SYSTEM, &user).await;
        usage.add(&u);
        let text = match r {
            Ok(t) => t,
            Err(e) => return (Err(e), usage),
        };
        let parsed = match parse_classified(&text) {
            Ok(p) => p,
            Err(e) => return (Err(e), usage),
        };
        let counts: BTreeMap<&str, usize> =
            chunk.iter().map(|c| (c.term.as_str(), c.count)).collect();
        for c in parsed {
            if c.policy == "noise" {
                continue;
            }
            if !["keep", "translate", "translit"].contains(&c.policy.as_str()) {
                continue;
            }
            out.push(ModGlossaryEntry {
                count: counts.get(c.term.as_str()).copied().unwrap_or(0),
                term: c.term,
                policy: c.policy,
                ko: c.ko.filter(|k| !k.trim().is_empty()),
                aliases: vec![],
                why: c.why,
                confirmed: false,
            });
        }
    }
    (Ok(out), usage)
}
