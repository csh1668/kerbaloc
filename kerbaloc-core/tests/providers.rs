use kerbaloc_core::llm::{
    anthropic, claude_code, codex, create_provider, default_prices, gemini, openai_compat,
    parse_items_text, ProviderConfig, ProviderKind,
};

#[test]
fn parse_items_text_handles_fences_and_prose() {
    let plain = r#"[{"i":1,"ko":"안녕"}]"#;
    assert_eq!(parse_items_text(plain).unwrap().len(), 1);

    let fenced = "```json\n[{\"i\":1,\"ko\":\"안녕\"}]\n```";
    assert_eq!(parse_items_text(fenced).unwrap().len(), 1);

    let prose = "Here is the translation:\n[{\"i\":2,\"ko\":\"세계\"}]\nDone.";
    let v = parse_items_text(prose).unwrap();
    assert_eq!(v[0].i, 2);

    assert!(parse_items_text("I cannot translate this.").is_err());
}

#[test]
fn openai_compat_parse_keeps_usage_on_failure() {
    let ok = r#"{"choices":[{"message":{"content":"[{\"i\":1,\"ko\":\"안녕\"}]"}}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
    let (items, usage) = openai_compat::parse_response(ok);
    assert_eq!(items.unwrap().len(), 1);
    assert_eq!(usage.input_tokens, 10);

    let bad = r#"{"choices":[{"message":{"content":"no json"}}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
    let (items, usage) = openai_compat::parse_response(bad);
    assert!(items.is_err());
    assert_eq!(usage.input_tokens, 7, "파싱 실패해도 토큰 집계");
}

#[test]
fn anthropic_parse_keeps_usage_on_failure() {
    let ok = r#"{"content":[{"type":"text","text":"[{\"i\":1,\"ko\":\"안녕\"}]"}],"usage":{"input_tokens":20,"output_tokens":9}}"#;
    let (items, usage) = anthropic::parse_response(ok);
    assert_eq!(items.unwrap().len(), 1);
    assert_eq!(usage.output_tokens, 9);

    let bad = r#"{"content":[{"type":"text","text":"nope"}],"usage":{"input_tokens":4,"output_tokens":1}}"#;
    let (items, usage) = anthropic::parse_response(bad);
    assert!(items.is_err());
    assert_eq!(usage.input_tokens, 4);
}

#[test]
fn claude_code_parse_output() {
    let ok = r#"{"type":"result","is_error":false,"result":"[{\"i\":1,\"ko\":\"안녕\"}]","usage":{"input_tokens":30,"output_tokens":12}}"#;
    let (items, usage) = claude_code::parse_output(ok);
    assert_eq!(items.unwrap().len(), 1);
    assert_eq!(usage.input_tokens, 30);

    let err = r#"{"type":"result","is_error":true,"result":"rate limited","usage":{"input_tokens":1,"output_tokens":0}}"#;
    let (items, usage) = claude_code::parse_output(err);
    assert!(items.is_err());
    assert_eq!(usage.input_tokens, 1);
}

#[test]
fn provider_kind_parse_and_default_prices() {
    let k: ProviderKind = "claude-code".parse().unwrap();
    assert_eq!(k, ProviderKind::ClaudeCode);
    assert!("nope".parse::<ProviderKind>().is_err());

    assert_eq!(default_prices(ProviderKind::Ollama, "llama3"), (0.0, 0.0));
    assert_eq!(default_prices(ProviderKind::ClaudeCode, "sonnet"), (0.0, 0.0));
    assert_eq!(default_prices(ProviderKind::Gemini, "gemini-3.1-flash-lite"), (0.25, 1.50));
    assert!(default_prices(ProviderKind::Anthropic, "claude-haiku-4-5").0 > 0.0);
}

fn cfg(kind: ProviderKind, model: &str) -> ProviderConfig {
    ProviderConfig {
        kind,
        model: model.into(),
        api_key: None,
        base_url: None,
        reasoning: None,
        command: None,
        prices: None,
    }
}

#[test]
fn codex_kind_allows_empty_model_and_is_free() {
    let k: ProviderKind = "codex".parse().unwrap();
    assert_eq!(k, ProviderKind::Codex);
    assert_eq!(default_prices(ProviderKind::Codex, ""), (0.0, 0.0));
    let p = create_provider(&cfg(ProviderKind::Codex, "")).unwrap();
    assert_eq!(p.name(), "codex", "모델 미지정 시 이름은 codex");
    assert!(
        create_provider(&cfg(ProviderKind::ClaudeCode, "")).is_err(),
        "다른 제공자는 모델 필수"
    );
}

