//! Claude Code CLI(`claude -p`) 제공자 — 구독 기반이라 토큰 비용 0으로 취급.
//! 로컬에 claude CLI가 설치·로그인되어 있어야 한다.

use super::{parse_items_text, Provider, TranslatedItem, Usage};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct ClaudeCodeProvider {
    model: String, // "sonnet"/"opus"/"haiku" 또는 전체 모델 ID
}

impl ClaudeCodeProvider {
    pub fn new(model: &str) -> Self {
        ClaudeCodeProvider {
            model: model.to_string(),
        }
    }
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
        match run_claude(&self.model, &format!("{system}\n\n{context}\n\n{payload}")).await {
            Ok(stdout) => parse_output(&stdout),
            Err(e) => (Err(e), Usage::default()),
        }
    }

    async fn complete(&self, system: &str, user: &str) -> (anyhow::Result<String>, Usage) {
        let stdout = match run_claude(&self.model, &format!("{system}\n\n{user}")).await {
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

/// claude CLI 실행 공통부: stdin으로 프롬프트 전달, stdout(JSON) 반환.
async fn run_claude(model: &str, prompt: &str) -> anyhow::Result<String> {
    // Windows에서 claude는 .cmd 셔틀일 수 있어 셸 경유로 실행
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "claude"]);
        c
    } else {
        Command::new("claude")
    };
    cmd.args(["-p", "--output-format", "json", "--model", model])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("claude CLI 실행 실패 (설치·PATH 확인): {e}"))?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin.write_all(prompt.as_bytes()).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    anyhow::ensure!(
        out.status.success(),
        "claude CLI 종료 코드 {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(300)
            .collect::<String>()
    );
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
