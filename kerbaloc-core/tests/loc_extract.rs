use kerbaloc_core::{cfg::parse, loc::extract_localization};

#[test]
fn extracts_lang_entries_including_nested() {
    let text = "Localization\n{\n\ten-us\n\t{\n\t\t#a = A\n\t\tsub\n\t\t{\n\t\t\t#b = B\n\t\t}\n\t}\n\tko\n\t{\n\t\t#a = 가\n\t}\n}\n";
    let root = parse(text).unwrap();
    let en = extract_localization(&root, "en-us");
    assert_eq!(en.get("#a").unwrap(), "A");
    assert_eq!(en.get("sub/#b").unwrap(), "B");
    let ko = extract_localization(&root, "ko");
    assert_eq!(ko.len(), 1);
}

#[test]
fn merges_multiple_localization_blocks() {
    // Tantares처럼 여러 파일/블록 분할 — 같은 root 아래 두 블록도 합집합
    let text = "Localization\n{\n\ten-us { #a = A }\n}\nLocalization\n{\n\ten-us { #b = B }\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(extract_localization(&root, "en-us").len(), 2);
}
