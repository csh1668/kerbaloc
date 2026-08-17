//! KerbaLoc 스튜디오 — kerbaloc-core를 Tauri 커맨드로 노출.

use kerbaloc_core::{
    dbclient, game, glossary::Glossary, jobstore::JobStore, pack, packgen, pipeline, scan, share,
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

#[derive(Serialize, Deserialize, Default, Clone)]
struct Settings {
    ksp_root: Option<String>,
    nick: Option<String>,
    /// gemini | openai | anthropic | claude-code | ollama | lmstudio (기본 gemini)
    provider: Option<String>,
    /// 모델 ID — 비우면 제공자별 기본 모델
    model: Option<String>,
    gemini_api_key: Option<String>,
    openai_api_key: Option<String>,
    anthropic_api_key: Option<String>,
    /// OpenAI 호환 베이스 URL 오버라이드 (…/v1)
    ollama_url: Option<String>,
    lmstudio_url: Option<String>,
    /// 1M 토큰당 USD — 비우면 모델별 기본 단가
    price_in: Option<f64>,
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
    fn provider_config(&self) -> Result<kerbaloc_core::llm::ProviderConfig, String> {
        use kerbaloc_core::llm::{default_model, ProviderConfig, ProviderKind};
        let kind: ProviderKind = self
            .provider
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("gemini")
            .parse()
            .map_err(|e: anyhow::Error| e.to_string())?;
        let model = self
            .model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| default_model(kind).to_string());
        let (api_key, base_url) = match kind {
            ProviderKind::Gemini => (self.gemini_api_key.clone(), None),
            ProviderKind::OpenAi => (self.openai_api_key.clone(), None),
            ProviderKind::Anthropic => (self.anthropic_api_key.clone(), None),
            ProviderKind::ClaudeCode => (None, None),
            ProviderKind::Ollama => (None, self.ollama_url.clone()),
            ProviderKind::LmStudio => (None, self.lmstudio_url.clone()),
        };
        let prices = match (self.price_in, self.price_out) {
            (Some(i), Some(o)) => Some((i, o)),
            _ => None,
        };
        Ok(ProviderConfig { kind, model, api_key, base_url, prices })
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
    std::fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
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

    let gpath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../glossary/core.ko.json");
    let glossary = Glossary::load(&gpath).map_err(|e| fail0(e.to_string()))?;

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
                JobStore::create(&job_dir, &m).map_err(|e| fail0(e.to_string()))?,
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
    .map_err(|e| fail0(e.to_string()))?;

    let cost = report
        .usage
        .cost_usd(provider.prices().0, provider.prices().1);

    let variant = packgen::make_variant_id(&sanitize_method(&cfg.model), &nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(&pack_dir, &unit, &report.ok, &variant, &cfg.model)
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

/// 현재(저장 전 포함) 설정으로 제공자의 모델 목록 조회.
#[tauri::command]
async fn list_models_cmd(settings: Settings) -> Result<Vec<String>, String> {
    let mut cfg = settings.provider_config()?;
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

/// 앱 시작 시 백그라운드로 업데이트 확인 — 새 버전이 있으면 동의 없이
/// 자동 다운로드·설치 후 재시작한다.
fn spawn_auto_update(app: &tauri::App) {
    use tauri_plugin_updater::UpdaterExt;
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let Ok(updater) = handle.updater() else { return };
        let Ok(Some(update)) = updater.check().await else { return };
        if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
            handle.restart();
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            list_models_cmd,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}
