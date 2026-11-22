use crate::validate::{validate_key_translation, Severity};
use crate::{cfg, loc, parts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct PackMeta {
    pub schema: String,
    pub lang: String,
    pub mod_id: String,
    pub variant_id: String,
    pub src_sha256: String,
    pub keys_translated: usize,
    pub keys_target: usize,
}

pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub(crate) fn safe_component(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !value.ends_with('.')
}

pub(crate) fn localization_cfg(lang: &str, entries: &BTreeMap<String, String>) -> String {
    cfg::serialize(&cfg::Node {
        name: String::new(),
        values: vec![],
        children: vec![cfg::Node {
            name: "Localization".into(),
            values: vec![],
            children: vec![cfg::Node {
                name: lang.into(),
                values: entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                children: vec![],
            }],
        }],
    })
}

pub(crate) fn validate_generated_part(
    mod_id: &str,
    key: &str,
    field: &parts::PartField,
) -> Result<(), String> {
    if !parts::is_safe_part_name(&field.part_name) {
        return Err(format!("{key}: 안전하지 않은 PART 대상 이름"));
    }
    if !matches!(field.field.as_str(), "title" | "description") {
        return Err(format!("{key}: PART title/description 이외의 필드"));
    }
    if key != parts::key(mod_id, &field.part_name, &field.field) {
        return Err(format!("{key}: PART 대상과 합성 키가 일치하지 않음"));
    }
    let source = &field.source;
    if source.trim().is_empty()
        || source.trim() != source.as_str()
        || source.starts_with('#')
        || source.starts_with('@')
        || source.chars().any(char::is_control)
        || source.contains(['{', '}'])
        || source.contains("//")
        || source.contains('$')
        || source.contains("@/")
    {
        return Err(format!("{key}: PART 영어 폴백 값이 안전한 리터럴 원문이 아님"));
    }
    Ok(())
}

pub(crate) fn generated_parts_cfg(fields: &BTreeMap<String, parts::PartField>) -> String {
    let mut by_part: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    for (key, field) in fields {
        by_part
            .entry(&field.part_name)
            .or_default()
            .insert(&field.field, key);
    }
    cfg::serialize(&cfg::Node {
        name: String::new(),
        values: vec![],
        children: by_part
            .into_iter()
            .map(|(part_name, values)| cfg::Node {
                name: format!("@PART[{part_name}]"),
                values: ["title", "description"]
                    .into_iter()
                    .filter_map(|field| {
                        values
                            .get(field)
                            .map(|key| (format!("@{field}"), (*key).to_string()))
                    })
                    .collect(),
                children: vec![],
            })
            .collect(),
    })
}

pub(crate) fn generated_cfg_errors(dir: &Path) -> Vec<String> {
    let mut errors = vec![];
    if !dir.exists() {
        return errors;
    }
    for item in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = match item {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!("생성 PART 팩 파일 검사 실패: {error}"));
                continue;
            }
        };
        if entry.file_type().is_symlink() {
            errors.push(format!("생성 PART 팩은 심볼릭 링크를 포함할 수 없음: {}", entry.path().display()));
        }
        if !entry.file_type().is_file()
            || !entry.path().extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("cfg"))
        {
            continue;
        }
        let relative = entry.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        if !matches!(relative.as_str(), "Localization/ko.cfg" | "Localization/en-us.cfg" | "Patches/parts.cfg") {
            errors.push(format!("생성 PART 팩에 허용되지 않은 추가 CFG: {relative}"));
        }
    }
    errors
}

