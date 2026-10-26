use kerbaloc_core::llm::ProviderKind;
use kerbaloc_core::settings::Settings;

fn load(json: &str) -> Settings {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("settings.json");
    std::fs::write(&path, json).unwrap();
    Settings::load_from(&path)
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
fn missing_file_gets_default_gemini_profile() {
    let s = Settings::load_from(std::path::Path::new("Z:/없는/경로/settings.json"));
    assert_eq!(s.profiles.len(), 1);
    assert_eq!(s.profiles[0].provider, "gemini");
    let cfg = s.active().unwrap().provider_config().unwrap();
    assert_eq!(
        cfg.model, "gemini-3.1-flash-lite",
        "모델 미지정이면 기본 모델"
    );
}

#[test]
fn existing_profiles_untouched_and_legacy_not_saved() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"gemini_api_key":"old","active_profile":"x",
            "profiles":[{"id":"x","name":"내 Codex","provider":"codex","reasoning":"high"}]}"#,
    )
    .unwrap();
    let s = Settings::load_from(&path);
    assert_eq!(s.profiles.len(), 1);
    let cfg = s.active().unwrap().provider_config().unwrap();
    assert_eq!(cfg.kind, ProviderKind::Codex);
    assert_eq!(cfg.reasoning.as_deref(), Some("high"));

    s.save_to(&path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        !saved.contains("gemini_api_key"),
        "구버전 필드는 다시 저장하지 않음"
    );
    assert_eq!(
        Settings::load_from(&path).profiles[0].name,
        "내 Codex",
        "왕복 보존"
    );
}

#[test]
fn profile_lookup_by_id_or_name_and_fallback() {
    let s = load(
        r#"{"active_profile":"gone","profiles":[
            {"id":"a","name":"Alpha","provider":"gemini"},
            {"id":"b","name":"Beta","provider":"claude-code"}]}"#,
    );
    assert_eq!(s.active().unwrap().id, "a", "활성 id가 없으면 첫 프로필");
    assert_eq!(
        s.profile_or_active(Some("Beta")).unwrap().id,
        "b",
        "이름으로도 찾음"
    );
    assert_eq!(s.profile_or_active(Some("b")).unwrap().name, "Beta");
    assert!(s.profile_or_active(Some("nope")).is_err());
}

#[test]
fn defaults_for_run_options() {
    let s = load(r#"{"nick":"  ","workers":0}"#);
    assert_eq!(s.nick(), "anon", "빈 닉네임은 anon");
    assert_eq!(s.workers(), 1, "워커 최소 1");
    assert_eq!(s.max_retries(), 2);
    assert_eq!(s.batch_options().max_keys, 40);
}

#[test]
fn settings_path_matches_tauri_app_config_dir() {
    // Tauri 2: app_config_dir = dirs::config_dir()/<identifier> (tauri.conf.json)
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../../kerbaloc-app/src-tauri/tauri.conf.json")).unwrap();
    assert_eq!(conf["identifier"], kerbaloc_core::settings::APP_ID);
    assert!(kerbaloc_core::settings::settings_path().ends_with("dev.kerbaloc.studio/settings.json"));
}
