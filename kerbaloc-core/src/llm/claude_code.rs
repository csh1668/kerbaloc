//! Claude Code CLI(`claude -p`) 제공자 — 구독 기반이라 토큰 비용 0으로 취급.
//! 로컬에 claude CLI가 설치·로그인되어 있어야 한다.

use super::{parse_items_text, run_cli, Provider, TranslatedItem, Usage};
use serde_json::Value;

pub struct ClaudeCodeProvider {
    model: String, // "sonnet"/"opus"/"haiku"/"fable" 또는 전체 모델 ID
    command: String,
    /// --effort (low|medium|high|xhigh|max) — None이면 CLI 기본값
    effort: Option<String>,
}

impl ClaudeCodeProvider {
    pub fn new(model: &str, command: &str, effort: Option<&str>) -> Self {
        ClaudeCodeProvider {
            model: model.to_string(),
            command: command.to_string(),
            effort: effort.map(String::from),
        }
    }

    async fn run(&self, prompt: &str) -> anyhow::Result<String> {
        run_cli(
            &self.command,
            &cli_args(&self.model, self.effort.as_deref()),
            prompt,
            None,
        )
        .await
    }
}

/// `claude -p` 인자 — 프롬프트는 stdin.
pub fn cli_args(model: &str, effort: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--output-format", "json", "--model", model]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(e) = effort {
        args.extend(["--effort".to_string(), e.to_string()]);
    }
    args
}

/// `claude -p --output-format json` stdout에서 (번역 항목, 사용량) 추출.
pub fn parse_output(stdout: &str) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
    let v: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => return (Err(e.into()), Usage::default()),
    };
    let usage = Usage {
        input_tokens: v
            .pointer("/usage/input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: v
            .pointer("/usage/output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    let items = (|| {
        if v.get("is_error").and_then(Value::as_bool) == Some(true) {
            anyhow::bail!(
                "claude CLI 오류: {}",
                v.get("result").and_then(Value::as_str).unwrap_or("?")
            );
        }
        let text = v
            .get("result")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("claude 출력에 result 없음: {stdout:.300}"))?;
        parse_items_text(text)
    })();
    (items, usage)
}

#[async_trait::async_trait]
impl Provider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        &self.model
    }

    fn prices(&self) -> (f64, f64) {
        (0.0, 0.0) // 구독 — API 과금 없음
    }

    async fn translate(
        &self,
        system: &str,
        context: &str,
        payload: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
        match self
            .run(&format!("{system}\n\n{context}\n\n{payload}"))
            .await
        {
            Ok(stdout) => parse_output(&stdout),
            Err(e) => (Err(e), Usage::default()),
        }
    }

    async fn complete(&self, system: &str, user: &str) -> (anyhow::Result<String>, Usage) {
        let stdout = match self.run(&format!("{system}\n\n{user}")).await {
            Ok(s) => s,
            Err(e) => return (Err(e), Usage::default()),
        };
        let v: Value = match serde_json::from_str(stdout.trim()) {
            Ok(v) => v,
            Err(e) => return (Err(e.into()), Usage::default()),
        };
        let usage = Usage {
            input_tokens: v
                .pointer("/usage/input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: v
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        };
        let out = if v.get("is_error").and_then(Value::as_bool) == Some(true) {
            Err(anyhow::anyhow!(
                "claude CLI 오류: {}",
                v.get("result").and_then(Value::as_str).unwrap_or("?")
            ))
        } else {
            v.get("result")
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("claude 출력에 result 없음"))
        };
        (out, usage)
    }
}
