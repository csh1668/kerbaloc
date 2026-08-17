use crate::cfg;
use crate::pack::{validate_pack, PackMeta};
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
    std::fs::create_dir_all(out_dir.join("Localization"))?;
    let mut ko = cfg::Node {
        name: "ko".into(),
        values: vec![],
        children: vec![],
    };
    for (k, v) in translations {
        ko.values.push((k.clone(), v.clone()));
    }
    let root = cfg::Node {
        name: String::new(),
        values: vec![],
        children: vec![cfg::Node {
            name: "Localization".into(),
            values: vec![],
            children: vec![ko],
        }],
    };
    std::fs::write(out_dir.join("Localization/ko.cfg"), cfg::serialize(&root))?;

    let meta = PackMeta {
        schema: "kerbaloc/pack@1".into(),
        lang: "ko".into(),
        mod_id: unit.mod_id.clone(),
        variant_id: variant_id.to_string(),
        src_sha256: unit.source_hash.clone(),
        keys_translated: translations.len(),
        keys_target: unit.entries.len(),
    };
    let mut meta_json = serde_json::to_value(&meta)?;
    meta_json["model"] = serde_json::Value::String(model_name.to_string());
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
