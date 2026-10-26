use clap::{Parser, Subcommand};
use kerbaloc_core::llm::create_provider;
use kerbaloc_core::settings::Settings;
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
    /// LLM으로 모드 번역 → 검증 → 팩 생성 (앱과 같은 설정·프로필·용어집 사용)
    Translate {
        mod_id: String,
        /// 사용할 LLM 프로필 (id 또는 이름, 미지정 시 활성 프로필)
        #[arg(long)]
        profile: Option<String>,
        /// 변형 ID에 들어갈 닉네임 (미지정 시 설정의 닉네임)
        #[arg(long)]
        nick: Option<String>,
        /// 완성된 팩을 GameData/KerbaLoc에 바로 설치
        #[arg(long)]
        install: bool,
        /// 같은 소스·모델의 기존 잡이 있어도 이어하지 않고 새로 시작
        #[arg(long)]
        fresh: bool,
    },
    /// LLM 프로필 관리 — 앱 설정과 같은 파일(settings.json)을 쓴다
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// 프로필 목록 (* = 활성)
    List,
    /// 활성 프로필 변경 (id 또는 이름)
    Use { profile: String },
    /// 모델 목록 조회 — ▶ = 이 프로필이 쓸 모델 (지정값 → 기본 모델 → 목록 최상단)
    Models { profile: Option<String> },
    /// 프로필 생성·수정: key=value … (빈 값은 해제)
    /// 키: name provider model api_key base_url reasoning command price_in price_out
    Set {
        profile: String,
        #[arg(required = true)]
        values: Vec<String>,
    },
    /// 프로필 삭제 (마지막 하나는 삭제 불가)
    Remove { profile: String },
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
    /// 팩을 익명 프록시로 공유 (계정·키 불필요 — 자동 PR 생성)
    Share {
        pack_dir: PathBuf,
        /// 미지정 시 설정의 닉네임
        #[arg(long)]
        nick: Option<String>,
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
        DbCmd::Share { pack_dir, nick } => {
            let r = kerbaloc_core::pack::validate_pack(&pack_dir, None);
            if !r.errors.is_empty() {
                for e in &r.errors {
                    eprintln!("오류: {e}");
                }
                anyhow::bail!("팩 검증 실패 — 공유 중단");
            }
            let proxy = std::env::var("KERBALOC_PROXY_URL")
                .unwrap_or_else(|_| kerbaloc_core::share::DEFAULT_PROXY.into());
            let rt = tokio::runtime::Runtime::new()?;
            let nick = nick.unwrap_or_else(|| Settings::load().nick());
            let result = rt.block_on(kerbaloc_core::share::share_pack(&proxy, &pack_dir, &nick))?;
            println!("공유 완료! PR: {}", result.pr_url);
        }
    }
    Ok(())
}

fn cmd_translate(
    root: &std::path::Path,
    mod_id: &str,
    profile: Option<&str>,
    nick: Option<String>,
    install: bool,
    fresh: bool,
) -> anyhow::Result<()> {
    use kerbaloc_core::translate::{translate_mod, TranslateOptions};
    let settings = Settings::load();
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

    let rt = tokio::runtime::Runtime::new()?;
    let p = settings.profile_or_active(profile)?;
    let cfg = rt.block_on(settings.resolved_provider_config(profile))?;
    let provider = create_provider(&cfg)?;
    println!(
        "프로필: {} ({} / {})",
        p.name,
        cfg.kind.id(),
        provider.name()
    );

    let nick = nick.unwrap_or_else(|| settings.nick());
    let opts = TranslateOptions {
        nick: &nick,
        batch: settings.batch_options(),
        max_retries: settings.max_retries(),
        fresh,
    };
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(settings.workers()));
    let out = rt
        .block_on(translate_mod(
            root,
            &unit,
            provider.as_ref(),
            &opts,
            sem,
            &|done, total, cost| println!("  [{done}/{total}] 누적 ${cost:.4}"),
        ))
        .map_err(|(e, cost)| anyhow::anyhow!("{e} (지출 ${cost:.4})"))?;

    if let Some(note) = &out.glossary_note {
        println!("{note}");
    }
    let report = &out.report;
    println!(
        "완료: ok {} / 검수 필요 {} / 실패 {} — 비용 ${:.4}",
        report.ok.len(),
        report.review.len(),
        report.failed.len(),
        out.cost
    );
    println!("팩 생성: {}", out.pack_dir.display());

    if !report.review.is_empty() {
        let review_path = out.pack_dir.join("review.txt");
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
        let dest = kerbaloc_core::pack::install_pack(root, &out.pack_dir)?;
        println!("설치됨: {}", dest.display());
    }
    Ok(())
}

/// 키 보유 여부만 표시 — 목록 출력에 키 값을 노출하지 않는다.
fn mask(v: &Option<String>) -> &'static str {
    if v.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        "있음"
    } else {
        "-"
    }
}