#[test]
fn blank_options_are_treated_as_unset() {
    let mut c = cfg(ProviderKind::Gemini, "gemini-3.1-flash-lite");
    c.api_key = Some("   ".into());
    assert!(create_provider(&c).is_err(), "공백 키는 미설정");
}

#[test]
fn codex_exec_args() {
    let a = codex::exec_args("gpt-5.5-codex", Some("high"));
    assert_eq!(a[0], "exec");
    assert!(a.contains(&"--json".to_string()));
    assert!(a.windows(2).any(|w| w == ["-s", "read-only"]));
    assert!(a.windows(2).any(|w| w == ["-m", "gpt-5.5-codex"]));
    assert!(a
        .windows(2)
        .any(|w| w == ["-c", "model_reasoning_effort=high"]));
    assert_eq!(a.last().unwrap(), "-", "프롬프트는 stdin");

    let a = codex::exec_args("", None);
    assert!(!a.contains(&"-m".to_string()), "모델 미지정이면 -m 생략");
    assert!(!a.contains(&"-c".to_string()));
}

#[test]
fn codex_parse_jsonl_ignores_warning_items() {
    // codex-cli 0.164 실제 출력 (설정 경고 item 포함)
    let out = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Codex is ignoring 1 unrecognized configuration setting."}}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"[{\"i\":0,\"ko\":\"안녕\"}]"}}
{"type":"turn.completed","usage":{"input_tokens":14441,"cached_input_tokens":0,"output_tokens":16,"reasoning_output_tokens":0}}"#;
    let (r, usage) = codex::parse_jsonl(out);
    assert_eq!(parse_items_text(&r.unwrap()).unwrap()[0].ko, "안녕");
    assert_eq!((usage.input_tokens, usage.output_tokens), (14441, 16));
}

#[test]
fn codex_parse_jsonl_failures() {
    let failed = r#"{"type":"turn.started"}
{"type":"turn.failed","error":{"message":"usage limit reached"}}"#;
    let (r, _) = codex::parse_jsonl(failed);
    assert!(r.unwrap_err().to_string().contains("usage limit"));

    // 최상위 error(재연결 알림 등) 뒤에 응답이 오면 성공
    let recovered = r#"{"type":"error","message":"Reconnecting... 1/5"}
{"type":"item.completed","item":{"type":"agent_message","text":"OK"}}
{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}"#;
    assert_eq!(codex::parse_jsonl(recovered).0.unwrap(), "OK");

    let only_error = r#"{"type":"error","message":"stream disconnected"}"#;
    assert!(codex::parse_jsonl(only_error).0.is_err());
    assert!(codex::parse_jsonl("").0.is_err());
}

#[test]
fn claude_code_cli_args_effort() {
    let a = claude_code::cli_args("sonnet", Some("xhigh"));
    assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]));
    assert!(a.windows(2).any(|w| w == ["--effort", "xhigh"]));
    assert!(!claude_code::cli_args("sonnet", None).contains(&"--effort".to_string()));
}

#[test]
fn anthropic_body_effort_and_no_sampling() {
    let b = anthropic::messages_body("claude-opus-5-5", "sys", "hi", Some("low"));
    assert_eq!(b["output_config"]["effort"], "low");
    assert!(
        b.get("temperature").is_none(),
        "현행 모델은 temperature 거부"
    );
    assert!(b["max_tokens"].as_u64().unwrap() >= 16000);
    let b = anthropic::messages_body("claude-haiku-4-5", "sys", "hi", None);
    assert!(b.get("output_config").is_none());
}

#[test]
fn anthropic_parse_skips_leading_thinking_block() {
    let body = r#"{"content":[{"type":"thinking","thinking":"","signature":"x"},{"type":"text","text":"[{\"i\":1,\"ko\":\"안녕\"}]"}],"usage":{"input_tokens":5,"output_tokens":40}}"#;
    let (items, _) = anthropic::parse_response(body);
    assert_eq!(items.unwrap()[0].ko, "안녕");
}

#[test]
fn openai_body_options() {
    let b = openai_compat::chat_body("gpt-5.1-mini", "sys", "hi", None, Some("low"));
    assert_eq!(b["reasoning_effort"], "low");
    assert!(b.get("temperature").is_none());
    let b = openai_compat::chat_body("llama3", "sys", "hi", Some(0.2), None);
    assert_eq!(b["temperature"], 0.2);
    assert!(b.get("reasoning_effort").is_none());
}