fn validate_generated_files(
    dir: &Path,
    meta: &PackMeta,
    metadata: &serde_json::Value,
    ko_text: &str,
    entries: &BTreeMap<String, String>,
    source_entries: Option<&BTreeMap<String, String>>,
    report: &mut ValidationReport,
) {
    let patch_path = dir.join("Patches/parts.cfg");
    let fallback_path = dir.join("Localization/en-us.cfg");
    let Some(raw_fields) = metadata.get("part_fields") else {
        if patch_path.exists()
            || fallback_path.exists()
            || entries.keys().any(|key| key.starts_with("#KerbaLoc_Part_"))
        {
            report
                .errors
                .push("생성 PART 패치/폴백/합성 키에 part_fields 메타데이터가 없음".into());
        }
        return;
    };
    if meta.lang != "ko" {
        report.errors.push("생성 PART 팩의 대상 언어는 ko여야 함".into());
    }
    report.errors.extend(generated_cfg_errors(dir));
    // This extension is intentionally exact, while old pack.json fields stay compatible.
    let Some(objects) = raw_fields.as_object() else {
        report.errors.push("part_fields는 객체여야 함".into());
        return;
    };
    if objects.is_empty() {
        report.errors.push("part_fields가 비어 있음".into());
        return;
    }
    for (key, value) in objects {
        if value.as_object().is_none_or(|object| {
            object.len() != 3
                || !["part_name", "field", "source"]
                    .into_iter()
                    .all(|name| object.get(name).is_some_and(serde_json::Value::is_string))
        }) {
            report
                .errors
                .push(format!("{key}: part_fields의 필드는 part_name/field/source만 허용"));
        }
    }
    let fields: BTreeMap<String, parts::PartField> = match serde_json::from_value(raw_fields.clone()) {
        Ok(fields) => fields,
        Err(error) => {
            report.errors.push(format!("part_fields: {error}"));
            return;
        }
    };
    let mut valid_mapping = true;
    let mut fallback = BTreeMap::new();
    for (key, field) in &fields {
        if let Err(error) = validate_generated_part(&meta.mod_id, key, field) {
            report.errors.push(error);
            valid_mapping = false;
            continue;
        }
        fallback.insert(key.clone(), field.source.clone());
        match entries.get(key) {
            None => report.errors.push(format!("{key}: PART 패치에 대응하는 ko 번역 키가 없음")),
            Some(translated) => {
                for finding in validate_key_translation(key, &field.source, translated) {
                    if finding.severity == Severity::Error {
                        report.errors.push(format!("{key}: [{}] {}", finding.rule, finding.message));
                    }
                }
                if translated.chars().any(char::is_control)
                    || crate::tokens::extract_tokens(&field.source)
                        != crate::tokens::extract_tokens(translated)
                {
                    report.errors.push(format!("{key}: PART 번역의 제어 문자 또는 토큰 개수 불일치"));
                }
            }
        }
        if let Some(source) = source_entries {
            if source.get(key) != Some(&field.source) {
                report.errors.push(format!("{key}: PART 폴백/대상 매핑이 원문과 일치하지 않음"));
            }
        }
    }
    for key in entries.keys().filter(|key| key.starts_with("#KerbaLoc_Part_")) {
        if !fields.contains_key(key) {
            report.errors.push(format!("{key}: 합성 ko 키에 PART 대상/영어 폴백이 없음"));
        }
    }
    // Exact canonical generated files prohibit extra nodes, edits, selectors, references,
    // duplicate keys and dangling tokens that the tolerant ConfigNode parser may discard.
    if ko_text.replace("\r\n", "\n") != localization_cfg("ko", entries) {
        report.errors.push("생성 PART 팩의 ko.cfg는 ko 전용 표준 Localization 구조여야 함".into());
    }
    if valid_mapping {
        for (path, expected, label) in [
            (&patch_path, generated_parts_cfg(&fields), "Patches/parts.cfg"),
            (&fallback_path, localization_cfg("en-us", &fallback), "Localization/en-us.cfg"),
        ] {
            match std::fs::read_to_string(path) {
                Ok(actual) if actual.replace("\r\n", "\n") == expected => {}
                Ok(_) => report.errors.push(format!(
                    "{label}: 생성 PART 대상/키/필드/영어 폴백 불일치 또는 추가 구성 포함"
                )),
                Err(error) => report.errors.push(format!("{label}: {error}")),
            }
        }
    }
}

