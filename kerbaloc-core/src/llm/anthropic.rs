//! Anthropic Messages API 제공자.

use super::{parse_items_text, Provider, TranslatedItem, Usage};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

pub struct AnthropicProvider {
    model: String,
    api_key: String,
    prices: (f64, f64),
    client: reqwest::Client,
    base_url: String,
    /// output_config.effort (low|medium|high|xhigh|max) — None이면 모델 기본값
    effort: Option<String>,
}

impl AnthropicProvider {
    pub fn new(
        model: &str,
        api_key: &str,
        prices: (f64, f64),
        base_url: Option<&str>,
        effort: Option<&str>,
    ) -> Self {
        AnthropicProvider {
            model: model.to_string(),
            api_key: api_key.to_string(),
            prices,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(300)) // 사고 포함 응답은 길어질 수 있음
                .build()
                .expect("reqwest 클라이언트 생성 실패"),
            base_url: base_url
                .unwrap_or(DEFAULT_BASE_URL)
                .trim_end_matches('/')
                .to_string(),
            effort: effort.map(String::from),
        }
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

/// /v1/messages 요청 본문. temperature는 보내지 않는다 — 현행 모델(Opus 4.7+·5.x 등)은
/// 기본값 외 샘플링 파라미터를 400으로 거부한다. max_tokens는 사고 토큰 포함 상한.
pub fn messages_body(model: &str, system: &str, user: &str, effort: Option<&str>) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    });
    if let Some(e) = effort {
        body["output_config"] = json!({"effort": e});
    }
    body
}

/// content 배열의 첫 text 블록 — 사고가 켜진 모델은 thinking 블록이 먼저 온다.
fn first_text(v: &Value) -> Option<&str> {
    v.get("content")?
        .as_array()?
        .iter()
        .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))?
        .get("text")?
        .as_str()
}

/// /v1/messages 응답에서 (번역 항목, 사용량) 추출. usage는 파싱 실패해도 집계.
pub fn parse_response(body: &str) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
    let v: Value = match serde_json::from_str(body) {
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
        let text = first_text(&v)
            .ok_or_else(|| anyhow::anyhow!("응답에 content 텍스트 없음: {body:.300}"))?;
        parse_items_text(text)
    })();
    (items, usage)
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.model
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
        let body = messages_body(
            &self.model,
            system,
            &format!("{context}\n\n{payload}"),
            self.effort.as_deref(),
        );
        let mut delay = Duration::from_millis(500);
        for attempt in 0..4 {
            let resp = match self
                .client
                .post(self.messages_url())
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return (Err(e.into()), Usage::default()),
            };
            let status = resp.status();
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => return (Err(e.into()), Usage::default()),
            };
            if status.is_success() {
                return parse_response(&text);
            }
            if (status.as_u16() == 429 || status.as_u16() == 529) && attempt < 3 {
                tokio::time::sleep(delay).await;
                delay *= 4;
                continue;
            }
            return (
                Err(anyhow::anyhow!("Anthropic API {status}: {text:.300}")),
                Usage::default(),
            );
        }
        unreachable!()
    }

    async fn complete(&self, system: &str, user: &str) -> (anyhow::Result<String>, Usage) {
        let body = messages_body(&self.model, system, user, self.effort.as_deref());
        let text = match self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => match r.text().await {
                Ok(t) => t,
                Err(e) => return (Err(e.into()), Usage::default()),
            },
            Err(e) => return (Err(e.into()), Usage::default()),
        };
        let v: Value = match serde_json::from_str(&text) {
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
        let out = first_text(&v)
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("응답에 텍스트 없음: {text:.300}"));
        (out, usage)
    }
}