#[test]
fn gemini_thinking_config_and_thought_tokens() {
    let gc = gemini::generation_config(true, Some("low"));
    assert_eq!(gc["thinkingConfig"]["thinkingLevel"], "low");
    assert_eq!(gc["responseMimeType"], "application/json");
    let gc = gemini::generation_config(false, None);
    assert!(gc.get("thinkingConfig").is_none());
    assert!(gc.get("responseSchema").is_none());

    let body = r#"{"candidates":[{"content":{"parts":[{"text":"[{\"i\":1,\"ko\":\"안녕\"}]"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5,"thoughtsTokenCount":93}}"#;
    let (_, usage) = gemini::parse_response(body);
    assert_eq!(usage.output_tokens, 98, "사고 토큰도 출력으로 과금");
}

#[test]
fn pick_model_rules() {
    use kerbaloc_core::llm::pick_model;
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let g = ProviderKind::Gemini;
    // 지정값은 목록에 없어도 유지
    assert_eq!(
        pick_model(g, &list(&["a", "b"]), Some("custom")).as_deref(),
        Some("custom")
    );
    // 미지정 → 기본 모델이 목록에 있으면 기본
    let l = list(&["gemini-2.5-flash", "gemini-3.1-flash-lite"]);
    assert_eq!(
        pick_model(g, &l, None).as_deref(),
        Some("gemini-3.1-flash-lite")
    );
    assert_eq!(
        pick_model(g, &l, Some("  ")).as_deref(),
        Some("gemini-3.1-flash-lite"),
        "공백은 미지정"
    );
    // 기본 모델이 목록에 없으면 최상단
    assert_eq!(
        pick_model(g, &list(&["x", "y"]), None).as_deref(),
        Some("x")
    );
    // 기본 모델이 없는 제공자(Codex)도 최상단
    assert_eq!(
        pick_model(ProviderKind::Codex, &list(&["gpt-6.1-sol"]), None).as_deref(),
        Some("gpt-6.1-sol")
    );
    // 목록이 비면 기본 모델, 그것도 없으면 None
    assert_eq!(
        pick_model(g, &[], None).as_deref(),
        Some("gemini-3.1-flash-lite")
    );
    assert_eq!(pick_model(ProviderKind::Ollama, &[], None), None);
}

#[test]
fn model_choices_falls_back_on_error() {
    use kerbaloc_core::llm::model_choices;
    let rt = tokio::runtime::Runtime::new().unwrap();
    // 키 없는 Gemini → 조회 실패 → fallback 목록 + 사유, 기본 모델 선택
    let mc = rt.block_on(model_choices(&cfg(ProviderKind::Gemini, ""), None));
    assert!(mc.error.is_some());
    assert!(!mc.models.is_empty());
    assert_eq!(mc.selected.as_deref(), Some("gemini-3.1-flash-lite"));
    // Claude Code는 정적 별칭 목록
    let mc = rt.block_on(model_choices(&cfg(ProviderKind::ClaudeCode, ""), None));
    assert!(mc.error.is_none());
    assert_eq!(mc.selected.as_deref(), Some("sonnet"));
}

#[test]
fn catalog_is_consistent() {
    use kerbaloc_core::llm::{default_model, provider_catalog};
    let cat = provider_catalog();
    assert_eq!(cat.len(), ProviderKind::ALL.len());
    for info in &cat {
        let k: ProviderKind = info.id.parse().unwrap();
        assert_eq!(k.id(), info.id, "id 왕복");
        let d = default_model(k);
        if !d.is_empty() {
            assert!(
                info.fallback_models.contains(&d),
                "{}: 기본 모델은 fallback에 포함",
                info.id
            );
        }
        assert!(
            info.needs_api_key || info.cli_command.is_some() || info.default_base_url.is_some(),
            "{}: 접속 수단 필요",
            info.id
        );
    }
}

#[test]
fn openai_style_ids_sorted_newest_first() {
    use kerbaloc_core::llm::openai_style_ids;
    let v: serde_json::Value = serde_json::from_str(
        r#"{"data":[{"id":"old","created":1},{"id":"new","created":9},{"id":"mid","created":5}]}"#,
    )
    .unwrap();
    assert_eq!(openai_style_ids(&v), ["new", "mid", "old"]);
    // created 없으면(Anthropic) 원래 순서 유지
    let v: serde_json::Value = serde_json::from_str(r#"{"data":[{"id":"b"},{"id":"a"}]}"#).unwrap();
    assert_eq!(openai_style_ids(&v), ["b", "a"]);
}

#[test]
fn codex_models_by_priority_listed_only() {
    let json = r#"{"models":[
        {"slug":"gpt-6-astra","visibility":"list","priority":2},
        {"slug":"gpt-reserve","visibility":"hide","priority":4},
        {"slug":"gpt-6.1-sol","visibility":"list","priority":1}]}"#;
    assert_eq!(codex::parse_models(json), ["gpt-6.1-sol", "gpt-6-astra"]);
    assert!(codex::parse_models("not json").is_empty());
}