fn cmd_profile(cmd: ProfileCmd) -> anyhow::Result<()> {
    use kerbaloc_core::llm::{default_model, model_choices, ProviderKind};
    use kerbaloc_core::settings::LlmProfile;
    let mut settings = Settings::load();
    match cmd {
        ProfileCmd::List => {
            let active = settings.active().map(|p| p.id.clone());
            println!(
                "  {:<14} {:<16} {:<12} {:<28} {:<8} 키",
                "id", "이름", "유형", "모델", "추론"
            );
            for p in &settings.profiles {
                println!(
                    "{} {:<14} {:<16} {:<12} {:<28} {:<8} {}",
                    if Some(&p.id) == active.as_ref() {
                        "*"
                    } else {
                        " "
                    },
                    p.id,
                    p.name,
                    p.provider,
                    p.model.as_deref().unwrap_or("(자동)"),
                    p.reasoning.as_deref().unwrap_or("-"),
                    mask(&p.api_key)
                );
            }
            println!(
                "설정 파일: {}",
                kerbaloc_core::settings::settings_path().display()
            );
        }
        ProfileCmd::Use { profile } => {
            let id = settings.profile_or_active(Some(&profile))?.id.clone();
            settings.active_profile = Some(id.clone());
            settings.save()?;
            println!("활성 프로필: {id}");
        }
        ProfileCmd::Models { profile } => {
            let p = settings.profile_or_active(profile.as_deref())?;
            let cfg = p.provider_config()?;
            let rt = tokio::runtime::Runtime::new()?;
            let mc = rt.block_on(model_choices(&cfg, p.model.as_deref()));
            if let Some(e) = &mc.error {
                eprintln!("목록 조회 실패 — 기본 후보를 표시합니다: {e}");
            }
            let default = default_model(cfg.kind);
            for m in &mc.models {
                let mark = if mc.selected.as_deref() == Some(m) {
                    "▶"
                } else {
                    " "
                };
                let tag = if m == default { " (기본)" } else { "" };
                println!("{mark} {m}{tag}");
            }
            if let Some(s) = mc
                .selected
                .as_deref()
                .filter(|s| !mc.models.iter().any(|m| m == s))
            {
                println!("▶ {s} (목록에 없음 — 지정값)");
            }
            println!("선택: kerbaloc profile set {} model=<모델>", p.id);
        }
        ProfileCmd::Set { profile, values } => {
            let idx = settings
                .profiles
                .iter()
                .position(|p| p.id == profile || p.name == profile);
            let mut p = match idx {
                Some(i) => settings.profiles[i].clone(),
                None => LlmProfile {
                    id: profile.clone(),
                    name: profile.clone(),
                    ..Default::default()
                },
            };
            for kv in &values {
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("key=value 형식이 아님: {kv}"))?;
                let v = v.trim();
                let opt = || (!v.is_empty()).then(|| v.to_string());
                let price = || -> anyhow::Result<Option<f64>> {
                    Ok(if v.is_empty() { None } else { Some(v.parse()?) })
                };
                match k.trim() {
                    "name" => p.name = v.to_string(),
                    "provider" => {
                        v.parse::<ProviderKind>()?;
                        p.provider = v.to_string();
                    }
                    "model" => p.model = opt(),
                    "api_key" => p.api_key = opt(),
                    "base_url" => p.base_url = opt(),
                    "reasoning" => p.reasoning = opt(),
                    "command" => p.command = opt(),
                    "price_in" => p.price_in = price()?,
                    "price_out" => p.price_out = price()?,
                    other => anyhow::bail!("알 수 없는 키: {other}"),
                }
            }
            let kind: ProviderKind = p
                .provider
                .parse()
                .map_err(|_| anyhow::anyhow!("provider를 지정하세요 (예: provider=gemini)"))?;
            if let Some(r) = &p.reasoning {
                let levels = kind.info().reasoning_levels;
                anyhow::ensure!(
                    levels.contains(&r.as_str()),
                    "{}의 reasoning은 {} 중 하나",
                    kind.id(),
                    levels.join("|")
                );
            }
            let id = p.id.clone();
            match idx {
                Some(i) => settings.profiles[i] = p,
                None => settings.profiles.push(p),
            }
            settings.save()?;
            println!("저장됨: {id}");
        }
        ProfileCmd::Remove { profile } => {
            anyhow::ensure!(
                settings.profiles.len() > 1,
                "마지막 프로필은 삭제할 수 없습니다"
            );
            let id = settings.profile_or_active(Some(&profile))?.id.clone();
            settings.profiles.retain(|p| p.id != id);
            if settings.active_profile.as_deref() == Some(id.as_str()) {
                settings.active_profile = Some(settings.profiles[0].id.clone());
            }
            settings.save()?;
            println!("삭제됨: {id}");
        }
    }
    Ok(())
}

fn resolve_root(cli_root: Option<PathBuf>) -> PathBuf {
    // 우선순위: --root > 설정의 KSP 경로(앱과 공유) > Steam 자동 감지
    if let Some(r) = cli_root.or_else(|| Settings::load().ksp_root()) {
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
    // db index/validate는 KSP 설치 없이 동작해야 한다(CI 러너) — 루트는 지연 해석.
    let needs_root = !matches!(
        cli.cmd,
        Cmd::Db {
            cmd: DbCmd::Index { .. } | DbCmd::Validate { .. } | DbCmd::List | DbCmd::Share { .. }
        } | Cmd::Profile { .. }
    );
    let root = if needs_root {
        resolve_root(cli.root)
    } else {
        cli.root.unwrap_or_else(|| PathBuf::from("."))
    };
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
            profile,
            nick,
            install,
            fresh,
        } => {
            if let Err(e) = cmd_translate(&root, &mod_id, profile.as_deref(), nick, install, fresh)
            {
                eprintln!("오류: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Profile { cmd } => {
            if let Err(e) = cmd_profile(cmd) {
                eprintln!("오류: {e}");
                std::process::exit(1);
            }
        }
    }
}
