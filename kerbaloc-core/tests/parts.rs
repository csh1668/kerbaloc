use kerbaloc_core::{cfg, parts};

fn part_with_value(value: &str) -> cfg::Node {
    cfg::Node {
        name: String::new(),
        values: vec![],
        children: vec![cfg::Node {
            name: "PART".into(),
            values: vec![
                ("name".into(), "literal.part-1_2".into()),
                ("title".into(), value.into()),
            ],
            children: vec![],
        }],
    }
}

#[test]
fn keys_use_full_digest_and_distinguish_identity_components() {
    let key = parts::key("Mod", "engine", "title");
    let digest = key
        .strip_prefix("#KerbaLoc_Part_")
        .unwrap()
        .strip_suffix("_title")
        .unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest
        .bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    assert_ne!(key, parts::key("OtherMod", "engine", "title"));
    assert_ne!(key, parts::key("Mod", "Engine", "title"));
    assert_ne!(key, parts::key("Mod", "engine", "description"));
    assert_ne!(
        parts::key("ab", "c", "title"),
        parts::key("a", "bc", "title")
    );
}

#[test]
fn part_names_are_literal_ascii_selectors() {
    for name in ["engine", "Engine_2-1.25", "0", "_adapter", "rocket.a.b"] {
        assert!(parts::is_safe_part_name(name), "{name:?}");
    }
    for name in [
        "",
        " engine",
        "engine ",
        "a b",
        "a,b",
        "a*",
        "a?",
        "a[b]",
        "a/b",
        "a\\b",
        "@a",
        "$a",
        "#LOC_a",
        "a:NEEDS[b]",
        "a|b",
        "한글",
        "a\0b",
        "a\nb",
    ] {
        assert!(!parts::is_safe_part_name(name), "{name:?}");
    }
}

#[test]
fn only_plain_top_level_part_display_fields_are_extracted() {
    let root = cfg::parse(
        r#"
PART
{
    name = main.engine
    title = Main Engine
    description = A <b>compact</b> engine.\nReady for flight.
    manufacturer = Untouched Company
    MODULE
    {
        name = EngineModule
        title = Internal module title
        description = Internal module description
    }
}
@PART[main.engine]
{
    name = patch
    title = Patched title
}
+PART[main.engine]
{
    name = copy
    description = Copy description
}
PART:NEEDS[SomeMod]
{
    name = conditional
    title = Conditional title
}
WRAPPER
{
    PART
    {
        name = nested
        title = Nested title
    }
}
"#,
    )
    .unwrap();
    let fields = parts::extract(&root);
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].part_name, "main.engine");
    assert_eq!(fields[0].field, "title");
    assert_eq!(fields[0].source, "Main Engine");
    assert_eq!(fields[1].field, "description");
    assert_eq!(
        fields[1].source,
        "A <b>compact</b> engine.\\nReady for flight."
    );
}

#[test]
fn tagged_dynamic_and_duplicate_fields_are_skipped_independently() {
    let root = cfg::parse(
        r#"
PART
{
    name = tagged
    title = #LOC_tagged_title
    description = Plain description
}
PART
{
    name = repeated-field
    title = First title
    title = Second title
    description = Unique description
}
PART
{
    name = dynamic
    title = #$../title$
    description = $someVariable$
}
PART
{
    name = repeated-name
    name = repeated-name
    title = Ambiguous part name
}
PART
{
    name = unsafe*selector
    title = Unsafe selector
}
"#,
    )
    .unwrap();
    let fields = parts::extract(&root);
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.field == "description"));
    assert_eq!(fields[0].part_name, "tagged");
    assert_eq!(fields[1].part_name, "repeated-field");
}

#[test]
fn unsafe_cfg_values_and_controls_are_not_translation_input() {
    for value in [
        "",
        "   ",
        "#LOC_title",
        " #LOC_title",
        "#$../title$",
        "$variable$",
        "@/title",
        "value @/target",
        "value { body }",
        "value // comment",
        "two\nlines",
        "two\rlines",
        "tab\tvalue",
        "nul\0value",
        "escape\u{1b}value",
    ] {
        assert!(
            parts::extract(&part_with_value(value)).is_empty(),
            "{value:?}"
        );
    }
    assert_eq!(
        parts::extract(&part_with_value(
            "Literal \\n and <color=#ffffff>white</color>"
        ))[0]
            .source,
        "Literal \\n and <color=#ffffff>white</color>"
    );
}

#[test]
fn part_field_metadata_roundtrips_without_changing_source() {
    let field = parts::PartField {
        part_name: "rocket-1.25".into(),
        field: "description".into(),
        source: "A pressure-fed engine.\\nKeep upright.".into(),
    };
    let json = serde_json::to_string(&field).unwrap();
    assert_eq!(
        serde_json::from_str::<parts::PartField>(&json).unwrap(),
        field
    );
}
