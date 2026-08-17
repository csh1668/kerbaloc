use kerbaloc_core::llm::gemini::parse_response;

#[test]
fn parses_normal_response() {
    let json = r#"{
      "candidates": [{"content": {"parts": [{"text": "[{\"i\":1,\"ko\":\"안녕\"},{\"i\":2,\"ko\":\"세계 <<1>>\"}]"}]}}],
      "usageMetadata": {"promptTokenCount": 120, "candidatesTokenCount": 30}
    }"#;
    let (items, usage) = parse_response(json);
    let items = items.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].i, 2);
    assert_eq!(items[1].ko, "세계 <<1>>");
    assert_eq!(usage.input_tokens, 120);
    assert_eq!(usage.output_tokens, 30);
}

#[test]
fn missing_candidates_is_error_but_usage_kept() {
    let (items, usage) =
        parse_response(r#"{"usageMetadata":{"promptTokenCount": 55, "candidatesTokenCount": 3}}"#);
    assert!(items.is_err());
    assert_eq!(usage.input_tokens, 55, "파싱 실패해도 청구된 토큰은 집계");
    assert_eq!(usage.output_tokens, 3);
}

#[test]
fn non_json_text_part_is_error_but_usage_kept() {
    let json = r#"{
      "candidates": [{"content": {"parts": [{"text": "I cannot translate this."}]}}],
      "usageMetadata": {"promptTokenCount": 80, "candidatesTokenCount": 12}
    }"#;
    let (items, usage) = parse_response(json);
    assert!(items.is_err());
    assert_eq!(usage.input_tokens, 80);
    assert_eq!(usage.output_tokens, 12);
}
