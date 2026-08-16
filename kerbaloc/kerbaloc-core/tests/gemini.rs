use kerbaloc_core::llm::gemini::parse_response;

#[test]
fn parses_normal_response() {
    let json = r#"{
      "candidates": [{"content": {"parts": [{"text": "[{\"i\":1,\"ko\":\"안녕\"},{\"i\":2,\"ko\":\"세계 <<1>>\"}]"}]}}],
      "usageMetadata": {"promptTokenCount": 120, "candidatesTokenCount": 30}
    }"#;
    let (items, usage) = parse_response(json).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].i, 2);
    assert_eq!(items[1].ko, "세계 <<1>>");
    assert_eq!(usage.input_tokens, 120);
    assert_eq!(usage.output_tokens, 30);
}

#[test]
fn missing_candidates_is_error() {
    assert!(parse_response(r#"{"usageMetadata":{}}"#).is_err());
}

#[test]
fn non_json_text_part_is_error() {
    let json =
        r#"{"candidates": [{"content": {"parts": [{"text": "I cannot translate this."}]}}]}"#;
    assert!(parse_response(json).is_err());
}
