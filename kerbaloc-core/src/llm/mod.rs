pub mod anthropic;
pub mod claude_code;
pub mod codex;
pub mod gemini;
pub mod openai_compat;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

/// 번역 요청 항목. prev/fix는 재시도 시 위반 피드백 주입용.
#[derive(Debug, Clone, Serialize)]
pub struct TranslateItem {
    pub i: usize,
    #[serde(rename = "k")]
    pub key: String,
    pub en: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslatedItem {
    pub i: usize,
    pub ko: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
    }

    /// price_in/price_out: 1M 토큰당 USD.
    pub fn cost_usd(&self, price_in: f64, price_out: f64) -> f64 {
        self.input_tokens as f64 / 1e6 * price_in + self.output_tokens as f64 / 1e6 * price_out
    }
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    /// 1M 토큰당 (입력, 출력) USD.
    fn prices(&self) -> (f64, f64);
    /// 실패해도 토큰이 청구될 수 있으므로 (결과, 사용량)을 분리해 반환한다.
    /// 사용량을 알 수 없는 실패(네트워크 오류 등)는 Usage::default().
    async fn translate(
        &self,
        system: &str,
        context: &str,
        payload: &str,
    ) -> (anyhow::Result<Vec<TranslatedItem>>, Usage);

    /// 범용 텍스트 완성 — 용어집 분류 등 번역 외 호출용.
    async fn complete(&self, _system: &str, _user: &str) -> (anyhow::Result<String>, Usage) {
        (
            Err(anyhow::anyhow!("이 제공자는 complete를 지원하지 않습니다")),
            Usage::default(),
        )
    }
}

/// 모델 텍스트 출력에서 JSON 배열 추출 — 코드펜스·설명문 감싸기에 관대.
/// (Gemini의 responseSchema 같은 스키마 강제가 없는 제공자용)
pub fn parse_items_text(text: &str) -> anyhow::Result<Vec<TranslatedItem>> {
    let mut t = text.trim();
    // ```json ... ``` 코드펜스 제거
    if t.starts_with("```") {
        t = t.trim_start_matches("```json").trim_start_matches("```");
        if let Some(end) = t.rfind("```") {
            t = &t[..end];
        }
        t = t.trim();
    }
    if let Ok(v) = serde_json::from_str::<Vec<TranslatedItem>>(t) {
        return Ok(v);
    }
    // 배열 바깥에 설명이 붙은 경우: 첫 '[' ~ 마지막 ']' 구간 재시도
    let (Some(s), Some(e)) = (t.find('['), t.rfind(']')) else {
        anyhow::bail!("모델 출력에 JSON 배열 없음: {t:.200}");
    };
    serde_json::from_str::<Vec<TranslatedItem>>(&t[s..=e])
        .map_err(|err| anyhow::anyhow!("모델 출력이 JSON 배열이 아님: {err} — {t:.200}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Gemini,
    OpenAi,
    Anthropic,
    ClaudeCode,
    Codex,
    Ollama,
    LmStudio,
}

impl std::str::FromStr for ProviderKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Ok(match s {
            "gemini" => ProviderKind::Gemini,
            "openai" => ProviderKind::OpenAi,
            "anthropic" => ProviderKind::Anthropic,
            "claude-code" => ProviderKind::ClaudeCode,
            "codex" => ProviderKind::Codex,
            "ollama" => ProviderKind::Ollama,
            "lmstudio" => ProviderKind::LmStudio,
            _ => anyhow::bail!("알 수 없는 제공자: {s}"),
        })
    }
}

