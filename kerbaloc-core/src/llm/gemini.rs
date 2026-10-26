use super::{Provider, TranslatedItem, Usage};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    pub model: String,
    api_key: String,
    prices: (f64, f64),
    client: reqwest::Client,
    base_url: String,
    /// generationConfig.thinkingConfig.thinkingLevel (minimal|low|medium|high)
    thinking_level: Option<String>,
}

impl GeminiProvider {
    pub fn new(model: &str, api_key: &str) -> Self {
        Self::with_prices(model, api_key, Self::prices_for(model))
    }

    pub fn with_prices(model: &str, api_key: &str, prices: (f64, f64)) -> Self {
        GeminiProvider {
            model: model.to_string(),
            api_key: api_key.to_string(),
            prices,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("reqwest 클라이언트 생성 실패"),
            base_url: DEFAULT_BASE_URL.to_string(),
            thinking_level: None,
        }
    }

    pub fn with_options(mut self, base_url: Option<&str>, thinking_level: Option<&str>) -> Self {
        if let Some(u) = base_url {
            self.base_url = u.trim_end_matches('/').to_string();
        }
        self.thinking_level = thinking_level.map(String::from);
        self
    }

    fn url(&self) -> String {
        format!("{}/models/{}:generateContent", self.base_url, self.model)
    }

    pub fn prices_for(model: &str) -> (f64, f64) {
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

/// 사고(thinking) 토큰도 출력 단가로 청구되므로 출력에 합산한다.
fn usage_of(v: &Value) -> Usage {
    let n = |p: &str| v.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: n("/usageMetadata/promptTokenCount"),
        output_tokens: n("/usageMetadata/candidatesTokenCount")
            + n("/usageMetadata/thoughtsTokenCount"),
    }
}

/// generationConfig 조립 — thinking_level이 있으면 thinkingConfig 추가.
pub fn generation_config(json_schema: bool, thinking_level: Option<&str>) -> Value {
    let mut gc = json!({"temperature": 0.2});
    if json_schema {
        gc["responseMimeType"] = json!("application/json");
        gc["responseSchema"] = json!({
            "type": "ARRAY",
            "items": {
                "type": "OBJECT",
                "properties": {"i": {"type": "INTEGER"}, "ko": {"type": "STRING"}},
                "required": ["i", "ko"]
            }
        });
    }
    if let Some(level) = thinking_level {
        gc["thinkingConfig"] = json!({"thinkingLevel": level});
    }
    gc
}

/// 200 응답이라도 본문 파싱에 실패하면 토큰은 이미 청구된 상태 —
/// usage는 항상 추출해 실패와 함께 반환한다.
pub fn parse_response(body: &str) -> (anyhow::Result<Vec<TranslatedItem>>, Usage) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (Err(e.into()), Usage::default()),
    };
    let usage = usage_of(&v);
    let items = (|| {
        let text = v
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("응답에 candidates 텍스트 없음: {body:.300}"))?;
        serde_json::from_str::<Vec<TranslatedItem>>(text)
            .map_err(|e| anyhow::anyhow!("모델 출력이 JSON 배열이 아님: {e} — {text:.200}"))
    })();
    (items, usage)
}

#[async_trait::async_trait]
impl Provider for GeminiProvider {
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
        let url = self.url();
        let body = json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": format!("{context}\n\n{payload}")}]}],
            "generationConfig": generation_config(true, self.thinking_level.as_deref()),
        });
        // 429/503 지수 백오프: 0.5s → 2s → 8s (AIMD 적응 동시성은 v1 범위 외)
        let mut delay = Duration::from_millis(500);
        for attempt in 0..4 {
            let resp = match self
                .client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
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
            if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < 3 {
                tokio::time::sleep(delay).await;
                delay *= 4;
                continue;
            }
            return (
                Err(anyhow::anyhow!("Gemini API {status}: {text:.300}")),
                Usage::default(),
            );
        }
        unreachable!()
    }

    async fn complete(&self, system: &str, user: &str) -> (anyhow::Result<String>, Usage) {
        let url = self.url();
        let body = json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": generation_config(false, self.thinking_level.as_deref()),
        });
        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await;
        let text = match resp {
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
        let usage = usage_of(&v);
        let out = v
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("응답에 텍스트 없음: {text:.300}"));
        (out, usage)
    }
}
