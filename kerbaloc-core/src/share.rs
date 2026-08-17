//! 익명 업로드 프록시(kerbaloc-proxy)로 팩 제출 — 계정·키 불필요.

use crate::pack::PackMeta;
use base64::Engine;
use serde_json::json;
use std::path::Path;

pub const DEFAULT_PROXY: &str = "https://kerbaloc-proxy.kerbaloc-proxy.workers.dev";

pub struct ShareResult {
    pub pr_url: String,
}

/// 팩 디렉터리에서 팩 하나의 메타·파일 필드 구성 (schema/nick 제외).
fn pack_entry(pack_dir: &Path) -> anyhow::Result<serde_json::Value> {
    let meta: PackMeta =
        serde_json::from_str(&std::fs::read_to_string(pack_dir.join("pack.json"))?)?;
    let meta_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(pack_dir.join("pack.json"))?)?;
    let b64 = base64::engine::general_purpose::STANDARD;

    let mut files = vec![json!({
        "path": "pack.json",
        "contentBase64": b64.encode(std::fs::read(pack_dir.join("pack.json"))?),
    })];
    for sub in ["Localization", "Patches"] {
        if let Ok(rd) = std::fs::read_dir(pack_dir.join(sub)) {
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().to_string();
                files.push(json!({
                    "path": format!("{sub}/{name}"),
                    "contentBase64": b64.encode(std::fs::read(e.path())?),
                }));
            }
        }
    }
    Ok(json!({
        "modId": meta.mod_id,
        "variantId": meta.variant_id,
        "keysTranslated": meta.keys_translated,
        "keysTarget": meta.keys_target,
        "srcSha256": meta.src_sha256,
        "model": meta_json.get("model").cloned().unwrap_or(serde_json::Value::Null),
        "files": files,
    }))
}

/// 팩 디렉터리에서 kerbaloc/submit@1 페이로드 구성.
pub fn build_payload(pack_dir: &Path, nick: &str) -> anyhow::Result<serde_json::Value> {
    let mut v = pack_entry(pack_dir)?;
    let o = v.as_object_mut().expect("object");
    o.insert("schema".into(), json!("kerbaloc/submit@1"));
    o.insert("nick".into(), json!(nick));
    Ok(v)
}

/// 여러 팩 디렉터리에서 kerbaloc/submit-batch@1 페이로드 구성 (한 번의 PR).
pub fn build_batch_payload(pack_dirs: &[&Path], nick: &str) -> anyhow::Result<serde_json::Value> {
    let packs: Vec<serde_json::Value> = pack_dirs
        .iter()
        .map(|d| pack_entry(d))
        .collect::<anyhow::Result<_>>()?;
    Ok(json!({
        "schema": "kerbaloc/submit-batch@1",
        "nick": nick,
        "packs": packs,
    }))
}

pub async fn share_pack(
    proxy_url: &str,
    pack_dir: &Path,
    nick: &str,
) -> anyhow::Result<ShareResult> {
    submit(proxy_url, build_payload(pack_dir, nick)?).await
}

/// 여러 팩을 한 번의 제출(한 개의 PR)로 공유.
pub async fn share_packs(
    proxy_url: &str,
    pack_dirs: &[&Path],
    nick: &str,
) -> anyhow::Result<ShareResult> {
    anyhow::ensure!(!pack_dirs.is_empty(), "공유할 팩이 없습니다");
    submit(proxy_url, build_batch_payload(pack_dirs, nick)?).await
}

async fn submit(proxy_url: &str, payload: serde_json::Value) -> anyhow::Result<ShareResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("kerbaloc/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .post(format!("{}/v1/submit", proxy_url.trim_end_matches('/')))
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    anyhow::ensure!(
        status.is_success(),
        "제출 실패 {status}: {}",
        serde_json::to_string(&body).unwrap_or_default()
    );
    let pr_url = body
        .get("prUrl")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("응답에 prUrl 없음"))?
        .to_string();
    Ok(ShareResult { pr_url })
}
