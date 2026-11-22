use kerbaloc_core::scan::{scan_gamedata, slug};
use kerbaloc_core::{hash, parts};
use std::collections::BTreeMap;
use std::fs;

fn write(p: &std::path::Path, rel: &str, content: &str) {
    let f = p.join(rel);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(f, content).unwrap();
}

const LOC: &str = "Localization\n{\n\ten-us\n\t{\n\t\t#LOC_A_x = Hello\n\t}\n\tja\n\t{\n\t\t#LOC_A_x = こんにちは\n\t}\n}\n";

fn make_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    // CKAN 소유 모드 (폴더명 ≠ identifier)
    write(d.path(), "GameData/000_Toolbar/Localization/en-us.cfg", LOC);
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(
        d.path().join("CKAN/registry.json"),
        r#"{
      "installed_modules": {"Toolbar": {"source_module": {"identifier":"Toolbar","name":"Toolbar Continued","version":"1.8.1.2","localizations":["en-us","ja"]}}},
      "installed_files": {"GameData/000_Toolbar/Localization/en-us.cfg": "Toolbar"}
    }"#,
    )
    .unwrap();
    // 수동 설치 + .version(GITHUB) — .version이 별도 하위 폴더에 있는 실측 패턴
    write(d.path(), "GameData/MyMod/Lang/strings.cfg", LOC);
    write(
        d.path(),
        "GameData/MyMod/Versioning/MyMod.version",
        r#"{"NAME":"MyMod","VERSION":"2.0.0","GITHUB":{"USERNAME":"u","REPOSITORY":"CoolMod"}}"#,
    );
    // 스톡
    write(d.path(), "GameData/Squad/Localization/dictionary.cfg", LOC);
    // .version 없는 수동 설치 (폴더명 폴백)
    write(d.path(), "GameData/zzz_Plain Mod/en-us.cfg", LOC);
    // 우리 산출물(제외 대상)
    write(
        d.path(),
        "GameData/KerbaLoc/ko/X/Localization/ko.cfg",
        "Localization\n{\n\tko\n\t{\n\t\t#a = 가\n\t}\n}\n",
    );
    d
}

#[test]
fn resolves_modids_by_priority() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let ids: Vec<&str> = units.iter().map(|u| u.mod_id.as_str()).collect();
    assert!(ids.contains(&"Toolbar"), "{ids:?}"); // CKAN identifier
    assert!(ids.contains(&"local.CoolMod"), "{ids:?}"); // AVC GITHUB repo
    assert!(ids.contains(&"Squad"), "{ids:?}"); // 예약 스톡
    assert!(ids.contains(&"local.Plain-Mod"), "{ids:?}"); // 폴더명 slug 폴백
    assert!(!ids.iter().any(|i| i.contains("KerbaLoc")), "{ids:?}"); // 자기 산출물 제외
}

#[test]
fn unit_carries_hash_version_and_official_langs() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let t = units.iter().find(|u| u.mod_id == "Toolbar").unwrap();
    assert!(t.source_hash.starts_with("v1:sha256:"));
    assert_eq!(t.version.raw.as_deref(), Some("1.8.1.2"));
    assert_eq!(t.version.source, "ckan");
    assert!(t.official_langs.contains(&"ja".to_string()));
    assert_eq!(t.entries.len(), 1);
    assert!(t.part_fields.is_empty());
    let tag_only = BTreeMap::from([("#LOC_A_x".to_string(), "Hello".to_string())]);
    assert_eq!(t.source_hash, hash::source_hash(&tag_only));
    assert_eq!(t.keys_hash, hash::keys_hash(&tag_only));

    let m = units.iter().find(|u| u.mod_id == "local.CoolMod").unwrap();
    assert_eq!(m.version.raw.as_deref(), Some("2.0.0"));
    assert_eq!(m.version.source, "avc");
}

#[test]
fn slug_rules() {
    assert_eq!(slug("000_Toolbar"), "Toolbar");
    assert_eq!(slug("zzz_Deferred"), "Deferred");
    assert_eq!(slug("Adiri's TUFX Profiles"), "Adiris-TUFX-Profiles");
    assert_eq!(slug("[x]_Science!"), "x-Science");
}

#[test]
fn explicit_modulemanager_renames_also_block_ambiguous_targets() {
    for header in [
        "+PART[other]",
        "$PART[other]",
        "@PART[other]",
        "%PART[other]",
    ] {
        for rename in ["@name", "%name"] {
            let d = tempfile::tempdir().unwrap();
            write(
                d.path(),
                "GameData/A/part.cfg",
                "PART\n{\nname = target\ntitle = Original\n}\n",
            );
            write(
                d.path(),
                "GameData/B/patch.cfg",
                &format!("{header}\n{{\n{rename} = target\n}}\n"),
            );
            assert!(scan_gamedata(d.path()).is_empty(), "{header} with {rename}");
        }
    }
}

