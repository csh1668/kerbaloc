//! KerbaLoc 스튜디오 — kerbaloc-core를 Tauri 커맨드로 노출.

use kerbaloc_core::llm::{create_provider, model_choices, ModelChoices, Provider, ProviderInfo};
use kerbaloc_core::settings::{LlmProfile, Settings};
use kerbaloc_core::translate::{self, TranslateOptions, TranslateOutcome};
use kerbaloc_core::{dbclient, game, modglossary, pack, packgen, scan, share};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// GameData 스캔은 수 초 걸리므로 결과를 캐시한다. refresh(force)로만 무효화.
#[derive(Default)]
struct ScanCache(Mutex<Option<Vec<scan::ModUnit>>>);

fn cached_units(app: &AppHandle, force: bool) -> Result<Vec<scan::ModUnit>, String> {
    let root = resolve_root()?;
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

fn resolve_root() -> Result<PathBuf, String> {
    if let Some(p) = Settings::load().ksp_root() {
        return Ok(p);
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
fn game_status() -> Result<serde_json::Value, String> {
    let root = resolve_root()?;
    Ok(serde_json::json!({
        "root": root.to_string_lossy(),
        "language": game::read_language(&root),
    }))
}

#[tauri::command]
fn set_language(lang: String) -> Result<(), String> {
    let root = resolve_root()?;
    game::set_language(&root, &lang).map_err(|e| e.to_string())
}

/// async: 스캔이 수 초 걸리므로 메인(UI) 스레드를 막지 않게 런타임 스레드에서 실행.
#[tauri::command]
async fn scan_units(app: AppHandle, force: bool) -> Result<Vec<UnitInfo>, String> {
    let root = resolve_root()?;
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
    let root = resolve_root()?;
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
fn remove_pack(mod_id: String) -> Result<bool, String> {
    let root = resolve_root()?;
    pack::remove_pack(&root, "ko", &mod_id).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
struct TranslateProgress {
    mod_id: String,
    done: usize,
    total: usize,
    cost: f64,
}

/// 모드 1개 번역 (코어 공용 로직) + 진행 이벤트 발행.
/// 실패 시에도 그때까지 지출한 비용을 함께 반환한다: Err((사유, 비용)).
async fn translate_one(
    app: &AppHandle,
    root: &std::path::Path,
    provider: &dyn Provider,
    opts: &TranslateOptions<'_>,
    mod_id: &str,
    sem: std::sync::Arc<tokio::sync::Semaphore>,
) -> Result<TranslateOutcome, (String, f64)> {
    let unit = cached_unit(app, mod_id).map_err(|e| (e, 0.0))?;
    let app2 = app.clone();
    let mod_id2 = mod_id.to_string();
    translate::translate_mod(
        root,
        &unit,
        provider,
        opts,
        sem,
        &move |done, total, cost| {
            let _ = app2.emit(
                "translate-progress",
                TranslateProgress {
                    mod_id: mod_id2.clone(),
                    done,
                    total,
                    cost,
                },
            );
        },
    )
    .await
    .map_err(|(e, cost)| (e.to_string(), cost))
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
    let root = resolve_root()?;
    let settings = Settings::load();
    // 활성 프로필을 한 번만 해석 (모델 미지정이면 목록 조회) — 모든 모드가 같은 제공자 공유
    let cfg = settings
        .resolved_provider_config(None)
        .await
        .map_err(|e| e.to_string())?;
    let provider = create_provider(&cfg).map_err(|e| e.to_string())?;
    let provider = provider.as_ref();
    let nick = settings.nick();
    let opts = TranslateOptions {
        nick: &nick,
        batch: settings.batch_options(),
        max_retries: settings.max_retries(),
        fresh: false,
    };
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(settings.workers()));

    let tasks = mod_ids.iter().map(|mod_id| {
        let app = app.clone();
        let root = root.clone();
        let sem = sem.clone();
        let opts = &opts;
        async move {
            let done = match translate_one(&app, &root, provider, opts, mod_id, sem).await {
                Ok(r) => {
                    let installed = pack::install_pack(&root, &r.pack_dir).is_ok();
                    BatchModDone {
                        mod_id: mod_id.clone(),
                        ok: r.report.ok.len(),
                        review: r.report.review.len(),
                        failed: r.report.failed.len(),
                        cost: r.cost,
                        error: None,
                        installed,
                        pack_dir: Some(r.pack_dir.to_string_lossy().to_string()),
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
async fn share_pack_cmd(pack_dir: String) -> Result<String, String> {
    let nick = Settings::load().nick();
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

fn share_nick_and_proxy() -> (String, String) {
    let nick = Settings::load().nick();
    let proxy = std::env::var("KERBALOC_PROXY_URL").unwrap_or_else(|_| share::DEFAULT_PROXY.into());
    (nick, proxy)
}

/// 설치된 번역 팩들(GameData/KerbaLoc/ko/<ModId>)을 검증 후 **한 번의 PR**로 공유.
/// 검증 실패한 팩은 건너뛰고 skipped로 보고한다.
#[tauri::command]
async fn share_installed(mod_ids: Vec<String>) -> Result<BatchShareResult, String> {
    let root = resolve_root()?;
    let (nick, proxy) = share_nick_and_proxy();
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
async fn share_packs_cmd(pack_dirs: Vec<String>) -> Result<String, String> {
    let (nick, proxy) = share_nick_and_proxy();
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
    let root = resolve_root()?;
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
    let root = resolve_root()?;
    let nick = Settings::load().nick();
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

/// 저장된 모드 용어집 로드 (없으면 빈 목록).
#[tauri::command]
fn get_mod_glossary(mod_id: String) -> Result<Vec<modglossary::ModGlossaryEntry>, String> {
    let root = resolve_root()?;
    Ok(
        modglossary::ModGlossary::load(&modglossary::path_for(&root, &mod_id))
            .map(|g| g.entries)
            .unwrap_or_default(),
    )
}

#[derive(Serialize)]
struct GenGlossaryResult {
    entries: Vec<modglossary::ModGlossaryEntry>,
    cost: f64,
}

/// 용어집 초안 생성 (수동 버튼) — 기존 항목은 유지하고 새 용어만 미확정으로 추가.
#[tauri::command]
async fn gen_mod_glossary(app: AppHandle, mod_id: String) -> Result<GenGlossaryResult, String> {
    let root = resolve_root()?;
    let cfg = Settings::load()
        .resolved_provider_config(None)
        .await
        .map_err(|e| e.to_string())?;
    let provider = create_provider(&cfg).map_err(|e| e.to_string())?;
    let unit = cached_unit(&app, &mod_id)?;

    let (r, usage) = modglossary::generate_draft(
        provider.as_ref(),
        &modglossary::path_for(&root, &mod_id),
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
    mod_id: String,
    entries: Vec<modglossary::ModGlossaryEntry>,
) -> Result<(), String> {
    let root = resolve_root()?;
    let entries = entries
        .into_iter()
        .map(|mut e| {
            e.confirmed = true;
            e
        })
        .collect();
    modglossary::ModGlossary {
        version: 1,
        entries,
    }
    .save(&modglossary::path_for(&root, &mod_id))
    .map_err(|e| e.to_string())
}

/// 편집 중인(저장 전 포함) 프로필로 모델 목록 조회 + 선택 (코어 공용 규칙:
/// 고른 값 → 기본 모델 → 목록 최상단). 조회 실패 시 제공자 fallback 목록과 사유를 함께 반환.
#[tauri::command]
async fn list_models_cmd(profile: LlmProfile) -> Result<ModelChoices, String> {
    let cfg = profile.provider_config().map_err(|e| e.to_string())?;
    Ok(model_choices(&cfg, profile.model.as_deref()).await)
}

/// 제공자 메타데이터 (설정 화면 SSOT) — 라벨·기본 모델·URL·추론 강도 선택지 등.
#[tauri::command]
fn provider_catalog() -> Vec<ProviderInfo> {
    kerbaloc_core::llm::provider_catalog()
}

#[tauri::command]
fn load_settings() -> Settings {
    Settings::load()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())
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
    // 개발 빌드(tauri dev)는 버전이 0.1.0이라 릴리스 exe로 자기 자신을 덮어쓰고 재실행해 버린다
    if cfg!(debug_assertions) {
        return;
    }
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
            provider_catalog,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}
