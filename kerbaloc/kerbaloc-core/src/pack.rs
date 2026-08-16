use crate::validate::{validate_translation, Severity};
use crate::{cfg, loc};
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

pub fn validate_pack(
    dir: &Path,
    source_entries: Option<&BTreeMap<String, String>>,
) -> ValidationReport {
    let mut r = ValidationReport {
        errors: vec![],
        warnings: vec![],
    };
    let meta: PackMeta = match std::fs::read_to_string(dir.join("pack.json"))
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => {
            r.errors.push(format!("pack.json: {e}"));
            return r;
        }
    };
    if meta.schema != "kerbaloc/pack@1" {
        r.errors.push(format!("알 수 없는 스키마: {}", meta.schema));
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
    let text = String::from_utf8_lossy(&bytes);
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
    if let Some(src) = source_entries {
        let mut translated = 0;
        for (k, dst) in &entries {
            match src.get(k) {
                None => r.warnings.push(format!("{k}: 원문에 없는 키")),
                Some(s) => {
                    if s != dst {
                        translated += 1;
                    }
                    for f in validate_translation(s, dst) {
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
    let meta: PackMeta = serde_json::from_str(&std::fs::read_to_string(dir.join("pack.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let dest = ksp_root
        .join("GameData")
        .join("KerbaLoc")
        .join(&meta.lang)
        .join(&meta.mod_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    copy_dir(dir, &dest)?;
    Ok(dest)
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
