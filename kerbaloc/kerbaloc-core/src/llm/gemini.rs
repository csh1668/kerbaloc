use super::{Provider, TranslatedItem, Usage};
use serde_json::{json, Value};
use std::time::Duration;

pub struct GeminiProvider {
    pub model: String,
    api_key: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(model: &str, api_key: &str) -> Self {
        GeminiProvider {
            model: model.to_string(),
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("reqwest 클라이언트 생성 실패"),
        }
    }

    fn prices_for(model: &str) -> (f64, f64) {
        // per 1M tokens (부록 B §1.1)
        if model.contains("flash-lite") {
            (0.25, 1.50)
        } else if model.contains("flash") {
            (0.50, 3.00)
        } else {
            (2.00, 12.00)
        }
    }
}

pub fn parse_response(body: &str) -> anyhow::Result<(Vec<TranslatedItem>, Usage)> {
    let v: Value = serde_json::from_str(body)?;
    let text = v
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("응답에 candidates 텍스트 없음: {body:.300}"))?;
    let items: Vec<TranslatedItem> = serde_json::from_str(text)
        .map_err(|e| anyhow::anyhow!("모델 출력이 JSON 배열이 아님: {e} — {text:.200}"))?;
    let usage = Usage {
        input_tokens: v
            .pointer("/usageMetadata/promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: v
            .pointer("/usageMetadata/candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    Ok((items, usage))
}

#[async_trait::async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        &self.model
    }

    fn prices(&self) -> (f64, f64) {
        Self::prices_for(&self.model)
    }

    async fn translate(
        &self,
        system: &str,
        context: &str,
        payload: &str,
    ) -> anyhow::Result<(Vec<TranslatedItem>, Usage)> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let body = json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": format!("{context}\n\n{payload}")}]}],
            "generationConfig": {
                "temperature": 0.2,
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "ARRAY",
                    "items": {
                        "type": "OBJECT",
                        "properties": {"i": {"type": "INTEGER"}, "ko": {"type": "STRING"}},
                        "required": ["i", "ko"]
                    }
                }
            }
        });
        // 429/503 지수 백오프: 0.5s → 2s → 8s (AIMD 적응 동시성은 v1 범위 외)
        let mut delay = Duration::from_millis(500);
        for attempt in 0..4 {
            let resp = self
                .client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if status.is_success() {
                return parse_response(&text);
            }
            if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < 3 {
                tokio::time::sleep(delay).await;
                delay *= 4;
                continue;
            }
            anyhow::bail!("Gemini API {status}: {text:.300}");
        }
        unreachable!()
    }
}
