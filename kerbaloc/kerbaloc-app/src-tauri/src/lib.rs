//! KerbaLoc 스튜디오 — kerbaloc-core를 Tauri 커맨드로 노출.

use kerbaloc_core::{dbclient, game, glossary::Glossary, jobstore::JobStore, pack, packgen, pipeline, scan, share};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

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
fn scan_units(app: AppHandle) -> Result<Vec<UnitInfo>, String> {
    let root = resolve_root(&app)?;
    let installed_dir = root.join("GameData").join("KerbaLoc").join("ko");
    Ok(scan::scan_gamedata(&root)
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
    let m = dbclient::fetch_manifest().await.map_err(|e| e.to_string())?;
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
async fn install_from_db(app: AppHandle, mod_id: String, variant: Option<String>) -> Result<String, String> {
    let root = resolve_root(&app)?;
    let m = dbclient::fetch_manifest().await.map_err(|e| e.to_string())?;
    let i = dbclient::fetch_index(&m).await.map_err(|e| e.to_string())?;
    let p = i
        .packs
        .iter()
        .find(|p| p.mod_id == mod_id)
        .ok_or_else(|| format!("DB에 {mod_id} 없음"))?;
    let v = match &variant {
        Some(id) => p.variants.iter().find(|v| &v.variant_id == id).ok_or("변형 없음")?,
        None => p.variants.last().ok_or("변형 없음")?,
    };
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    dbclient::download_variant(&m, v, tmp.path())
        .await
        .map_err(|e| e.to_string())?;
    let src = scan::scan_gamedata(&root)
        .into_iter()
        .find(|u| u.mod_id == mod_id)
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
    done: usize,
    total: usize,
    cost: f64,
}

#[derive(Serialize)]
struct TranslateResult {
    ok: usize,
    review: Vec<serde_json::Value>,
    failed: usize,
    cost: f64,
    pack_dir: String,
}

#[tauri::command]
async fn translate_mod(app: AppHandle, mod_id: String) -> Result<TranslateResult, String> {
    use kerbaloc_core::llm::{gemini::GeminiProvider, Provider};
    let root = resolve_root(&app)?;
    let settings = load_settings_inner(&app);
    let api_key = settings
        .gemini_api_key
        .filter(|k| !k.is_empty())
        .ok_or("설정에서 Gemini API 키를 입력하세요")?;
    let nick = settings.nick.unwrap_or_else(|| "anon".into());

    let unit = scan::scan_gamedata(&root)
        .into_iter()
        .find(|u| u.mod_id == mod_id)
        .ok_or_else(|| format!("설치본에서 {mod_id}를 찾지 못했습니다"))?;

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
            let _ = app2.emit("translate-progress", TranslateProgress { done, total, cost });
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let variant = packgen::make_variant_id("gemini31flashlite", &nick);
    let pack_dir = root.join("KerbaLoc-packs").join(&unit.mod_id).join(&variant);
    packgen::build_pack(&pack_dir, &unit, &report.ok, &variant, "gemini-3.1-flash-lite")
        .map_err(|e| e.to_string())?;

    Ok(TranslateResult {
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
fn install_local_pack(app: AppHandle, pack_dir: String) -> Result<String, String> {
    let root = resolve_root(&app)?;
    let dest = pack::install_pack(&root, &PathBuf::from(pack_dir)).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
async fn share_pack_cmd(app: AppHandle, pack_dir: String) -> Result<String, String> {
    let nick = load_settings_inner(&app).nick.unwrap_or_else(|| "anon".into());
    let proxy =
        std::env::var("KERBALOC_PROXY_URL").unwrap_or_else(|_| share::DEFAULT_PROXY.into());
    let r = share::share_pack(&proxy, &PathBuf::from(pack_dir), &nick)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.pr_url)
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
        .invoke_handler(tauri::generate_handler![
            game_status,
            set_language,
            scan_units,
            db_index,
            install_from_db,
            remove_pack,
            translate_mod,
            install_local_pack,
            share_pack_cmd,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}
