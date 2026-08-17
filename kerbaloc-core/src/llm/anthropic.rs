//! Anthropic Messages API 제공자.

use super::{parse_items_text, Provider, TranslatedItem, Usage};
use serde_json::{json, Value};
use std::time::Duration;

pub struct AnthropicProvider {
    model: String,
    api_key: String,
    prices: (f64, f64),
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(model: &str, api_key: &str, prices: (f64, f64)) -> Self {
        AnthropicProvider {
            model: model.to_string(),
            api_key: api_key.to_string(),
            prices,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("reqwest 클라이언트 생성 실패"),
        }
    }
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
        let text = v
            .pointer("/content/0/text")
            .and_then(|x| x.as_str())
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
        let body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "temperature": 0.2,
            "system": system,
            "messages": [
                {"role": "user", "content": format!("{context}\n\n{payload}")},
            ],
        });
        let mut delay = Duration::from_millis(500);
        for attempt in 0..4 {
            let resp = match self
                .client
                .post("https://api.anthropic.com/v1/messages")
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
}
