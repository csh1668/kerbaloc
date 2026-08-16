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
    gemini_api_key: Option<String>,
    nick: Option<String>,
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
    let src = cached_unit(&app, &mod_id).ok().map(|u| u.entries);
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

async fn translate_one(app: &AppHandle, mod_id: &str) -> Result<TranslateResult, String> {
    use kerbaloc_core::llm::{gemini::GeminiProvider, Provider};
    let root = resolve_root(app)?;
    let settings = load_settings_inner(app);
    let api_key = settings
        .gemini_api_key
        .filter(|k| !k.is_empty())
        .ok_or("설정에서 Gemini API 키를 입력하세요")?;
    let nick = settings.nick.unwrap_or_else(|| "anon".into());

    let unit = cached_unit(app, mod_id)?;

    let gpath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../glossary/core.ko.json");
    let glossary = Glossary::load(&gpath).map_err(|e| e.to_string())?;
    let provider = GeminiProvider::new("gemini-3.1-flash-lite", &api_key);
    let escalation = GeminiProvider::new("gemini-3-flash", &api_key);

    let job_dir = root.join("KerbaLoc-jobs").join(&unit.mod_id);
    let manifest = pipeline::make_manifest(
        &unit.mod_id,
        &unit.source_hash,
        provider.name(),
        provider.prices(),
        &unit.entries,
    );
    let store = JobStore::create(&job_dir, &manifest).map_err(|e| e.to_string())?;

    let app2 = app.clone();
    let mod_id2 = mod_id.to_string();
    let report = pipeline::translate_job(
        &provider,
        &escalation,
        &unit.entries,
        &glossary,
        &unit.display_name,
        &store,
        &manifest,
        8,
        &move |done, total, cost| {
            let _ = app2.emit(
                "translate-progress",
                TranslateProgress { mod_id: mod_id2.clone(), done, total, cost },
            );
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let variant = packgen::make_variant_id("gemini31flashlite", &nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(
        &pack_dir,
        &unit,
        &report.ok,
        &variant,
        "gemini-3.1-flash-lite",
    )
    .map_err(|e| e.to_string())?;

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
        cost: report
            .usage
            .cost_usd(provider.prices().0, provider.prices().1),
        pack_dir: pack_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
async fn translate_mod(app: AppHandle, mod_id: String) -> Result<TranslateResult, String> {
    translate_one(&app, &mod_id).await
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
}

/// 여러 모드를 순차 번역(모드 내부는 병렬 배치). 각 모드 완료마다 이벤트 발행,
/// 성공한 팩은 즉시 게임에 설치.
#[tauri::command]
async fn translate_batch(app: AppHandle, mod_ids: Vec<String>) -> Result<Vec<BatchModDone>, String> {
    let root = resolve_root(&app)?;
    let mut out = vec![];
    for mod_id in mod_ids {
        let done = match translate_one(&app, &mod_id).await {
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
                }
            }
            Err(e) => BatchModDone {
                mod_id: mod_id.clone(),
                ok: 0,
                review: 0,
                failed: 0,
                cost: 0.0,
                error: Some(e),
                installed: false,
            },
        };
        let _ = app.emit("batch-mod-done", done.clone());
        out.push(done);
    }
    Ok(out)
}

#[tauri::command]
fn install_local_pack(app: AppHandle, pack_dir: String) -> Result<String, String> {
    let root = resolve_root(&app)?;
    let dest = pack::install_pack(&root, &PathBuf::from(pack_dir)).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ScanCache::default())
        .invoke_handler(tauri::generate_handler![
            game_status,
            set_language,
            scan_units,
            db_index,
            install_from_db,
            remove_pack,
            translate_mod,
            translate_batch,
            install_local_pack,
            share_pack_cmd,
            get_mod_keys,
            save_mod_keys,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}