#[test]
fn part_only_mod_is_owned_and_hashed_with_its_synthetic_entries() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "GameData/ActualFolder/engine.cfg",
        "PART\n{\n name = engine-1.25\n title = Small Engine\n description = A reliable engine.\n}\n",
    );
    write(
        d.path(),
        "CKAN/registry.json",
        r#"{
          "installed_modules": {"EnginePack": {"source_module": {"identifier":"EnginePack","name":"Engine Pack","version":"1.2.3"}}},
          "installed_files": {"GameData/ActualFolder/engine.cfg": "EnginePack"}
        }"#,
    );
    let units = scan_gamedata(d.path());
    assert_eq!(units.len(), 1);
    let unit = &units[0];
    assert_eq!(unit.mod_id, "EnginePack");
    assert_eq!(unit.version.raw.as_deref(), Some("1.2.3"));
    assert_eq!(unit.part_fields.len(), 2);
    assert_eq!(unit.entries.len(), 2);
    let title = parts::key("EnginePack", "engine-1.25", "title");
    assert_eq!(unit.entries[&title], "Small Engine");
    assert_eq!(unit.part_fields[&title].part_name, "engine-1.25");
    assert_eq!(unit.part_fields[&title].source, "Small Engine");
    assert_eq!(unit.source_hash, hash::source_hash(&unit.entries));
    assert_eq!(unit.keys_hash, hash::keys_hash(&unit.entries));
    assert_eq!(
        unit.files,
        vec![d.path().join("GameData/ActualFolder/engine.cfg")]
    );
}

#[test]
fn mixed_files_keep_real_tags_and_only_plain_display_fields() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "GameData/Mixed/part.cfg",
        &format!(
            "{LOC}\nPART\n{{\n name = tagged-title\n title = #LOC_A_x\n description = A plain description.\n MODULE\n {{\n name = Internal\n title = Not a PART title\n }}\n}}\n"
        ),
    );
    let units = scan_gamedata(d.path());
    let unit = &units[0];
    assert_eq!(unit.part_fields.len(), 1);
    assert_eq!(unit.entries["#LOC_A_x"], "Hello");
    assert_eq!(
        unit.entries[&parts::key("local.Mixed", "tagged-title", "description")],
        "A plain description."
    );
    assert_eq!(unit.entries.len(), 2);
    assert_eq!(unit.files.len(), 1);
    assert!(unit.official_langs.contains(&"ja".to_string()));
}

#[test]
fn duplicate_names_across_mods_block_fields_even_when_other_part_is_tagged() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "GameData/A/part.cfg",
        &format!("{LOC}\nPART\n{{\n name = shared\n title = Unsafe duplicate target\n}}\nPART\n{{\n name = unique\n title = Safe unique target\n}}\n"),
    );
    write(
        d.path(),
        "GameData/B/part.cfg",
        "PART\n{\n name = shared\n title = #LOC_AlreadyLocalized\n}\n",
    );
    let units = scan_gamedata(d.path());
    let unit = units.iter().find(|unit| unit.mod_id == "local.A").unwrap();
    assert_eq!(unit.part_fields.len(), 1);
    assert_eq!(
        unit.part_fields.values().next().unwrap().part_name,
        "unique"
    );
    assert!(!unit
        .entries
        .contains_key(&parts::key("local.A", "shared", "title")));
    assert_eq!(unit.entries["#LOC_A_x"], "Hello");
}

#[test]
fn duplicate_definitions_across_files_and_within_file_produce_no_targets() {
    let d = tempfile::tempdir().unwrap();
    for relative in ["GameData/A/a.cfg", "GameData/A/z.cfg"] {
        write(
            d.path(),
            relative,
            "PART\n{\n name = duplicate\n title = Title\n}\n",
        );
    }
    write(
        d.path(),
        "GameData/B/b.cfg",
        "PART\n{\n name = twice\n title = First\n}\nPART\n{\n name = twice\n title = Second\n}\n",
    );
    assert!(scan_gamedata(d.path()).is_empty());
}

#[test]
fn conditional_and_named_copy_declarations_make_plain_targets_ambiguous() {
    for alternate in ["PART:NEEDS[OtherMod]", "+PART[original]", "$PART[original]"] {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "GameData/A/plain.cfg",
            "PART\n{\n name = shared\n title = Plain title\n}\n",
        );
        write(
            d.path(),
            "GameData/B/alternate.cfg",
            &format!("{alternate}\n{{\n name = shared\n title = Alternate title\n}}\n"),
        );
        assert!(scan_gamedata(d.path()).is_empty(), "{alternate}");
    }
}

#[test]
fn real_keys_in_any_language_take_precedence_over_synthetic_keys() {
    let d = tempfile::tempdir().unwrap();
    let collision = parts::key("local.A", "engine", "title");
    write(
        d.path(),
        "GameData/A/part.cfg",
        &format!("{LOC}\nPART\n{{\n name = engine\n title = Plain title\n}}\n"),
    );
    write(
        d.path(),
        "GameData/B/ko.cfg",
        &format!("Localization\n{{\n ko\n {{\n {collision} = 기존 키\n }}\n}}\n"),
    );
    let units = scan_gamedata(d.path());
    let unit = units.iter().find(|unit| unit.mod_id == "local.A").unwrap();
    assert!(unit.part_fields.is_empty());
    assert_eq!(unit.entries.len(), 1);
    assert_eq!(unit.entries["#LOC_A_x"], "Hello");
}

