use kerbaloc_core::llm::Usage;

#[test]
fn usage_accumulates_and_prices() {
    let mut u = Usage {
        input_tokens: 1_000_000,
        output_tokens: 500_000,
    };
    u.add(&Usage {
        input_tokens: 1_000_000,
        output_tokens: 500_000,
    });
    assert_eq!(u.input_tokens, 2_000_000);
    // (2M / 1M) * $0.25 + (1M / 1M) * $1.50 = $2.00
    let cost = u.cost_usd(0.25, 1.50);
    assert!((cost - 2.0).abs() < 1e-9, "{cost}");
}
