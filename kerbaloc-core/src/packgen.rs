use crate::cfg;
use crate::pack::{
    generated_cfg_errors, generated_parts_cfg, localization_cfg, safe_component,
    validate_generated_part, validate_pack, PackMeta,
};
use crate::scan::ModUnit;
use std::collections::BTreeMap;
use std::path::Path;

/// "YYYY-MM-DD-<method>-<nick>" (UTC). 닉은 ASCII 소문자+숫자 ≤16자, 없으면 anon.
pub fn make_variant_id(method_slug: &str, nick: &str) -> String {
    let nick: String = nick
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(16)
        .collect();
    let nick = if nick.is_empty() {
        "anon".to_string()
    } else {
        nick
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("시계 이상");
    let days = now.as_secs() / 86_400;
    // 1970-01-01 기준 civil date 변환 (Howard Hinnant 알고리즘)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}-{method_slug}-{nick}")
}

/// 번역 결과로 팩 디렉터리 생성 후 자체 검증. 오류 시 Err (디렉터리는 남겨 진단 가능).
pub fn build_pack(
    out_dir: &Path,
    unit: &ModUnit,
    translations: &BTreeMap<String, String>,
    variant_id: &str,
    model_name: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !translations.is_empty(),
        "번역 항목이 비어 있음 — 기존 팩을 유지합니다"
    );
    anyhow::ensure!(
        safe_component(&unit.mod_id),
        "안전하지 않은 모드 ID — 기존 팩을 유지합니다"
    );
    let mut emitted_parts = BTreeMap::new();
    for (key, translated) in translations {
        let source = unit
            .entries
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("{key}: 원문에 없는 번역 키"))?;
        anyhow::ensure!(
            !key.trim().is_empty()
                && key.trim() == key.as_str()
                && !key.chars().any(char::is_control)
                && !key.contains(['{', '}', '='])
                && !key.contains("//"),
            "{key:?}: 안전하지 않은 번역 키"
        );
        anyhow::ensure!(
            !translated.chars().any(char::is_control),
            "{key}: 번역 값에 제어 문자 포함"
        );
        for finding in crate::validate::validate_key_translation(key, source, translated) {
            anyhow::ensure!(
                finding.severity != crate::validate::Severity::Error,
                "{key}: [{}] {}",
                finding.rule,
                finding.message
            );
        }
        if let Some(field) = unit.part_fields.get(key) {
            validate_generated_part(&unit.mod_id, key, field).map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                source == &field.source,
                "{key}: PART 원문과 번역 대상 원문이 일치하지 않음"
            );
            anyhow::ensure!(
                crate::tokens::extract_tokens(source) == crate::tokens::extract_tokens(translated),
                "{key}: PART 번역의 엔진 토큰 또는 토큰 개수가 일치하지 않음"
            );
            emitted_parts.insert(key.clone(), field.clone());
        } else {
            anyhow::ensure!(
                !key.starts_with("#KerbaLoc_Part_"),
                "{key}: 생성 PART 키의 대상 메타데이터가 없음"
            );
        }
    }
    let ko_text = localization_cfg("ko", translations);
    anyhow::ensure!(
        crate::loc::extract_localization(&cfg::parse(&ko_text)?, "ko") == *translations,
        "번역 cfg 라운드트립 불일치"
    );
    if !emitted_parts.is_empty() {
        let errors = generated_cfg_errors(out_dir);
        anyhow::ensure!(errors.is_empty(), "{}", errors.join("\n"));
    }
    std::fs::create_dir_all(out_dir.join("Localization"))?;
    std::fs::write(out_dir.join("Localization/ko.cfg"), ko_text)?;

    if emitted_parts.is_empty() {
        // Only files owned by this generator are removed when rebuilding a tag-only pack.
        for relative in ["Patches/parts.cfg", "Localization/en-us.cfg"] {
            match std::fs::remove_file(out_dir.join(relative)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    } else {
        std::fs::create_dir_all(out_dir.join("Patches"))?;
        std::fs::write(
            out_dir.join("Patches/parts.cfg"),
            generated_parts_cfg(&emitted_parts),
        )?;
        let fallback = emitted_parts
            .iter()
            .map(|(key, field)| (key.clone(), field.source.clone()))
            .collect();
        std::fs::write(
            out_dir.join("Localization/en-us.cfg"),
            localization_cfg("en-us", &fallback),
        )?;
    }

    let meta = PackMeta {
        schema: "kerbaloc/pack@1".into(),
        lang: "ko".into(),
        mod_id: unit.mod_id.clone(),
        variant_id: variant_id.to_string(),
        src_sha256: unit.source_hash.clone(),
        keys_translated: translations
            .iter()
            .filter(|(key, value)| unit.entries.get(*key) != Some(*value))
            .count(),
        keys_target: unit.entries.len(),
    };
    let mut meta_json = serde_json::to_value(&meta)?;
    meta_json["model"] = serde_json::Value::String(model_name.to_string());
    if !emitted_parts.is_empty() {
        meta_json["part_fields"] = serde_json::to_value(&emitted_parts)?;
    }
    std::fs::write(
        out_dir.join("pack.json"),
        serde_json::to_string_pretty(&meta_json)?,
    )?;

    let r = validate_pack(out_dir, Some(&unit.entries));
    anyhow::ensure!(
        r.errors.is_empty(),
        "생성된 팩이 자체 검증 실패:\n{}",
        r.errors.join("\n")
    );
    Ok(())
}
