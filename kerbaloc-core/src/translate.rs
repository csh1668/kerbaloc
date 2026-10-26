//! 모드 1개 번역 — 앱·CLI 공용 오케스트레이션 (SSOT).
//! 블랙리스트 확인 → 모드 용어집(없으면 초안 자동 생성) → 잡 재개/생성 → 번역 → 팩 생성.

use crate::batching::BatchOptions;
use crate::glossary::Glossary;
use crate::jobstore::JobStore;
use crate::llm::Provider;
use crate::pipeline::{self, TranslationReport};
use crate::scan::ModUnit;
use crate::{blacklist, modglossary, packgen};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct TranslateOptions<'a> {
    pub nick: &'a str,
    pub batch: BatchOptions,
    pub max_retries: u32,
    /// true면 기존 잡을 무시하고 새로 시작 (기본: 같은 소스·모델의 잡이면 이어서)
    pub fresh: bool,
}

pub struct TranslateOutcome {
    pub report: TranslationReport,
    /// 번역 + 용어집 생성 비용 (USD)
    pub cost: f64,
    pub pack_dir: PathBuf,
    /// 이번 번역에서 용어집을 자동 생성했으면 그 결과(성공·실패)
    pub glossary_note: Option<String>,
}

/// 변형 ID의 method 세그먼트용: 모델명 → 소문자 영숫자만.
pub fn method_slug(model: &str) -> String {
    let s: String = model
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if s.is_empty() {
        "custom".into()
    } else {
        s.chars().take(30).collect()
    }
}

/// 실패해도 그때까지 지출한 비용을 함께 반환한다: Err((사유, 비용)).
pub async fn translate_mod(
    root: &Path,
    unit: &ModUnit,
    provider: &dyn Provider,
    opts: &TranslateOptions<'_>,
    sem: Arc<Semaphore>,
    progress: &(dyn Fn(usize, usize, f64) + Sync),
) -> Result<TranslateOutcome, (anyhow::Error, f64)> {
    let fail0 = |e: anyhow::Error| (e, 0.0);
    if let Some(b) = blacklist::check(&unit.entries) {
        return Err(fail0(anyhow::anyhow!(
            "번역 차단된 모드({}): {}",
            b.name,
            b.reason
        )));
    }
    let prices = provider.prices();

    // 모드 용어집: 없으면 초안을 자동 생성(미확정 저장)한다. 실패해도 번역은 코어 용어집만으로 계속.
    let gpath = modglossary::path_for(root, &unit.mod_id);
    let mut glossary_cost = 0.0;
    let mut glossary_note = None;
    if !gpath.exists() {
        let _permit = sem.acquire().await.map_err(|e| fail0(e.into()))?;
        let (r, usage) =
            modglossary::generate_draft(provider, &gpath, &unit.display_name, &unit.entries).await;
        glossary_cost = usage.cost_usd(prices.0, prices.1);
        glossary_note = Some(match r {
            Ok(g) => format!("용어집 {}개 자동 생성", g.entries.len()),
            Err(e) => format!("용어집 자동 생성 실패: {e}"),
        });
    }
    let fail_g = |e: anyhow::Error| (e, glossary_cost);

    let mut glossary = Glossary::embedded_core();
    // 모드 용어집: 초안 포함 전 항목을 병합해 프롬프트에 주입
    if let Ok(mg) = modglossary::ModGlossary::load(&gpath) {
        glossary.extend_entries(mg.prompt_entries());
    }

    // 재개: 같은 소스·모델의 잡이 있으면 이어서 (성공 청크 재번역 방지)
    let job_dir = root.join("KerbaLoc-jobs").join(&unit.mod_id);
    let existing = if opts.fresh {
        None
    } else {
        JobStore::open(&job_dir)
            .ok()
            .filter(|(_, m)| m.src_hash == unit.source_hash && m.model == provider.name())
    };
    let (store, manifest) = match existing {
        Some(sm) => sm,
        None => {
            let m = pipeline::make_manifest_with(
                &unit.mod_id,
                &unit.source_hash,
                provider.name(),
                prices,
                &unit.entries,
                opts.batch,
            );
            (JobStore::create(&job_dir, &m).map_err(fail_g)?, m)
        }
    };

    let report = pipeline::translate_job(
        provider,
        &unit.entries,
        &glossary,
        &unit.display_name,
        &store,
        &manifest,
        sem,
        opts.max_retries,
        progress,
    )
    .await
    .map_err(fail_g)?;

    let cost = report.usage.cost_usd(prices.0, prices.1) + glossary_cost;

    // 모델명은 provider.name() — Codex 기본 모델(빈 값)은 "codex"로 기록
    let variant = packgen::make_variant_id(&method_slug(provider.name()), opts.nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(&pack_dir, unit, &report.ok, &variant, provider.name())
        .map_err(|e| (e, cost))?;

    Ok(TranslateOutcome {
        report,
        cost,
        pack_dir,
        glossary_note,
    })
}
