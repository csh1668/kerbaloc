//! kerbaloc-db 레포 조작: 인덱스 생성 + 전체 검증.
//! CI가 `kerbaloc db index`/`db validate`로 동일 로직을 사용한다 (로직 단일화).
//! v1 범위 외(부록 E): source.sig.json, mask.json, 팩별 zip, 태그 충돌 전역 검사.

use crate::pack::{self, PackMeta, ValidationReport};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn variant_dirs(repo_dir: &Path) -> Vec<(String, String, std::path::PathBuf)> {
    // (mod_id, variant_id, dir) — 정렬됨
    let mut out = vec![];
    let packs = repo_dir.join("packs");
    let Ok(mods) = std::fs::read_dir(&packs) else {
        return out;
    };
    let mut mods: Vec<_> = mods.filter_map(Result::ok).collect();
    mods.sort_by_key(|e| e.file_name());
    for m in mods {
        if !m.path().is_dir() {
            continue;
        }
        let vdir = m.path().join("ko").join("variants");
        let Ok(vs) = std::fs::read_dir(&vdir) else {
            continue;
        };
        let mut vs: Vec<_> = vs.filter_map(Result::ok).collect();
        vs.sort_by_key(|e| e.file_name());
        for v in vs {
            if v.path().is_dir() {
                out.push((
                    m.file_name().to_string_lossy().to_string(),
                    v.file_name().to_string_lossy().to_string(),
                    v.path(),
                ));
            }
        }
    }
    out
}

fn variant_files(dir: &Path) -> anyhow::Result<Vec<Value>> {
    // pack.json + Localization/*.cfg + Patches/*.cfg — 경로 정렬
    let mut rels: Vec<String> = vec!["pack.json".into()];
    for sub in ["Localization", "Patches"] {
        if let Ok(rd) = std::fs::read_dir(dir.join(sub)) {
            for e in rd.filter_map(Result::ok) {
                rels.push(format!("{sub}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    rels.sort();
    let mut out = vec![];
    for rel in rels {
        let bytes = std::fs::read(dir.join(&rel))?;
        out.push(json!({
            "path": rel,
            "sha256": sha256_hex(&bytes),
            "sizeB": bytes.len(),
        }));
    }
    Ok(out)
}

/// 결정적 index/ko.json 생성 (타임스탬프 없음).
pub fn build_index(repo_dir: &Path) -> anyhow::Result<Value> {
    let mut packs: Vec<Value> = vec![];
    let mut cur_mod: Option<(String, Vec<Value>)> = None;
    for (mod_id, variant_id, dir) in variant_dirs(repo_dir) {
        let meta: PackMeta =
            serde_json::from_str(&std::fs::read_to_string(dir.join("pack.json"))?)?;
        let rel_path = format!("packs/{mod_id}/ko/variants/{variant_id}/");
        let v = json!({
            "variantId": variant_id,
            "path": rel_path,
            "srcSha256": meta.src_sha256,
            "keysTranslated": meta.keys_translated,
            "keysTarget": meta.keys_target,
            "model": serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join("pack.json"))?)
                .ok().and_then(|j| j.get("model").cloned()).unwrap_or(Value::Null),
            "files": variant_files(&dir)?,
        });
        match &mut cur_mod {
            Some((id, vs)) if *id == mod_id => vs.push(v),
            _ => {
                if let Some((id, vs)) = cur_mod.take() {
                    packs.push(json!({"modId": id, "variants": vs}));
                }
                cur_mod = Some((mod_id, vec![v]));
            }
        }
    }
    if let Some((id, vs)) = cur_mod {
        packs.push(json!({"modId": id, "variants": vs}));
    }
    Ok(json!({
        "schema": "kerbaloc/index@1",
        "lang": "ko",
        "packs": packs,
    }))
}

/// 레포 내 모든 변형 검증. (원문 없이 구조 검사만 — 토큰 검사는 클라이언트 설치 시 수행)
pub fn validate_repo(repo_dir: &Path) -> ValidationReport {
    let mut r = ValidationReport {
        errors: vec![],
        warnings: vec![],
    };
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}-[a-z0-9-]+$").unwrap();
    let variants = variant_dirs(repo_dir);
    if variants.is_empty() {
        r.warnings.push("packs/ 아래 변형이 없음".into());
    }
    for (mod_id, variant_id, dir) in variants {
        let prefix = format!("packs/{mod_id}/.../{variant_id}");
        if !re.is_match(&variant_id) {
            r.errors
                .push(format!("{prefix}: variantId 형식 위반 (YYYY-MM-DD-method-nick)"));
        }
        match std::fs::read_to_string(dir.join("pack.json"))
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<PackMeta>(&t).map_err(|e| e.to_string()))
        {
            Err(e) => {
                r.errors.push(format!("{prefix}: pack.json {e}"));
                continue;
            }
            Ok(meta) => {
                if meta.mod_id != mod_id {
                    r.errors
                        .push(format!("{prefix}: mod_id({}) ≠ 디렉터리({mod_id})", meta.mod_id));
                }
                if meta.variant_id != variant_id {
                    r.errors.push(format!(
                        "{prefix}: variant_id({}) ≠ 디렉터리({variant_id})",
                        meta.variant_id
                    ));
                }
            }
        }
        let vr = pack::validate_pack(&dir, None);
        for e in vr.errors {
            r.errors.push(format!("{prefix}: {e}"));
        }
        for w in vr.warnings {
            r.warnings.push(format!("{prefix}: {w}"));
        }
    }
    r
}