/// 제공자 구성 — 설정/환경변수에서 조립해 팩토리에 넘긴다.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub model: String,
    pub api_key: Option<String>,
    /// API 베이스 URL 오버라이드 — Gemini(…/v1beta)·OpenAI 호환(…/v1)·Anthropic(호스트까지)
    pub base_url: Option<String>,
    /// 추론 강도 — 제공자별 값을 그대로 전달 (Gemini thinkingLevel, OpenAI reasoning_effort,
    /// Anthropic effort, claude --effort, codex model_reasoning_effort). None이면 보내지 않음
    pub reasoning: Option<String>,
    /// CLI 제공자(claude/codex) 실행 파일 — None이면 PATH의 기본 이름
    pub command: Option<String>,
    /// 1M 토큰당 (입력, 출력) USD — None이면 모델별 기본 단가
    pub prices: Option<(f64, f64)>,
}

/// 빈 문자열(공백 포함)은 미설정으로 취급.
fn non_empty(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

pub fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Gemini => "gemini-3.1-flash-lite",
        ProviderKind::OpenAi => "gpt-5.1-mini",
        ProviderKind::Anthropic => "claude-haiku-4-5",
        ProviderKind::ClaudeCode => "sonnet",
        // Codex는 비우면 ~/.codex/config.toml의 기본 모델
        ProviderKind::Codex | ProviderKind::Ollama | ProviderKind::LmStudio => "",
    }
}

/// 모델별 기본 단가 (1M 토큰당 USD). 로컬·구독형은 0.
pub fn default_prices(kind: ProviderKind, model: &str) -> (f64, f64) {
    match kind {
        ProviderKind::Gemini => gemini::GeminiProvider::prices_for(model),
        ProviderKind::OpenAi => {
            if model.contains("nano") {
                (0.05, 0.40)
            } else if model.contains("mini") {
                (0.25, 2.00)
            } else {
                (1.25, 10.00)
            }
        }
        ProviderKind::Anthropic => {
            if model.contains("haiku") {
                (1.00, 5.00)
            } else if model.contains("opus") {
                (5.00, 25.00)
            } else {
                (3.00, 15.00)
            }
        }
        ProviderKind::ClaudeCode
        | ProviderKind::Codex
        | ProviderKind::Ollama
        | ProviderKind::LmStudio => (0.0, 0.0),
    }
}

fn compat_base_url(cfg: &ProviderConfig) -> String {
    non_empty(&cfg.base_url)
        .map(String::from)
        .unwrap_or_else(|| {
            match cfg.kind {
                ProviderKind::OpenAi => "https://api.openai.com/v1",
                ProviderKind::Ollama => "http://localhost:11434/v1",
                ProviderKind::LmStudio => "http://localhost:1234/v1",
                _ => unreachable!("OpenAI 호환 제공자 아님"),
            }
            .to_string()
        })
}

/// 구성에서 Provider 생성.
pub fn create_provider(cfg: &ProviderConfig) -> anyhow::Result<Box<dyn Provider>> {
    let model = cfg.model.trim();
    anyhow::ensure!(
        !model.is_empty() || cfg.kind == ProviderKind::Codex,
        "모델을 지정하세요"
    );
    let prices = cfg
        .prices
        .unwrap_or_else(|| default_prices(cfg.kind, model));
    let api_key = non_empty(&cfg.api_key);
    let base_url = non_empty(&cfg.base_url);
    let reasoning = non_empty(&cfg.reasoning);
    let command = non_empty(&cfg.command);
    Ok(match cfg.kind {
        ProviderKind::Gemini => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("Gemini API 키가 필요합니다"))?;
            Box::new(
                gemini::GeminiProvider::with_prices(model, key, prices)
                    .with_options(base_url, reasoning),
            )
        }
        ProviderKind::OpenAi => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("OpenAI API 키가 필요합니다"))?;
            // GPT-5 계열 추론 모델은 기본값 외 temperature를 거부한다
            Box::new(openai_compat::OpenAiCompatProvider::new(
                &compat_base_url(cfg),
                model,
                Some(key),
                prices,
                None,
                reasoning,
            ))
        }
        ProviderKind::Ollama | ProviderKind::LmStudio => {
            Box::new(openai_compat::OpenAiCompatProvider::new(
                &compat_base_url(cfg),
                model,
                api_key,
                prices,
                Some(0.2),
                reasoning,
            ))
        }
        ProviderKind::Anthropic => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("Anthropic API 키가 필요합니다"))?;
            Box::new(anthropic::AnthropicProvider::new(
                model, key, prices, base_url, reasoning,
            ))
        }
        ProviderKind::ClaudeCode => Box::new(claude_code::ClaudeCodeProvider::new(
            model,
            command.unwrap_or("claude"),
            reasoning,
        )),
        ProviderKind::Codex => Box::new(codex::CodexProvider::new(
            model,
            command.unwrap_or("codex"),
            reasoning,
            prices,
        )),
    })
}

