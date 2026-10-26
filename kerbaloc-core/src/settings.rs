//! 앱·CLI 공용 설정 (SSOT) — LLM 프로필, 배치 옵션, KSP 경로, 닉네임.
//! 저장 위치는 Tauri 앱 설정 디렉터리와 같다: `<config_dir>/dev.kerbaloc.studio/settings.json`
//! (Tauri 2의 app_config_dir = dirs::config_dir()/identifier).

use crate::batching::BatchOptions;
use crate::llm::{default_model, model_choices, ProviderConfig, ProviderKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// tauri.conf.json의 identifier와 같아야 한다.
pub const APP_ID: &str = "dev.kerbaloc.studio";

pub fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
        .join("settings.json")
}

/// LLM 프로필 — 제공자와 세부 옵션 묶음. 설정에 여러 개 두고 하나를 활성으로 쓴다.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LlmProfile {
    pub id: String,
    pub name: String,
    /// gemini | openai | anthropic | claude-code | codex | ollama | lmstudio
    pub provider: String,
    /// 모델 ID — 비우면 실행 시 [`model_choices`] 규칙으로 결정 (기본 모델 → 목록 최상단)
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// API 베이스 URL 오버라이드
    pub base_url: Option<String>,
    /// 추론 강도 — 값은 제공자별 (Gemini thinkingLevel, OpenAI reasoning_effort, Claude effort …)
    pub reasoning: Option<String>,
    /// CLI 실행 파일 (claude/codex) — 비우면 PATH
    pub command: Option<String>,
    /// 1M 토큰당 USD — 비우면 모델별 기본 단가
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
}

