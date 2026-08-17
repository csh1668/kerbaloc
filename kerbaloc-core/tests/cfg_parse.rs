use kerbaloc_core::cfg::{parse, Node};

fn loc(root: &Node) -> &Node {
    &root.children[0]
}

#[test]
fn parses_basic_localization_block() {
    let text = "Localization\n{\n\ten-us\n\t{\n\t\t#autoLOC_1 = Hello <<1>>\n\t}\n}\n";
    let root = parse(text).unwrap();
    let l = loc(&root);
    assert_eq!(l.name, "Localization");
    assert_eq!(l.children[0].name, "en-us");
    assert_eq!(
        l.children[0].values[0],
        ("#autoLOC_1".to_string(), "Hello <<1>>".to_string())
    );
}

#[test]
fn strips_inline_comment_after_node_name() {
    // 실존 사례: 구 커뮤니티 패치 dictionary.cfg의 "en-us// 주석"
    let text = "Localization\n{\n\ten-us// legacy patch 24.06.15\n\t{\n\t\t#a = b\n\t}\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(loc(&root).children[0].name, "en-us");
}

#[test]
fn truncates_value_at_comment() {
    let text = "N\n{\n\tkey = value // trailing\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(root.children[0].values[0].1, "value");
}

#[test]
fn inline_open_brace_on_name_line() {
    let text = "PART { name = fuelTank\n\ttitle = Tank\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(root.children[0].name, "PART");
    assert_eq!(root.children[0].values.len(), 2);
}

#[test]
fn value_may_contain_equals() {
    let text = "N\n{\n\tk = a = b\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(
        root.children[0].values[0],
        ("k".to_string(), "a = b".to_string())
    );
}

#[test]
fn strips_utf8_bom() {
    let text = "\u{feff}N\n{\n\tk = v\n}\n";
    assert!(parse(text).is_ok());
}

#[test]
fn unbalanced_braces_is_error() {
    assert!(parse("N\n{\n\tk = v\n").is_err());
}

#[test]
fn golden_stock_en_us_parses() {
    // research/stock-dictionary/en-us.cfg — 스톡 원본.
    // 값 줄 12,034개 (정규식 실측; 중복 키 4종 포함, 고유 키 12,030).
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../research/stock-dictionary/en-us.cfg"
    ))
    .unwrap();
    let root = parse(&text).unwrap();
    let en = &root.children[0].children[0];
    assert_eq!(en.name, "en-us");
    assert_eq!(en.values.len(), 12034);
}