/// CLI 제공자 공통 실행부: stdin으로 프롬프트 전달, stdout 반환.
/// Windows에서 claude/codex는 .cmd 셔틀일 수 있어 .exe가 아니면 셸 경유로 실행한다.
pub(crate) async fn run_cli(
    command: &str,
    args: &[String],
    prompt: &str,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<String> {
    let mut cmd = if cfg!(windows) && !command.to_ascii_lowercase().ends_with(".exe") {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        tokio::process::Command::new(command)
    };
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("{command} CLI 실행 실패 (설치·PATH 확인): {e}"))?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin.write_all(prompt.as_bytes()).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    anyhow::ensure!(
        out.status.success(),
        "{command} CLI 종료 코드 {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(300)
            .collect::<String>()
    );
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 제공자의 사용 가능 모델 목록 조회. Claude Code는 정적 목록.
pub async fn list_models(cfg: &ProviderConfig) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let names = match cfg.kind {
        ProviderKind::ClaudeCode => {
            return Ok(vec![
                "sonnet".into(),
                "opus".into(),
                "haiku".into(),
                "fable".into(),
            ]);
        }
        // 조회 API 없음 — 비우면 config.toml 기본 모델
        ProviderKind::Codex => return Ok(vec![]),
        ProviderKind::Gemini => {
            let key = non_empty(&cfg.api_key)
                .ok_or_else(|| anyhow::anyhow!("Gemini API 키가 필요합니다"))?;
            let base = non_empty(&cfg.base_url).unwrap_or(gemini::DEFAULT_BASE_URL);
            let v: serde_json::Value = client
                .get(format!(
                    "{}/models?pageSize=1000",
                    base.trim_end_matches('/')
                ))
                .header("x-goog-api-key", key)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            v.get("models")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|m| {
                            m.get("supportedGenerationMethods")
                                .and_then(|x| x.as_array())
                                .is_some_and(|a| {
                                    a.iter().any(|g| g.as_str() == Some("generateContent"))
                                })
                        })
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                        .map(|n| n.trim_start_matches("models/").to_string())
                        .collect()
                })
                .unwrap_or_default()
        }
        ProviderKind::Anthropic => {
            let key = non_empty(&cfg.api_key)
                .ok_or_else(|| anyhow::anyhow!("Anthropic API 키가 필요합니다"))?;
            let base = non_empty(&cfg.base_url).unwrap_or(anthropic::DEFAULT_BASE_URL);
            let v: serde_json::Value = client
                .get(format!(
                    "{}/v1/models?limit=100",
                    base.trim_end_matches('/')
                ))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            openai_style_ids(&v)
        }
        ProviderKind::OpenAi | ProviderKind::Ollama | ProviderKind::LmStudio => {
            let mut req = client.get(format!("{}/models", compat_base_url(cfg)));
            if let Some(k) = non_empty(&cfg.api_key) {
                req = req.bearer_auth(k);
            }
            let v: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
            openai_style_ids(&v)
        }
    };
    Ok(names)
}

/// {"data": [{"id": "..."}]} 형태(OpenAI·Anthropic 목록 API)에서 id들 추출.
fn openai_style_ids(v: &serde_json::Value) -> Vec<String> {
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
