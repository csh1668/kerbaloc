//! 원격 kerbaloc-db 다운로드 클라이언트.
//! 경로: manifest(release 자산) → jsDelivr@commit(불변, 기본) → raw@commit(폴백).
//! 모든 파일은 인덱스의 sha256으로 검증한다 (부록 E §4).

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const REPO: &str = "csh1668/kerbaloc-db";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub commit: String,
    #[serde(rename = "indexSha256")]
    pub index_sha256: String,
    pub repo: String,
}

#[derive(Debug, Deserialize)]
pub struct Index {
    pub schema: String,
    pub lang: String,
    pub packs: Vec<IndexPack>,
}

#[derive(Debug, Deserialize)]
pub struct IndexPack {
    #[serde(rename = "modId")]
    pub mod_id: String,
    pub variants: Vec<IndexVariant>,
}

#[derive(Debug, Deserialize)]
pub struct IndexVariant {
    #[serde(rename = "variantId")]
    pub variant_id: String,
    pub path: String,
    #[serde(rename = "srcSha256")]
    pub src_sha256: String,
    #[serde(rename = "keysTranslated")]
    pub keys_translated: usize,
    #[serde(rename = "keysTarget")]
    pub keys_target: usize,
    pub files: Vec<IndexFile>,
}

#[derive(Debug, Deserialize)]
pub struct IndexFile {
    pub path: String,
    pub sha256: String,
    #[serde(rename = "sizeB")]
    pub size_b: u64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("kerbaloc/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("클라이언트 생성 실패")
}

pub async fn fetch_manifest() -> anyhow::Result<Manifest> {
    let url = format!("https://github.com/{REPO}/releases/latest/download/manifest.json");
    let resp = client().get(&url).send().await?;
    anyhow::ensure!(
        resp.status().is_success(),
        "manifest 요청 실패 {}: {url}",
        resp.status()
    );
    let m: Manifest = resp.json().await?;
    anyhow::ensure!(
        m.schema == "kerbaloc/manifest@1",
        "알 수 없는 manifest 스키마"
    );
    Ok(m)
}

/// jsDelivr 기본 + raw 폴백으로 레포 파일 하나를 받는다.
async fn fetch_repo_file(commit: &str, rel: &str) -> anyhow::Result<Vec<u8>> {
    let urls = [
        format!("https://cdn.jsdelivr.net/gh/{REPO}@{commit}/{rel}"),
        format!("https://raw.githubusercontent.com/{REPO}/{commit}/{rel}"),
    ];
    let c = client();
    let mut last_err = None;
    for url in &urls {
        match c.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                return Ok(resp.bytes().await?.to_vec());
            }
            Ok(resp) => last_err = Some(anyhow::anyhow!("{url}: {}", resp.status())),
            Err(e) => last_err = Some(e.into()),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("다운로드 실패")))
}

pub async fn fetch_index(manifest: &Manifest) -> anyhow::Result<Index> {
    let bytes = fetch_repo_file(&manifest.commit, "index/ko.json").await?;
    let got = sha256_hex(&bytes);
    anyhow::ensure!(
        got == manifest.index_sha256,
        "인덱스 해시 불일치: 기대 {} 실제 {got}",
        manifest.index_sha256
    );
    Ok(serde_json::from_slice(&bytes)?)
}

/// 변형의 모든 파일을 받아 sha256 검증 후 dest_dir에 조립.
pub async fn download_variant(
    manifest: &Manifest,
    variant: &IndexVariant,
    dest_dir: &Path,
) -> anyhow::Result<()> {
    for f in &variant.files {
        let rel = format!("{}{}", variant.path, f.path);
        let bytes = fetch_repo_file(&manifest.commit, &rel).await?;
        let got = sha256_hex(&bytes);
        anyhow::ensure!(
            got == f.sha256,
            "{}: 해시 불일치 (기대 {} 실제 {got})",
            f.path,
            f.sha256
        );
        let out = dest_dir.join(&f.path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, &bytes)?;
    }
    Ok(())
}