impl LlmProfile {
    /// 저장된 값 그대로의 구성. 모델이 비어 있으면 제공자 기본 모델을 넣는다
    /// (목록 조회까지 하는 해석은 [`Settings::resolved_provider_config`]).
    pub fn provider_config(&self) -> anyhow::Result<ProviderConfig> {
        let kind: ProviderKind = self.provider.parse()?;
        let model = self
            .model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| default_model(kind).to_string());
        let prices = match (self.price_in, self.price_out) {
            (Some(i), Some(o)) => Some((i, o)),
            _ => None,
        };
        Ok(ProviderConfig {
            kind,
            model,
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            reasoning: self.reasoning.clone(),
            command: self.command.clone(),
            prices,
        })
    }

    fn model_unset(&self) -> bool {
        self.model.as_deref().is_none_or(|m| m.trim().is_empty())
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Settings {
    pub ksp_root: Option<String>,
    pub nick: Option<String>,
    #[serde(default)]
    pub profiles: Vec<LlmProfile>,
    /// 번역에 쓸 프로필 id — 없거나 못 찾으면 첫 프로필
    pub active_profile: Option<String>,
    /// 청크당 최대 키 수 (기본 40)
    pub max_keys: Option<u32>,
    /// 청크당 최대 페이로드 토큰 (기본 2000)
    pub max_payload_tokens: Option<u32>,
    /// 청크 검증 실패 시 피드백 재시도 최대횟수 (기본 2)
    pub max_retries: Option<u32>,
    /// 일괄 번역 동시 LLM 요청 수 (기본 20)
    pub workers: Option<u32>,
    // ── 구버전 단일 제공자 설정: 로드 시 프로필로 이전하고 다시 저장하지 않는다 ──
    #[serde(default, skip_serializing)]
    provider: Option<String>,
    #[serde(default, skip_serializing)]
    model: Option<String>,
    #[serde(default, skip_serializing)]
    gemini_api_key: Option<String>,
    #[serde(default, skip_serializing)]
    openai_api_key: Option<String>,
    #[serde(default, skip_serializing)]
    anthropic_api_key: Option<String>,
    #[serde(default, skip_serializing)]
    ollama_url: Option<String>,
    #[serde(default, skip_serializing)]
    lmstudio_url: Option<String>,
    #[serde(default, skip_serializing)]
    price_in: Option<f64>,
    #[serde(default, skip_serializing)]
    price_out: Option<f64>,
}

impl Settings {
    /// 기본 경로에서 로드 (없거나 깨졌으면 기본값) + 구버전 이전.
    pub fn load() -> Settings {
        Self::load_from(&settings_path())
    }

    pub fn load_from(path: &Path) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.migrate_legacy();
        s
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&settings_path())
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// 구버전 설정을 프로필로 이전 — 키·URL이 저장된 제공자마다 프로필을 만들고,
    /// 모델·단가는 당시 선택돼 있던 제공자 프로필에만 옮긴다. 프로필이 이미 있으면 그대로.
    pub fn migrate_legacy(&mut self) {
        if !self.profiles.is_empty() {
            return;
        }
        let active = self
            .provider
            .clone()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "gemini".into());
        let legacy = [
            ("gemini", "Gemini", self.gemini_api_key.clone(), None),
            ("openai", "OpenAI", self.openai_api_key.clone(), None),
            (
                "anthropic",
                "Claude API",
                self.anthropic_api_key.clone(),
                None,
            ),
            ("claude-code", "Claude Code", None, None),
            ("ollama", "Ollama", None, self.ollama_url.clone()),
            ("lmstudio", "LM Studio", None, self.lmstudio_url.clone()),
        ];
        let set = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
        for (id, name, api_key, base_url) in legacy {
            let is_active = id == active;
            if !is_active && !set(&api_key) && !set(&base_url) {
                continue;
            }
            self.profiles.push(LlmProfile {
                id: id.into(),
                name: name.into(),
                provider: id.into(),
                model: if is_active { self.model.clone() } else { None },
                api_key,
                base_url,
                price_in: if is_active { self.price_in } else { None },
                price_out: if is_active { self.price_out } else { None },
                ..Default::default()
            });
        }
        if self.profiles.is_empty() {
            self.profiles.push(LlmProfile {
                id: "gemini".into(),
                name: "Gemini".into(),
                provider: "gemini".into(),
                ..Default::default()
            });
        }
        let active_exists = self.profiles.iter().any(|p| p.id == active);
        self.active_profile = Some(if active_exists {
            active
        } else {
            self.profiles[0].id.clone()
        });
    }

    pub fn active(&self) -> Option<&LlmProfile> {
        self.profiles
            .iter()
            .find(|p| Some(&p.id) == self.active_profile.as_ref())
            .or_else(|| self.profiles.first())
    }

    /// id 우선, 없으면 이름으로 찾는다.
    pub fn find_profile(&self, key: &str) -> Option<&LlmProfile> {
        self.profiles
            .iter()
            .find(|p| p.id == key)
            .or_else(|| self.profiles.iter().find(|p| p.name == key))
    }

    /// `profile`(id·이름) 또는 활성 프로필.
    pub fn profile_or_active(&self, profile: Option<&str>) -> anyhow::Result<&LlmProfile> {
        match profile {
            Some(key) => self
                .find_profile(key)
                .ok_or_else(|| anyhow::anyhow!("프로필 '{key}'이(가) 없습니다")),
            None => self
                .active()
                .ok_or_else(|| anyhow::anyhow!("LLM 프로필이 없습니다. 설정에서 추가하세요.")),
        }
    }

    /// 번역에 쓸 최종 구성. 모델이 비어 있으면 목록을 조회해 기본 모델 → 최상단 순으로 고른다
    /// (앱 설정 화면의 모델 선택과 같은 규칙).
    pub async fn resolved_provider_config(
        &self,
        profile: Option<&str>,
    ) -> anyhow::Result<ProviderConfig> {
        let p = self.profile_or_active(profile)?;
        let mut cfg = p.provider_config()?;
        if p.model_unset() {
            if let Some(m) = model_choices(&cfg, None).await.selected {
                cfg.model = m;
            }
        }
        Ok(cfg)
    }

    pub fn batch_options(&self) -> BatchOptions {
        let d = BatchOptions::default();
        BatchOptions {
            max_keys: self.max_keys.map(|v| v as usize).unwrap_or(d.max_keys),
            max_payload_tokens: self
                .max_payload_tokens
                .map(|v| v as usize)
                .unwrap_or(d.max_payload_tokens),
        }
    }

    pub fn workers(&self) -> usize {
        self.workers.unwrap_or(20).max(1) as usize
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries.unwrap_or(2)
    }

    pub fn nick(&self) -> String {
        self.nick
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "anon".into())
    }

    /// 설정의 KSP 경로가 유효하면 그것, 아니면 None (호출 측이 자동 감지).
    pub fn ksp_root(&self) -> Option<PathBuf> {
        let p = PathBuf::from(self.ksp_root.as_deref()?.trim());
        p.join("buildID64.txt").is_file().then_some(p)
    }
}
