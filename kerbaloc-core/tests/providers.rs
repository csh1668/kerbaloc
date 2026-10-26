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
