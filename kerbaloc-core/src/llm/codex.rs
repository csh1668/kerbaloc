//! Codex CLI(`codex exec`) 제공자 — 구독 기반이라 기본 단가 0 (프로필에서 지정 가능).
//! 로컬에 codex CLI가 설치·로그인되어 있어야 한다.

use super::{parse_items_text, run_cli, Provider, TranslatedItem, Usage};
use serde_json::Value;

pub struct CodexProvider {
    /// 비우면 ~/.codex/config.toml의 기본 모델
    model: String,
    command: String,
    /// model_reasoning_effort (minimal|low|medium|high|xhigh) — None이면 config 기본값
    effort: Option<String>,
    prices: (f64, f64),
}

impl CodexProvider {
    pub fn new(model: &str, command: &str, effort: Option<&str>, prices: (f64, f64)) -> Self {
        CodexProvider {
            model: model.to_string(),
            command: command.to_string(),
            effort: effort.map(String::from),
            prices,
        }
    }

    async fn run(&self, prompt: &str) -> (anyhow::Result<String>, Usage) {
        // 에이전트가 사용자 폴더를 훑지 않도록 빈 작업 디렉터리에서 실행
        let cwd = std::env::temp_dir().join("kerbaloc-codex");
        if let Err(e) = std::fs::create_dir_all(&cwd) {
            return (Err(e.into()), Usage::default());
        }
        let args = exec_args(&self.model, self.effort.as_deref());
        match run_cli(&self.command, &args, prompt, Some(&cwd)).await {
            Ok(stdout) => parse_jsonl(&stdout),
            Err(e) => (Err(e), Usage::default()),
        }
    }
}

/// `codex exec` 인자 — 프롬프트는 stdin(`-`), 출력은 JSONL 이벤트.
/// 값은 따옴표 없이 넘긴다: TOML로 파싱되지 않으면 문자열 그대로 쓰이고,
/// Windows cmd 경유 시 따옴표 이스케이프 문제를 피한다.
pub fn exec_args(model: &str, effort: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--ephemeral",
        "-s",
        "read-only",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if !model.is_empty() {
        args.extend(["-m".to_string(), model.to_string()]);
    }
    if let Some(e) = effort {
        args.extend(["-c".to_string(), format!("model_reasoning_effort={e}")]);
    }
    args.push("-".to_string());
    args
}

/// `codex debug models` 카탈로그(JSON)에서 선택 가능한 모델 slug를 priority 순으로.
/// visibility가 "list"가 아닌 내부용 모델은 뺀다.
pub fn parse_models(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else {
        return vec![];
    };
    let mut models: Vec<(i64, String)> = v
        .get("models")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
                .filter_map(|m| {
                    let slug = m.get("slug")?.as_str()?.to_string();
                    let prio = m
                        .get("priority")
                        .and_then(Value::as_i64)
                        .unwrap_or(i64::MAX);
                    Some((prio, slug))
                })
                .collect()
        })
        .unwrap_or_default();
    models.sort_by_key(|(p, _)| *p);
    models.into_iter().map(|(_, s)| s).collect()
}

/// JSONL 이벤트에서 (마지막 agent_message 텍스트, 사용량) 추출.
/// item 타입 "error"는 설정 경고 등 비치명적 알림이라 무시한다. turn.failed는 실패,
/// 최상위 error 이벤트(재연결 알림일 수 있음)는 응답이 끝내 없을 때만 실패 사유로 쓴다.
pub fn parse_jsonl(stdout: &str) -> (anyhow::Result<String>, Usage) {
    let mut usage = Usage::default();
    let mut text: Option<String> = None;
    let mut failed: Option<String> = None;
    let mut error: Option<String> = None;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("item.completed") => {
                let item = &v["item"];
                if item.get("type").and_then(Value::as_str) == Some("agent_message") {
                    text = item.get("text").and_then(Value::as_str).map(String::from);
                }
            }
            Some("turn.completed") => {
                let n = |p: &str| v.pointer(p).and_then(Value::as_u64).unwrap_or(0);
                usage.add(&Usage {
                    input_tokens: n("/usage/input_tokens"),
                    output_tokens: n("/usage/output_tokens"),
                });
            }
            Some("turn.failed") => {
                failed = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or(Some("turn.failed".into()));
            }
            Some("error") => {
                error = v.get("message").and_then(Value::as_str).map(String::from);
            }
            _ => {}
        }
    }
    let out = match (failed, text, error) {
        (Some(e), _, _) => Err(anyhow::anyhow!("codex 오류: {e}")),
        (None, Some(t), _) => Ok(t),
        (None, None, Some(e)) => Err(anyhow::anyhow!("codex 오류: {e}")),
        (None, None, None) => Err(anyhow::anyhow!("codex 출력에 응답 없음: {stdout:.300}")),
    };
    (out, usage)
}

#[async_trait::async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &str {
        if self.model.is_empty() {
            "codex"
        } else {
            &self.model
        }
    }

    fn prices(&self) -> (f64, f64) {
        self.prices
    }

    async fn translate(
        &self,
        system: &str,
        context: &str,
        payload: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        let (r, usage) = self
            .run(&format!("{system}\n\n{context}\n\n{payload}"))
            .await;
        (r.and_then(|t| parse_items_text(&t)), usage)
    }

    async fn complete(&self, system: &str, user: &str) -> (anyhow::Result<String>, Usage) {
        self.run(&format!("{system}\n\n{user}")).await
    }
}