#[test]
fn hashes_and_overwrite_order_do_not_depend_on_file_creation_order() {
    fn populate(root: &std::path::Path, reversed: bool) {
        let files = [("a.cfg", "first", "Alpha"), ("z.cfg", "second", "Zulu")];
        let order = if reversed { [1, 0] } else { [0, 1] };
        for index in order {
            let (file, part_name, text) = files[index];
            write(
                root,
                &format!("GameData/Stable/{file}"),
                &format!("PART\n{{\n name = {part_name}\n title = {text}\n}}\nLocalization\n{{\n en-us\n {{\n #LOC_duplicate = {text}\n }}\n}}\n"),
            );
        }
    }
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    populate(left.path(), false);
    populate(right.path(), true);
    let a = scan_gamedata(left.path()).remove(0);
    let b = scan_gamedata(right.path()).remove(0);
    assert_eq!(a.entries["#LOC_duplicate"], "Zulu");
    assert_eq!(a.entries, b.entries);
    assert_eq!(a.part_fields, b.part_fields);
    assert_eq!(a.source_hash, b.source_hash);
    assert_eq!(a.keys_hash, b.keys_hash);
    let names: Vec<_> = a
        .files
        .iter()
        .map(|path| path.file_name().unwrap())
        .collect();
    assert_eq!(names, vec!["a.cfg", "z.cfg"]);
}

#[test]
fn kerbaloc_output_is_excluded_from_name_and_key_collision_inventory() {
    let d = tempfile::tempdir().unwrap();
    let key = parts::key("local.A", "engine", "title");
    write(
        d.path(),
        "GameData/A/part.cfg",
        "PART\n{\n name = engine\n title = Plain title\n}\n",
    );
    write(
        d.path(),
        "GameData/kerbaloc/ko/output.cfg",
        &format!("PART\n{{\n name = engine\n title = Generated duplicate\n}}\nLocalization\n{{\n en-us\n {{\n {key} = Generated key\n }}\n}}\n"),
    );
    let units = scan_gamedata(d.path());
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].part_fields.len(), 1);
    assert_eq!(units[0].entries[&key], "Plain title");
}

#[test]
fn cached_avc_metadata_preserves_nested_ownership_and_sorted_fallbacks() {
    let d = tempfile::tempdir().unwrap();
    // Create tied candidates in reverse order: lexical paths, not creation
    // order, choose the fallback and the deepest ancestor's version metadata.
    for (relative, name, repo, version) in [
        ("00Versioning/Z.version", "Wrong Fallback", "WrongFallback", "9.0"),
        ("00Versioning/A.version", "Fallback", "FallbackRepo", "1.0"),
        (
            "Engines/Experimental/Z.version",
            "Wrong Experimental",
            "WrongExperimental",
            "9.0",
        ),
        (
            "Engines/Experimental/A.version",
            "Experimental",
            "ExperimentalRepo",
            "3.0",
        ),
        ("Engines/Engines.version", "Engines", "EnginesRepo", "2.0"),
    ] {
        write(
            d.path(),
            &format!("GameData/Multi/{relative}"),
            &format!(
                r#"{{"NAME":"{name}","VERSION":"{version}","GITHUB":{{"USERNAME":"author","REPOSITORY":"{repo}"}}}}"#
            ),
        );
    }
    write(d.path(), "GameData/Multi/00Versioning/0-invalid.version", "not JSON");
    for (relative, name) in [
        ("Engines/a.cfg", "engine-a"),
        ("Engines/b.cfg", "engine-b"),
        ("Engines/Experimental/c.cfg", "experimental-c"),
        ("Engines/Experimental/d.cfg", "experimental-d"),
        ("Other/e.cfg", "fallback-e"),
        ("Other/f.cfg", "fallback-f"),
    ] {
        write(
            d.path(),
            &format!("GameData/Multi/{relative}"),
            &format!("PART\n{{\n name = {name}\n title = Literal {name}\n}}\n"),
        );
    }
    let units = scan_gamedata(d.path());
    assert_eq!(units.len(), 3);
    for (id, version, expected_names) in [
        ("local.EnginesRepo", "2.0", ["engine-a", "engine-b"]),
        (
            "local.ExperimentalRepo",
            "3.0",
            ["experimental-c", "experimental-d"],
        ),
        ("local.FallbackRepo", "1.0", ["fallback-e", "fallback-f"]),
    ] {
        let unit = units.iter().find(|unit| unit.mod_id == id).unwrap();
        assert_eq!(unit.version.raw.as_deref(), Some(version));
        assert_eq!(unit.version.source, "avc");
        assert_eq!(unit.files.len(), 2);
        assert_eq!(unit.part_fields.len(), 2);
        for name in expected_names {
            let key = parts::key(id, name, "title");
            assert_eq!(unit.part_fields[&key].part_name, name);
            assert_eq!(unit.entries[&key], format!("Literal {name}"));
        }
        assert_eq!(unit.source_hash, hash::source_hash(&unit.entries));
    }
}
