use kerbaloc_core::cfg::{parse, roundtrip_ok, serialize};

#[test]
fn serialize_then_reparse_equals() {
    let text = "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕 <<1>>\n\t\t#b = 줄\\n바꿈\n\t}\n}\n";
    let root = parse(text).unwrap();
    let out = serialize(&root);
    assert_eq!(parse(&out).unwrap(), root);
}

#[test]
fn roundtrip_ok_true_for_valid() {
    assert!(roundtrip_ok("N\n{\n\tk = v\n}\n").unwrap());
}

#[test]
fn golden_stock_roundtrips() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../research/stock-dictionary/en-us.cfg"
    ))
    .unwrap();
    assert!(roundtrip_ok(&text).unwrap());
}
