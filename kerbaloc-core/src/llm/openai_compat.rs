//! OpenAI 호환 chat/completions 제공자 — OpenAI·Ollama·LMStudio 공용.

use super::{parse_items_text, Provider, TranslatedItem, Usage};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OpenAiCompatProvider {
    base_url: String, // …/v1 까지
    model: String,
    api_key: Option<String>,
    prices: (f64, f64),
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(base_url: &str, model: &str, api_key: Option<&str>, prices: (f64, f64)) -> Self {
        OpenAiCompatProvider {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.map(String::from),
            prices,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(300)) // 로컬 모델은 느릴 수 있음
                .build()
                .expect("reqwest 클라이언트 생성 실패"),
        }
    }
}

/// chat/completions 응답에서 (번역 항목, 사용량) 추출. usage는 파싱 실패해도 집계.
pub fn parse_response(body: &str) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (Err(e.into()), Usage::default()),
    };
    let usage = Usage {
        input_tokens: v
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: v
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    let items = (|| {
        let text = v
            .pointer("/choices/0/message/content")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("응답에 choices 텍스트 없음: {body:.300}"))?;
        parse_items_text(text)
    })();
    (items, usage)
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatProvider {
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
            "temperature": 0.2,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": format!("{context}\n\n{payload}")},
            ],
        });
        let mut delay = Duration::from_millis(500);
        for attempt in 0..4 {
            let mut req = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&body);
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            let resp = match req.send().await {
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
            if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < 3 {
                tokio::time::sleep(delay).await;
                delay *= 4;
                continue;
            }
            return (
                Err(anyhow::anyhow!("API {status}: {text:.300}")),
                Usage::default(),
            );
        }
        unreachable!()
    }
}
