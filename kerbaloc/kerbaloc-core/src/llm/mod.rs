pub mod gemini;

use serde::{Deserialize, Serialize};

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
    async fn translate(
        &self,
        system: &str,
        context: &str,
        payload: &str,
    ) -> anyhow::Result<(Vec<TranslatedItem>, Usage)>;
}