pub fn validate_pack(
    dir: &Path,
    source_entries: Option<&BTreeMap<String, String>>,
) -> ValidationReport {
    let mut r = ValidationReport {
        errors: vec![],
        warnings: vec![],
    };
    let metadata: serde_json::Value = match std::fs::read_to_string(dir.join("pack.json"))
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => {
            r.errors.push(format!("pack.json: {e}"));
            return r;
        }
    };
    let meta: PackMeta = match serde_json::from_value(metadata.clone()) {
        Ok(meta) => meta,
        Err(error) => {
            r.errors.push(format!("pack.json: {error}"));
            return r;
        }
    };
    if meta.schema != "kerbaloc/pack@1" {
        r.errors.push(format!("알 수 없는 스키마: {}", meta.schema));
    }
    if !safe_component(&meta.lang) || !safe_component(&meta.mod_id) {
        r.errors.push("pack.json: 안전하지 않은 언어 또는 모드 경로".into());
        return r;
    }
    let cfg_path = dir.join("Localization").join(format!("{}.cfg", meta.lang));
    let bytes = match std::fs::read(&cfg_path) {
        Ok(b) => b,
        Err(e) => {
            r.errors.push(format!("{}: {e}", cfg_path.display()));
            return r;
        }
    };
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        r.errors.push("BOM 있는 UTF-8 — 팩 파일은 BOM 없이".into());
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            r.errors.push(format!("cfg UTF-8 오류: {error}"));
            return r;
        }
    };
    // 파싱 전 원문 줄 검사: 값 자리의 원시 중괄호는 파싱 시 구조 문자로 흡수되어
    // 사라지므로(게임의 ConfigNode 리더도 동일) 여기서만 잡을 수 있다.
    for (i, line) in text.lines().enumerate() {
        let stripped = match line.find("//") {
            Some(c) => &line[..c],
            None => line,
        };
        if let Some((k, v)) = stripped.split_once('=') {
            if !k.trim().is_empty() && (v.contains('{') || v.contains('}')) {
                r.errors.push(format!(
                    "{}행 {}: 값에 원시 중괄호 — ｢｣로 이스케이프 필요",
                    i + 1,
                    k.trim()
                ));
            }
        }
    }
    let root = match cfg::parse(&text) {
        Ok(x) => x,
        Err(e) => {
            r.errors.push(format!("cfg 파싱 실패: {e}"));
            return r;
        }
    };
    match cfg::roundtrip_ok(&text) {
        Ok(true) => {}
        _ => r
            .errors
            .push("라운드트립 불일치 — 구조 문자 손상 의심".into()),
    }
    fn langs_in(n: &cfg::Node, out: &mut Vec<String>) {
        if n.name == "Localization" {
            for c in &n.children {
                out.push(c.name.clone());
            }
        }
        for c in &n.children {
            langs_in(c, out);
        }
    }
    let mut langs = vec![];
    langs_in(&root, &mut langs);
    for l in &langs {
        if l != &meta.lang {
            r.errors.push(format!(
                "{l} 노드 포함 — 팩은 {} 노드만 가질 수 있음",
                meta.lang
            ));
        }
    }
    let entries = loc::extract_localization(&root, &meta.lang);
    if entries.is_empty() {
        r.errors.push("번역 항목이 비어 있음".into());
    }
    validate_generated_files(dir, &meta, &metadata, &text, &entries, source_entries, &mut r);
    if let Some(src) = source_entries {
        let mut translated = 0;
        for (k, dst) in &entries {
            match src.get(k) {
                None => r.warnings.push(format!("{k}: 원문에 없는 키")),
                Some(s) => {
                    if s != dst {
                        translated += 1;
                    }
                    for f in validate_key_translation(k, s, dst) {
                        let msg = format!("{k}: [{}] {}", f.rule, f.message);
                        match f.severity {
                            Severity::Error => r.errors.push(msg),
                            Severity::Warning => r.warnings.push(msg),
                        }
                    }
                }
            }
        }
        if translated != meta.keys_translated {
            r.warnings.push(format!(
                "커버리지 재계산 {translated} ≠ 신고값 {}",
                meta.keys_translated
            ));
        }
    }
    r
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

