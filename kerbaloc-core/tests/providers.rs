use kerbaloc_core::llm::{
    anthropic, claude_code, default_prices, openai_compat, parse_items_text, ProviderKind,
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
