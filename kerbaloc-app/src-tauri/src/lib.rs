//! KerbaLoc 스튜디오 — kerbaloc-core를 Tauri 커맨드로 노출.

use kerbaloc_core::{
    dbclient, game, glossary::Glossary, jobstore::JobStore, modglossary, pack, packgen, pipeline,
    scan, share,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// GameData 스캔은 수 초 걸리므로 결과를 캐시한다. refresh(force)로만 무효화.
#[derive(Default)]
struct ScanCache(Mutex<Option<Vec<scan::ModUnit>>>);

fn cached_units(app: &AppHandle, force: bool) -> Result<Vec<scan::ModUnit>, String> {
    let root = resolve_root(app)?;
    let state: State<ScanCache> = app.state();
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if force || guard.is_none() {
        *guard = Some(scan::scan_gamedata(&root));
    }
    Ok(guard.as_ref().unwrap().clone())
}

fn cached_unit(app: &AppHandle, mod_id: &str) -> Result<scan::ModUnit, String> {
    cached_units(app, false)?
        .into_iter()
        .find(|u| u.mod_id == mod_id)
        .ok_or_else(|| format!("설치본에서 {mod_id}를 찾지 못했습니다"))
}

/// LLM 프로필 — 제공자와 세부 옵션 묶음. 설정에 여러 개 두고 하나를 활성으로 쓴다.
#[derive(Serialize, Deserialize, Default, Clone)]
struct LlmProfile {
    id: String,
    name: String,
    /// gemini | openai | anthropic | claude-code | codex | ollama | lmstudio
    provider: String,
    /// 모델 ID — 비우면 제공자별 기본 모델 (Codex는 config.toml 기본값)
    model: Option<String>,
    api_key: Option<String>,
    /// API 베이스 URL 오버라이드
    base_url: Option<String>,
    /// 추론 강도 — 값은 제공자별 (Gemini thinkingLevel, OpenAI reasoning_effort, Claude effort …)
    reasoning: Option<String>,
    /// CLI 실행 파일 (claude/codex) — 비우면 PATH
    command: Option<String>,
    /// 1M 토큰당 USD — 비우면 모델별 기본 단가
    price_in: Option<f64>,
    price_out: Option<f64>,
}

impl LlmProfile {
    fn provider_config(&self) -> Result<kerbaloc_core::llm::ProviderConfig, String> {
        use kerbaloc_core::llm::{default_model, ProviderConfig, ProviderKind};
        let kind: ProviderKind = self
            .provider
            .parse()
            .map_err(|e: anyhow::Error| e.to_string())?;
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
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Settings {
    ksp_root: Option<String>,
    nick: Option<String>,
    #[serde(default)]
    profiles: Vec<LlmProfile>,
    /// 번역에 쓸 프로필 id — 없거나 못 찾으면 첫 프로필
    active_profile: Option<String>,
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
    /// 청크당 최대 키 수 (기본 40)
    max_keys: Option<u32>,
    /// 청크당 최대 페이로드 토큰 (기본 2000)
    max_payload_tokens: Option<u32>,
    /// 청크 검증 실패 시 피드백 재시도 최대횟수 (기본 2)
    max_retries: Option<u32>,
    /// 일괄 번역 동시 LLM 요청 수 (기본 20)
    workers: Option<u32>,
}

impl Settings {
    /// 구버전 설정을 프로필로 이전 — 키·URL이 저장된 제공자마다 프로필을 만들고,
    /// 모델·단가는 당시 선택돼 있던 제공자 프로필에만 옮긴다. 프로필이 이미 있으면 그대로.
    fn migrate_legacy(&mut self) {
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

    fn active(&self) -> Option<&LlmProfile> {
        self.profiles
            .iter()
            .find(|p| Some(&p.id) == self.active_profile.as_ref())
            .or_else(|| self.profiles.first())
    }

    fn provider_config(&self) -> Result<kerbaloc_core::llm::ProviderConfig, String> {
        self.active()
            .ok_or("LLM 프로필이 없습니다. 설정에서 추가하세요.")?
            .provider_config()
    }

    fn batch_options(&self) -> kerbaloc_core::batching::BatchOptions {
        let d = kerbaloc_core::batching::BatchOptions::default();
        kerbaloc_core::batching::BatchOptions {
            max_keys: self.max_keys.map(|v| v as usize).unwrap_or(d.max_keys),
            max_payload_tokens: self
                .max_payload_tokens
                .map(|v| v as usize)
                .unwrap_or(d.max_payload_tokens),
        }
    }
}

/// 변형 ID의 method 세그먼트용: 모델명 → 소문자 영숫자만.
fn sanitize_method(model: &str) -> String {
    let s: String = model
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if s.is_empty() { "custom".into() } else { s.chars().take(30).collect() }
}

fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("config dir");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

fn load_settings_inner(app: &AppHandle) -> Settings {
    let mut s: Settings = std::fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    s.migrate_legacy();
    s
}

fn resolve_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(r) = load_settings_inner(app).ksp_root {
        let p = PathBuf::from(r);
        if p.join("buildID64.txt").is_file() {
            return Ok(p);
        }
    }
    let roots = game::detect_ksp_roots();
    roots
        .into_iter()
        .next()
        .ok_or_else(|| "KSP 설치를 찾지 못했습니다. 설정에서 경로를 지정하세요.".to_string())
}

#[derive(Serialize)]
struct UnitInfo {
    mod_id: String,
    display_name: String,
    version: Option<String>,
    keys: usize,
    source_hash: String,
    installed: bool,
    /// 번역 차단 사유 (블랙리스트 모드면 Some)
    blacklisted: Option<String>,
}

#[tauri::command]
fn game_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let root = resolve_root(&app)?;
    Ok(serde_json::json!({
        "root": root.to_string_lossy(),
        "language": game::read_language(&root),
    }))
}

#[tauri::command]
fn set_language(app: AppHandle, lang: String) -> Result<(), String> {
    let root = resolve_root(&app)?;
    game::set_language(&root, &lang).map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_units(app: AppHandle, force: bool) -> Result<Vec<UnitInfo>, String> {
    let root = resolve_root(&app)?;
    let installed_dir = root.join("GameData").join("KerbaLoc").join("ko");
    Ok(cached_units(&app, force)?
        .into_iter()
        .map(|u| UnitInfo {
            installed: installed_dir.join(&u.mod_id).is_dir(),
            blacklisted: kerbaloc_core::blacklist::check(&u.entries)
                .map(|b| b.reason.clone()),
            mod_id: u.mod_id,
            display_name: u.display_name,
            version: u.version.raw,
            keys: u.entries.len(),
            source_hash: u.source_hash,
        })
        .collect())
}

#[tauri::command]
async fn db_index() -> Result<serde_json::Value, String> {
    let m = dbclient::fetch_manifest()
        .await
        .map_err(|e| e.to_string())?;
    let i = dbclient::fetch_index(&m).await.map_err(|e| e.to_string())?;
    serde_json::to_value(serde_json::json!({
        "commit": m.commit,
        "packs": i.packs.iter().map(|p| serde_json::json!({
            "modId": p.mod_id,
            "variants": p.variants.iter().map(|v| serde_json::json!({
                "variantId": v.variant_id,
                "srcSha256": v.src_sha256,
                "keysTranslated": v.keys_translated,
                "keysTarget": v.keys_target,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    }))
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn install_from_db(
    app: AppHandle,
    mod_id: String,
    variant: Option<String>,
) -> Result<String, String> {
    let root = resolve_root(&app)?;
    let m = dbclient::fetch_manifest()
        .await
        .map_err(|e| e.to_string())?;
    let i = dbclient::fetch_index(&m).await.map_err(|e| e.to_string())?;
    let p = i
        .packs
        .iter()
        .find(|p| p.mod_id == mod_id)
        .ok_or_else(|| format!("DB에 {mod_id} 없음"))?;
    let v = match &variant {
        Some(id) => p
            .variants
            .iter()
            .find(|v| &v.variant_id == id)
            .ok_or("변형 없음")?,
        None => p.variants.last().ok_or("변형 없음")?,
    };
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    dbclient::download_variant(&m, v, tmp.path())
        .await
        .map_err(|e| e.to_string())?;
    // 모드 버전(소스 해시)이 같을 때만 원문 대조 검증 — 다르면 구조 검증만
    // (버전 불일치 팩도 경고 후 설치 가능해야 하므로 토큰 대조로 거부하지 않는다)
    let src = cached_unit(&app, &mod_id)
        .ok()
        .filter(|u| u.source_hash == v.src_sha256)
        .map(|u| u.entries);
    let r = pack::validate_pack(tmp.path(), src.as_ref());
    if !r.errors.is_empty() {
        return Err(format!("팩 검증 실패:\n{}", r.errors.join("\n")));
    }
    let dest = pack::install_pack(&root, tmp.path()).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
fn remove_pack(app: AppHandle, mod_id: String) -> Result<bool, String> {
    let root = resolve_root(&app)?;
    pack::remove_pack(&root, "ko", &mod_id).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
struct TranslateProgress {
    mod_id: String,
    done: usize,
    total: usize,
    cost: f64,
}

#[derive(Serialize)]
struct TranslateResult {
    mod_id: String,
    ok: usize,
    review: Vec<serde_json::Value>,
    failed: usize,
    cost: f64,
    pack_dir: String,
    /// 이번 번역에서 용어집을 자동 생성했으면 그 결과(성공·실패)
    glossary_note: Option<String>,
}

/// 실패 시에도 그때까지 지출한 비용을 함께 반환한다: Err((사유, 비용)).
async fn translate_one(
    app: &AppHandle,
    mod_id: &str,
    sem: std::sync::Arc<tokio::sync::Semaphore>,
    max_retries: u32,
) -> Result<TranslateResult, (String, f64)> {
    use kerbaloc_core::llm::create_provider;
    let fail0 = |e: String| (e, 0.0);
    let root = resolve_root(app).map_err(fail0)?;
    let settings = load_settings_inner(app);
    let nick = settings.nick.clone().unwrap_or_else(|| "anon".into());
    let cfg = settings.provider_config().map_err(fail0)?;
    let provider = create_provider(&cfg).map_err(|e| fail0(e.to_string()))?;
    let provider = provider.as_ref();

    let unit = cached_unit(app, mod_id).map_err(fail0)?;
    if let Some(b) = kerbaloc_core::blacklist::check(&unit.entries) {
        return Err(fail0(format!("번역 차단된 모드({}): {}", b.name, b.reason)));
    }

    // 모드 용어집: 없으면 초안을 자동 생성(미확정 저장)한다. 실패해도 번역은 코어 용어집만으로 계속.
    let gpath = mod_glossary_path(&root, mod_id);
    let mut glossary_cost = 0.0;
    let mut glossary_note = None;
    if !gpath.exists() {
        let _permit = sem.acquire().await.map_err(|e| fail0(e.to_string()))?;
        let (r, usage) =
            modglossary::generate_draft(provider, &gpath, &unit.display_name, &unit.entries).await;
        glossary_cost = usage.cost_usd(provider.prices().0, provider.prices().1);
        glossary_note = Some(match r {
            Ok(g) => format!("용어집 {}개 자동 생성", g.entries.len()),
            Err(e) => format!("용어집 자동 생성 실패: {e}"),
        });
    }
    let fail_g = |e: String| (e, glossary_cost);

    let mut glossary = Glossary::embedded_core();
    // 모드 용어집: 초안 포함 전 항목을 병합해 프롬프트에 주입
    if let Ok(mg) = modglossary::ModGlossary::load(&gpath) {
        glossary.extend_entries(mg.prompt_entries());
    }

    // 재개: 같은 소스·모델의 잡이 있으면 이어서 (성공 청크 재번역 방지)
    let job_dir = root.join("KerbaLoc-jobs").join(&unit.mod_id);
    let (store, manifest) = match JobStore::open(&job_dir) {
        Ok((s, m)) if m.src_hash == unit.source_hash && m.model == provider.name() => (s, m),
        _ => {
            let m = pipeline::make_manifest_with(
                &unit.mod_id,
                &unit.source_hash,
                provider.name(),
                provider.prices(),
                &unit.entries,
                settings.batch_options(),
            );
            (
                JobStore::create(&job_dir, &m).map_err(|e| fail_g(e.to_string()))?,
                m,
            )
        }
    };

    let app2 = app.clone();
    let mod_id2 = mod_id.to_string();
    let report = pipeline::translate_job(
        provider,
        &unit.entries,
        &glossary,
        &unit.display_name,
        &store,
        &manifest,
        sem,
        max_retries,
        &move |done, total, cost| {
            let _ = app2.emit(
                "translate-progress",
                TranslateProgress { mod_id: mod_id2.clone(), done, total, cost },
            );
        },
    )
    .await
    .map_err(|e| fail_g(e.to_string()))?;

    let cost = report
        .usage
        .cost_usd(provider.prices().0, provider.prices().1)
        + glossary_cost;

    // 모델명은 provider.name() — Codex 기본 모델(빈 값)은 "codex"로 기록
    let variant = packgen::make_variant_id(&sanitize_method(provider.name()), &nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(&pack_dir, &unit, &report.ok, &variant, provider.name())
        .map_err(|e| (e.to_string(), cost))?;

    Ok(TranslateResult {
        mod_id: mod_id.to_string(),
        ok: report.ok.len(),
        review: report
            .review
            .iter()
            .map(|r| {
                serde_json::json!({
                    "key": r.key, "en": r.en, "candidates": r.candidates, "violations": r.violations
                })
            })
            .collect(),
        failed: report.failed.len(),
        cost,
        pack_dir: pack_dir.to_string_lossy().to_string(),
        glossary_note,
    })
}

#[derive(Serialize, Clone)]
struct BatchModDone {
    mod_id: String,
    ok: usize,
    review: usize,
    failed: usize,
    cost: f64,
    error: Option<String>,
    installed: bool,
    pack_dir: Option<String>,
    glossary_note: Option<String>,
}

/// 여러 모드를 병렬 번역 — 모든 모드의 청크 요청이 워커 세마포어(설정 workers, 기본 20)를
/// 공유한다. 각 모드 완료마다 이벤트 발행, 성공한 팩은 즉시 게임에 설치.
/// 실패한 모드도 그때까지의 비용이 cost에 포함된다.
#[tauri::command]
async fn translate_batch(app: AppHandle, mod_ids: Vec<String>) -> Result<Vec<BatchModDone>, String> {
    let root = resolve_root(&app)?;
    let settings = load_settings_inner(&app);
    let workers = settings.workers.unwrap_or(20).max(1) as usize;
    let max_retries = settings.max_retries.unwrap_or(2);
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(workers));

    let tasks = mod_ids.iter().map(|mod_id| {
        let app = app.clone();
        let root = root.clone();
        let sem = sem.clone();
        async move {
            let done = match translate_one(&app, mod_id, sem, max_retries).await {
                Ok(r) => {
                    let installed = pack::install_pack(&root, &PathBuf::from(&r.pack_dir)).is_ok();
                    BatchModDone {
                        mod_id: mod_id.clone(),
                        ok: r.ok,
                        review: r.review.len(),
                        failed: r.failed,
                        cost: r.cost,
                        error: None,
                        installed,
                        pack_dir: Some(r.pack_dir),
                        glossary_note: r.glossary_note,
                    }
                }
                Err((e, cost)) => BatchModDone {
                    mod_id: mod_id.clone(),
                    ok: 0,
                    review: 0,
                    failed: 0,
                    cost,
                    error: Some(e),
                    installed: false,
                    pack_dir: None,
                    glossary_note: None,
                },
            };
            let _ = app.emit("batch-mod-done", done.clone());
            done
        }
    });
    Ok(futures::future::join_all(tasks).await)
}

#[tauri::command]
async fn share_pack_cmd(app: AppHandle, pack_dir: String) -> Result<String, String> {
    let nick = load_settings_inner(&app)
        .nick
        .unwrap_or_else(|| "anon".into());
    let proxy = std::env::var("KERBALOC_PROXY_URL").unwrap_or_else(|_| share::DEFAULT_PROXY.into());
    let r = share::share_pack(&proxy, &PathBuf::from(pack_dir), &nick)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.pr_url)
}

#[derive(Serialize, Clone)]
struct ShareSkipped {
    mod_id: String,
    error: String,
}

#[derive(Serialize, Clone)]
struct BatchShareResult {
    pr_url: Option<String>, // 공유된 팩이 하나도 없으면 None
    shared: Vec<String>,    // 이 PR에 포함된 mod_id들
    skipped: Vec<ShareSkipped>,
}

fn share_nick_and_proxy(app: &AppHandle) -> (String, String) {
    let nick = load_settings_inner(app).nick.unwrap_or_else(|| "anon".into());
    let proxy = std::env::var("KERBALOC_PROXY_URL").unwrap_or_else(|_| share::DEFAULT_PROXY.into());
    (nick, proxy)
}

/// 설치된 번역 팩들(GameData/KerbaLoc/ko/<ModId>)을 검증 후 **한 번의 PR**로 공유.
/// 검증 실패한 팩은 건너뛰고 skipped로 보고한다.
#[tauri::command]
async fn share_installed(app: AppHandle, mod_ids: Vec<String>) -> Result<BatchShareResult, String> {
    let root = resolve_root(&app)?;
    let (nick, proxy) = share_nick_and_proxy(&app);
    let mut dirs: Vec<(String, PathBuf)> = vec![];
    let mut skipped = vec![];
    for mod_id in mod_ids {
        let dir = root
            .join("GameData")
            .join("KerbaLoc")
            .join("ko")
            .join(&mod_id);
        if !dir.is_dir() {
            skipped.push(ShareSkipped { mod_id, error: "설치되어 있지 않음".into() });
            continue;
        }
        let v = pack::validate_pack(&dir, None);
        if !v.errors.is_empty() {
            skipped.push(ShareSkipped {
                mod_id,
                error: format!("팩 검증 실패: {}", v.errors.join("; ")),
            });
            continue;
        }
        dirs.push((mod_id, dir));
    }
    if dirs.is_empty() {
        return Ok(BatchShareResult { pr_url: None, shared: vec![], skipped });
    }
    let paths: Vec<&std::path::Path> = dirs.iter().map(|(_, d)| d.as_path()).collect();
    let r = share::share_packs(&proxy, &paths, &nick)
        .await
        .map_err(|e| e.to_string())?;
    Ok(BatchShareResult {
        pr_url: Some(r.pr_url),
        shared: dirs.into_iter().map(|(m, _)| m).collect(),
        skipped,
    })
}

/// 팩 디렉터리 여러 개를 한 번의 PR로 공유 (일괄 번역 결과 공유용).
#[tauri::command]
async fn share_packs_cmd(app: AppHandle, pack_dirs: Vec<String>) -> Result<String, String> {
    let (nick, proxy) = share_nick_and_proxy(&app);
    let dirs: Vec<PathBuf> = pack_dirs.into_iter().map(PathBuf::from).collect();
    let paths: Vec<&std::path::Path> = dirs.iter().map(|d| d.as_path()).collect();
    let r = share::share_packs(&proxy, &paths, &nick)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.pr_url)
}

#[derive(Serialize)]
struct ModKey {
    key: String,
    en: String,
    ko: Option<String>,
}

/// 모드의 전체 번역 키 목록: 원문(en-us) + 설치된 팩의 번역(있으면).
#[tauri::command]
fn get_mod_keys(app: AppHandle, mod_id: String) -> Result<Vec<ModKey>, String> {
    use kerbaloc_core::{cfg, loc};
    let root = resolve_root(&app)?;
    let unit = cached_unit(&app, &mod_id)?;
    let installed_cfg = root
        .join("GameData")
        .join("KerbaLoc")
        .join("ko")
        .join(&mod_id)
        .join("Localization")
        .join("ko.cfg");
    let translated = std::fs::read_to_string(&installed_cfg)
        .ok()
        .and_then(|t| cfg::parse(&t).ok())
        .map(|r| loc::extract_localization(&r, "ko"))
        .unwrap_or_default();
    Ok(unit
        .entries
        .iter()
        .map(|(k, en)| ModKey {
            key: k.clone(),
            en: en.clone(),
            ko: translated.get(k).cloned(),
        })
        .collect())
}

#[derive(Deserialize)]
struct KeyEdit {
    key: String,
    ko: String,
}

#[derive(Serialize)]
struct SaveKeysResult {
    errors: Vec<String>,
    installed: Option<String>,
    pack_dir: Option<String>,
}

/// 편집된 번역을 검증 → manual 변형 팩 생성 → 게임에 설치.
/// 오류가 있으면 아무것도 쓰지 않고 오류 목록만 반환.
#[tauri::command]
fn save_mod_keys(
    app: AppHandle,
    mod_id: String,
    edits: Vec<KeyEdit>,
) -> Result<SaveKeysResult, String> {
    use kerbaloc_core::validate::{validate_key_translation, Severity};
    use std::collections::BTreeMap;
    let root = resolve_root(&app)?;
    let settings = load_settings_inner(&app);
    let nick = settings.nick.unwrap_or_else(|| "anon".into());
    let unit = cached_unit(&app, &mod_id)?;
    let _ = &root;

    let mut translations: BTreeMap<String, String> = BTreeMap::new();
    let mut errors: Vec<String> = vec![];
    for e in &edits {
        let ko = e.ko.trim();
        if ko.is_empty() {
            continue; // 빈 값 = 미번역 (영어 폴백)
        }
        match unit.entries.get(&e.key) {
            None => errors.push(format!("{}: 원문에 없는 키 (모드 업데이트됨?)", e.key)),
            Some(en) => {
                for f in validate_key_translation(&e.key, en, ko) {
                    if matches!(f.severity, Severity::Error) {
                        errors.push(format!("{}: [{}] {}", e.key, f.rule, f.message));
                    }
                }
                translations.insert(e.key.clone(), ko.to_string());
            }
        }
    }
    if !errors.is_empty() {
        return Ok(SaveKeysResult {
            errors,
            installed: None,
            pack_dir: None,
        });
    }
    if translations.is_empty() {
        return Err("번역된 키가 하나도 없습니다".into());
    }

    let variant = packgen::make_variant_id("manual", &nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(&pack_dir, &unit, &translations, &variant, "manual")
        .map_err(|e| e.to_string())?;
    let dest = pack::install_pack(&root, &pack_dir).map_err(|e| e.to_string())?;
    Ok(SaveKeysResult {
        errors: vec![],
        installed: Some(dest.to_string_lossy().to_string()),
        pack_dir: Some(pack_dir.to_string_lossy().to_string()),
    })
}

fn mod_glossary_path(root: &std::path::Path, mod_id: &str) -> PathBuf {
    root.join("KerbaLoc-glossaries").join(format!("{mod_id}.ko.json"))
}

/// 저장된 모드 용어집 로드 (없으면 빈 목록).
#[tauri::command]
fn get_mod_glossary(
    app: AppHandle,
    mod_id: String,
) -> Result<Vec<modglossary::ModGlossaryEntry>, String> {
    let root = resolve_root(&app)?;
    Ok(modglossary::ModGlossary::load(&mod_glossary_path(&root, &mod_id))
        .map(|g| g.entries)
        .unwrap_or_default())
}

#[derive(Serialize)]
struct GenGlossaryResult {
    entries: Vec<modglossary::ModGlossaryEntry>,
    cost: f64,
}

/// 용어집 초안 생성 (수동 버튼) — 기존 항목은 유지하고 새 용어만 미확정으로 추가.
#[tauri::command]
async fn gen_mod_glossary(app: AppHandle, mod_id: String) -> Result<GenGlossaryResult, String> {
    use kerbaloc_core::llm::create_provider;
    let root = resolve_root(&app)?;
    let settings = load_settings_inner(&app);
    let cfg = settings.provider_config()?;
    let provider = create_provider(&cfg).map_err(|e| e.to_string())?;
    let unit = cached_unit(&app, &mod_id)?;

    let (r, usage) = modglossary::generate_draft(
        provider.as_ref(),
        &mod_glossary_path(&root, &mod_id),
        &unit.display_name,
        &unit.entries,
    )
    .await;
    let g = r.map_err(|e| e.to_string())?;
    Ok(GenGlossaryResult {
        entries: g.entries,
        cost: usage.cost_usd(provider.prices().0, provider.prices().1),
    })
}

/// 편집된 용어집 저장 — 저장 시점의 모든 항목을 확정으로 표시.
#[tauri::command]
fn save_mod_glossary(
    app: AppHandle,
    mod_id: String,
    entries: Vec<modglossary::ModGlossaryEntry>,
) -> Result<(), String> {
    let root = resolve_root(&app)?;
    let entries = entries
        .into_iter()
        .map(|mut e| {
            e.confirmed = true;
            e
        })
        .collect();
    modglossary::ModGlossary { version: 1, entries }
        .save(&mod_glossary_path(&root, &mod_id))
        .map_err(|e| e.to_string())
}

/// 편집 중인(저장 전 포함) 프로필로 제공자의 모델 목록 조회.
#[tauri::command]
async fn list_models_cmd(profile: LlmProfile) -> Result<Vec<String>, String> {
    let mut cfg = profile.provider_config()?;
    if cfg.model.is_empty() {
        cfg.model = "placeholder".into(); // 목록 조회에는 모델 불필요
    }
    kerbaloc_core::llm::list_models(&cfg)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Settings {
    load_settings_inner(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    std::fs::write(
        settings_path(&app),
        serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// "0.1.12" 류 점 구분 숫자 버전 비교: a가 b보다 최신인가.
fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    };
    parse(a) > parse(b)
}

/// 포터블 단일 exe 자체 업데이트: GitHub 최신 릴리스의 KerbaLoc.exe가 더 새 버전이면
/// 동의 없이 내려받아 자기 자신을 교체하고 재시작한다.
/// (실행 중 exe는 rename 가능 — 구파일은 .old로 밀어두고 다음 시작 때 정리)
async fn auto_update(handle: &AppHandle) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let old = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old); // 이전 업데이트 잔여물 정리

    let client = reqwest::Client::builder()
        .user_agent(concat!("kerbaloc-app/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let rel: serde_json::Value = client
        .get("https://api.github.com/repos/csh1668/kerbaloc/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let latest = rel
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("tag_name 없음"))?;
    let current = handle.package_info().version.to_string();
    if !version_newer(latest, &current) {
        return Ok(());
    }
    let url = rel
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("KerbaLoc.exe"))
        })
        .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
        .ok_or_else(|| anyhow::anyhow!("릴리스에 KerbaLoc.exe 자산 없음"))?;
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    anyhow::ensure!(
        bytes.len() > 1_000_000 && bytes.starts_with(b"MZ"),
        "다운로드된 파일이 실행 파일이 아님"
    );

    let tmp = exe.with_extension("exe.new");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::rename(&old, &exe); // 롤백
        return Err(e.into());
    }
    std::process::Command::new(&exe).spawn()?;
    handle.exit(0);
    Ok(())
}

fn spawn_auto_update(app: &tauri::App) {
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let _ = auto_update(&handle).await; // 실패는 조용히 무시 — 다음 실행 때 재시도
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            spawn_auto_update(app);
            Ok(())
        })
        .manage(ScanCache::default())
        .invoke_handler(tauri::generate_handler![
            game_status,
            set_language,
            scan_units,
            db_index,
            install_from_db,
            remove_pack,
            translate_batch,
            share_pack_cmd,
            share_packs_cmd,
            share_installed,
            get_mod_keys,
            save_mod_keys,
            get_mod_glossary,
            gen_mod_glossary,
            save_mod_glossary,
            list_models_cmd,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(json: &str) -> Settings {
        let mut s: Settings = serde_json::from_str(json).unwrap();
        s.migrate_legacy();
        s
    }

    #[test]
    fn legacy_settings_become_profiles() {
        // 구버전: anthropic 선택 + gemini·anthropic 키 저장, openai 키 없음
        let s = load(
            r#"{"provider":"anthropic","model":"claude-opus-5-5","gemini_api_key":"g",
                "anthropic_api_key":"a","openai_api_key":"","price_in":1.0,"price_out":2.0}"#,
        );
        let ids: Vec<&str> = s.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["gemini", "anthropic"], "키가 있는 제공자만");
        assert_eq!(s.active_profile.as_deref(), Some("anthropic"));
        let a = s.active().unwrap();
        assert_eq!(a.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!((a.price_in, a.price_out), (Some(1.0), Some(2.0)));
        let g = &s.profiles[0];
        assert_eq!(g.api_key.as_deref(), Some("g"));
        assert!(
            g.model.is_none() && g.price_in.is_none(),
            "모델·단가는 선택 제공자에만"
        );
    }

    #[test]
    fn empty_settings_get_default_gemini_profile() {
        let s = load("{}");
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.profiles[0].provider, "gemini");
        assert_eq!(s.provider_config().unwrap().model, "gemini-3.1-flash-lite");
    }

    #[test]
    fn existing_profiles_untouched_and_legacy_not_saved() {
        let s = load(
            r#"{"gemini_api_key":"old","active_profile":"x",
                "profiles":[{"id":"x","name":"내 Codex","provider":"codex","reasoning":"high"}]}"#,
        );
        assert_eq!(s.profiles.len(), 1);
        let cfg = s.provider_config().unwrap();
        assert_eq!(cfg.kind, kerbaloc_core::llm::ProviderKind::Codex);
        assert_eq!(cfg.reasoning.as_deref(), Some("high"));
        let out = serde_json::to_string(&s).unwrap();
        assert!(
            !out.contains("gemini_api_key"),
            "구버전 필드는 다시 저장하지 않음"
        );
    }

    #[test]
    fn unknown_active_falls_back_to_first() {
        let s = load(
            r#"{"active_profile":"gone","profiles":[{"id":"a","name":"A","provider":"gemini"}]}"#,
        );
        assert_eq!(s.active().unwrap().id, "a");
    }
}