/// GameData/KerbaLoc/<lang>/<ModId>/ 로 복사. 기존 설치는 교체.
pub fn install_pack(ksp_root: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    // Never remove a working installation before validating the complete replacement.
    let report = validate_pack(dir, None);
    if !report.errors.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("팩 검증 실패:\n{}", report.errors.join("\n")),
        ));
    }
    let metadata: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pack.json"))?)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if metadata.get("part_fields").is_some() && !has_module_manager(&ksp_root.join("GameData"))? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "PART 제목/설명 패치 팩은 GameData의 ModuleManager DLL이 필요합니다 (4.2.3 이상 권장). 먼저 ModuleManager를 설치해 주세요.",
        ));
    }
    let meta: PackMeta = serde_json::from_str(&std::fs::read_to_string(dir.join("pack.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let dest = ksp_root
        .join("GameData")
        .join("KerbaLoc")
        .join(&meta.lang)
        .join(&meta.mod_id);
    if dest.exists() && std::fs::canonicalize(dir)? == std::fs::canonicalize(&dest)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "원본 팩과 설치 경로가 같습니다. 기존 설치를 그대로 유지합니다.",
        ));
    }
    if dest.exists() && (!dest.is_dir() || dest.symlink_metadata()?.file_type().is_symlink()) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
            "설치 경로가 일반 디렉터리가 아닙니다. 기존 경로를 유지합니다."));
    }
    let parent = dest.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos();
    // Keep incomplete/new/backup CFG files outside GameData so a crash cannot load them.
    let workspace = ksp_root.join("KerbaLoc-staging").join(format!(
        "install-{}-{}-{nonce}", meta.mod_id, std::process::id()
    ));
    std::fs::create_dir_all(&workspace)?;
    let stage = workspace.join("pack");
    let backup = workspace.join("previous");
    std::fs::create_dir(&stage)?;
    if let Err(error) = copy_dir(dir, &stage) {
        let _ = std::fs::remove_dir_all(&workspace);
        return Err(error);
    }
    let copied_report = validate_pack(&stage, None);
    if !copied_report.errors.is_empty() {
        let _ = std::fs::remove_dir_all(&workspace);
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
            format!("복사한 팩 검증 실패:\n{}", copied_report.errors.join("\n"))));
    }
    let replacing = dest.exists();
    if replacing {
        if let Err(error) = std::fs::rename(&dest, &backup) {
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(error);
        }
    }
    if let Err(error) = std::fs::rename(&stage, &dest) {
        if replacing {
            if let Err(restore_error) = std::fs::rename(&backup, &dest) {
                return Err(std::io::Error::other(format!(
                    "팩 교체 실패: {error}; 복구 실패: {restore_error}; 기존 팩 위치: {}", backup.display()
                )));
            }
        }
        let _ = std::fs::remove_dir_all(&workspace);
        return Err(error);
    }
    std::fs::remove_dir_all(&workspace)?;
    Ok(dest)
}

fn has_module_manager(game_data: &Path) -> std::io::Result<bool> {
    let entries = match std::fs::read_dir(game_data) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let is_mm = name == "modulemanager.dll"
            || name
                .strip_prefix("modulemanager.")
                .and_then(|suffix| suffix.strip_suffix(".dll"))
                .is_some_and(|version| {
                    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.')
                });
        if is_mm && entry.path().is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remove_pack(ksp_root: &Path, lang: &str, mod_id: &str) -> std::io::Result<bool> {
    let dest = ksp_root
        .join("GameData")
        .join("KerbaLoc")
        .join(lang)
        .join(mod_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
