use clap::{Parser, Subcommand};
use kerbaloc_core::{game, scan};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "kerbaloc", version)]
struct Cli {
    /// KSP 설치 경로 (미지정 시 Steam에서 자동 감지)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 현재 언어/설치 상태 표시
    Status,
    /// 게임 언어를 ko로 전환
    Enable,
    /// 게임 언어를 en-us로 복원
    Disable,
    /// GameData 스캔: 번역 대상 모드 목록
    Scan,
    /// 구방식 번역 오염(en-us 노드 내 한글) 감지 및 백업
    Doctor {
        #[arg(long)]
        backup: Option<PathBuf>,
    },
    /// 팩 디렉터리 검증 (CI에서도 사용)
    Validate { dir: PathBuf },
    /// 팩 설치 (GameData/KerbaLoc/<lang>/<ModId>/)
    Install { dir: PathBuf },
    /// 설치된 팩 제거
    Remove {
        mod_id: String,
        #[arg(long, default_value = "ko")]
        lang: String,
    },
    /// 번역 DB 레포 조작 (인덱스·검증·다운로드)
    Db {
        #[command(subcommand)]
        cmd: DbCmd,
    },
    /// LLM으로 모드 번역 → 검증 → 팩 생성
    Translate {
        mod_id: String,
        #[arg(long, default_value = "anon")]
        nick: String,
        /// 완성된 팩을 GameData/KerbaLoc에 바로 설치
        #[arg(long)]
        install: bool,
        /// 기존 잡 이어하기
        #[arg(long)]
        resume: bool,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// 레포 디렉터리에서 index/ko.json 생성
    Index {
        repo_dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 레포 내 모든 팩 검증 (CI 게이트)
    Validate { repo_dir: PathBuf },
    /// 원격 DB의 팩 목록 표시
    List,
    /// 원격 DB에서 팩 다운로드·검증·설치
    Install {
        mod_id: String,
        #[arg(long)]
        variant: Option<String>,
    },
}

fn cmd_db(root: &std::path::Path, cmd: DbCmd) -> anyhow::Result<()> {
    match cmd {
        DbCmd::Index { repo_dir, out } => {
            let idx = kerbaloc_core::dbrepo::build_index(&repo_dir)?;
            let text = serde_json::to_string_pretty(&idx)? + "\n";
            match out {
                Some(p) => {
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&p, text)?;
                    println!("인덱스 생성: {}", p.display());
                }
                None => print!("{text}"),
            }
        }
        DbCmd::Validate { repo_dir } => {
            let r = kerbaloc_core::dbrepo::validate_repo(&repo_dir);
            for w in &r.warnings {
                println!("경고: {w}");
            }
            for e in &r.errors {
                println!("오류: {e}");
            }
            if r.errors.is_empty() {
                println!("DB 검증 통과 (경고 {}건)", r.warnings.len());
            } else {
                std::process::exit(1);
            }
        }
        DbCmd::List => {
            let rt = tokio::runtime::Runtime::new()?;
            let (manifest, index) = rt.block_on(async {
                let m = kerbaloc_core::dbclient::fetch_manifest().await?;
                let i = kerbaloc_core::dbclient::fetch_index(&m).await?;
                anyhow::Ok((m, i))
            })?;
            println!("DB 커밋: {}", &manifest.commit[..12]);
            println!("{:<36} {:<40} {:>9}", "ModId", "변형", "키수");
            for p in &index.packs {
                for v in &p.variants {
                    println!(
                        "{:<36} {:<40} {:>4}/{:<4}",
                        p.mod_id, v.variant_id, v.keys_translated, v.keys_target
                    );
                }
            }
        }
        DbCmd::Install { mod_id, variant } => {
            let rt = tokio::runtime::Runtime::new()?;
            let tmp = tempfile::tempdir()?;
            let pack_dir = rt.block_on(async {
                let m = kerbaloc_core::dbclient::fetch_manifest().await?;
                let i = kerbaloc_core::dbclient::fetch_index(&m).await?;
                let p = i
                    .packs
                    .iter()
                    .find(|p| p.mod_id == mod_id)
                    .ok_or_else(|| anyhow::anyhow!("DB에 {mod_id} 팩이 없습니다"))?;
                let v = match &variant {
                    Some(id) => p
                        .variants
                        .iter()
                        .find(|v| &v.variant_id == id)
                        .ok_or_else(|| anyhow::anyhow!("변형 {id} 없음"))?,
                    None => p.variants.last().expect("변형 최소 1개"),
                };
                println!(
                    "다운로드: {}/{} ({}키)",
                    p.mod_id, v.variant_id, v.keys_translated
                );
                kerbaloc_core::dbclient::download_variant(&m, v, tmp.path()).await?;
                anyhow::Ok(tmp.path().to_path_buf())
            })?;
            // 설치본 원문과 대조 검증 후 설치
            let src = source_entries_for(root, &pack_dir);
            let r = kerbaloc_core::pack::validate_pack(&pack_dir, src.as_ref());
            for w in &r.warnings {
                println!("경고: {w}");
            }
            if !r.errors.is_empty() {
                for e in &r.errors {
                    eprintln!("오류: {e}");
                }
                anyhow::bail!("팩 검증 실패 — 설치 중단");
            }
            let dest = kerbaloc_core::pack::install_pack(root, &pack_dir)?;
            println!("설치됨: {}", dest.display());
        }
    }
    Ok(())
}

fn cmd_translate(
    root: &std::path::Path,
    mod_id: &str,
    nick: &str,
    install: bool,
    resume: bool,
) -> anyhow::Result<()> {
    use kerbaloc_core::{
        glossary::Glossary, jobstore::JobStore, llm::gemini::GeminiProvider, llm::Provider,
        packgen, pipeline,
    };
    dotenvy::dotenv().ok();
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY 환경변수가 필요합니다 (.env 지원)"))?;
    let model = std::env::var("KERBALOC_MODEL").unwrap_or_else(|_| "gemini-3.1-flash-lite".into());
    let esc_model =
        std::env::var("KERBALOC_MODEL_ESCALATION").unwrap_or_else(|_| "gemini-3-flash".into());

    let units = scan::scan_gamedata(root);
    let Some(unit) = units.into_iter().find(|u| u.mod_id == mod_id) else {
        anyhow::bail!(
            "설치본에서 {mod_id}를 찾지 못했습니다. `kerbaloc scan`으로 ModId를 확인하세요."
        );
    };
    println!(
        "{}: {}키, 소스 {}",
        unit.mod_id,
        unit.entries.len(),
        &unit.source_hash[..22]
    );

    // 용어집: 실행 파일 기준이 아니라 레포/설치 어느 쪽이든 — 우선 레포 경로, 없으면 스킵
    let gpath =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../glossary/core.ko.json");
    let glossary = Glossary::load(&gpath)
        .map_err(|e| anyhow::anyhow!("코어 용어집 로드 실패({e}): {}", gpath.display()))?;

    let provider = GeminiProvider::new(&model, &api_key);
    let escalation = GeminiProvider::new(&esc_model, &api_key);

    let job_dir = root.join("KerbaLoc-jobs").join(&unit.mod_id);
    let (store, manifest) = if resume && job_dir.join("manifest.json").is_file() {
        let (s, m) = JobStore::open(&job_dir)?;
        anyhow::ensure!(
            m.src_hash == unit.source_hash,
            "모드가 업데이트되어 재개할 수 없습니다. --resume 없이 새로 시작하세요."
        );
        (s, m)
    } else {
        let m = pipeline::make_manifest(
            &unit.mod_id,
            &unit.source_hash,
            provider.name(),
            provider.prices(),
            &unit.entries,
        );
        (JobStore::create(&job_dir, &m)?, m)
    };

    let rt = tokio::runtime::Runtime::new()?;
    let report = rt.block_on(pipeline::translate_job(
        &provider,
        &escalation,
        &unit.entries,
        &glossary,
        &unit.display_name,
        &store,
        &manifest,
        8,
        &|done, total, cost| println!("  [{done}/{total}] 누적 ${cost:.4}"),
    ))?;

    println!(
        "완료: ok {} / 검수 필요 {} / 실패 {} — 비용 ${:.4}",
        report.ok.len(),
        report.review.len(),
        report.failed.len(),
        report
            .usage
            .cost_usd(provider.prices().0, provider.prices().1)
    );

    let variant = packgen::make_variant_id(&model.replace(['-', '.'], ""), nick);
    let pack_dir = root
        .join("KerbaLoc-packs")
        .join(&unit.mod_id)
        .join(&variant);
    packgen::build_pack(&pack_dir, &unit, &report.ok, &variant, &model)?;
    println!("팩 생성: {}", pack_dir.display());

    if !report.review.is_empty() {
        let review_path = pack_dir.join("review.txt");
        let mut txt = String::new();
        for r in &report.review {
            txt.push_str(&format!(
                "{}\n  EN: {}\n  후보: {:?}\n  위반: {:?}\n\n",
                r.key, r.en, r.candidates, r.violations
            ));
        }
        std::fs::write(&review_path, txt)?;
        println!("검수 목록: {}", review_path.display());
    }
    for (k, why) in &report.failed {
        println!("실패: {k} — {why}");
    }

    if install {
        let dest = kerbaloc_core::pack::install_pack(root, &pack_dir)?;
        println!("설치됨: {}", dest.display());
    }
    Ok(())
}

fn resolve_root(cli_root: Option<PathBuf>) -> PathBuf {
    if let Some(r) = cli_root {
        return r;
    }
    let roots = game::detect_ksp_roots();
    match roots.len() {
        1 => roots.into_iter().next().unwrap(),
        0 => {
            eprintln!("KSP 설치를 찾지 못했습니다. --root 로 지정하세요.");
            std::process::exit(1);
        }
        _ => {
            eprintln!("KSP 설치가 여러 개입니다. --root 로 지정하세요:");
            for r in roots {
                eprintln!("  {}", r.display());
            }
            std::process::exit(1);
        }
    }
}

/// 팩의 mod_id에 해당하는 설치 모드의 en-us 원문을 스캔으로 찾는다.
/// 있으면 토큰 보존 검사까지 수행되고, 없으면 구조 검사만 한다.
fn source_entries_for(
    root: &std::path::Path,
    pack_dir: &std::path::Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(pack_dir.join("pack.json")).ok()?).ok()?;
    let mod_id = meta.get("mod_id")?.as_str()?.to_string();
    let units = scan::scan_gamedata(root);
    match units.into_iter().find(|u| u.mod_id == mod_id) {
        Some(u) => Some(u.entries),
        None => {
            eprintln!("주의: 설치본에서 {mod_id} 원문을 찾지 못해 토큰 검사를 생략합니다.");
            None
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let root = resolve_root(cli.root);
    match cli.cmd {
        Cmd::Status => {
            println!("KSP: {}", root.display());
            println!(
                "언어: {}",
                game::read_language(&root).unwrap_or_else(|| "?".into())
            );
        }
        Cmd::Enable => {
            game::set_language(&root, "ko").expect("buildID64.txt 쓰기 실패");
            println!("언어를 ko로 전환했습니다. 게임을 재시작하세요.");
        }
        Cmd::Disable => {
            game::set_language(&root, "en-us").expect("buildID64.txt 쓰기 실패");
            println!("언어를 en-us로 복원했습니다.");
        }
        Cmd::Scan => {
            let units = scan::scan_gamedata(&root);
            println!("{:<44} {:<12} {:>6}  해시", "ModId", "버전", "키수");
            for u in &units {
                println!(
                    "{:<44} {:<12} {:>6}  {}",
                    u.mod_id,
                    u.version.raw.as_deref().unwrap_or("-"),
                    u.entries.len(),
                    &u.source_hash[..22]
                );
            }
            println!("총 {}개 유닛", units.len());
        }
        Cmd::Doctor { backup } => {
            let items = kerbaloc_core::doctor::detect_pollution(&root);
            if items.is_empty() {
                println!("오염 없음 — en-us 노드에 한글이 든 파일이 없습니다.");
            } else {
                for it in &items {
                    println!(
                        "{}  ({}/{}키 한글)",
                        it.path.display(),
                        it.korean_values,
                        it.total
                    );
                }
                println!(
                    "총 {}개 파일이 구방식 번역으로 오염되어 있습니다.",
                    items.len()
                );
                if let Some(zip) = backup {
                    let n = kerbaloc_core::doctor::backup_polluted(&root, &zip, &items)
                        .expect("백업 실패");
                    println!("{n}개 파일을 {}에 백업했습니다.", zip.display());
                }
                println!("복원 방법: Squad/SquadExpansion → Steam 무결성 검사, 모드 → CKAN 재설치 또는 재다운로드.");
            }
        }
        Cmd::Validate { dir } => {
            let src = source_entries_for(&root, &dir);
            let r = kerbaloc_core::pack::validate_pack(&dir, src.as_ref());
            for w in &r.warnings {
                println!("경고: {w}");
            }
            for e in &r.errors {
                println!("오류: {e}");
            }
            if r.errors.is_empty() {
                println!("검증 통과 (경고 {}건)", r.warnings.len());
            } else {
                std::process::exit(1);
            }
        }
        Cmd::Install { dir } => {
            let src = source_entries_for(&root, &dir);
            let r = kerbaloc_core::pack::validate_pack(&dir, src.as_ref());
            if !r.errors.is_empty() {
                for e in &r.errors {
                    eprintln!("오류: {e}");
                }
                std::process::exit(1);
            }
            let dest = kerbaloc_core::pack::install_pack(&root, &dir).expect("설치 실패");
            println!("설치됨: {}", dest.display());
        }
        Cmd::Remove { mod_id, lang } => {
            if kerbaloc_core::pack::remove_pack(&root, &lang, &mod_id).expect("제거 실패") {
                println!("제거됨: {lang}/{mod_id}");
            } else {
                println!("설치되어 있지 않음: {lang}/{mod_id}");
            }
        }
        Cmd::Db { cmd } => {
            if let Err(e) = cmd_db(&root, cmd) {
                eprintln!("오류: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Translate {
            mod_id,
            nick,
            install,
            resume,
        } => {
            if let Err(e) = cmd_translate(&root, &mod_id, &nick, install, resume) {
                eprintln!("오류: {e}");
                std::process::exit(1);
            }
        }
    }
}
