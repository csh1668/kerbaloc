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
            println!("{:<44} {:<12} {:>6}  {}", "ModId", "버전", "키수", "해시");
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
                println!("총 {}개 파일이 구방식 번역으로 오염되어 있습니다.", items.len());
                if let Some(zip) = backup {
                    let n = kerbaloc_core::doctor::backup_polluted(&root, &zip, &items)
                        .expect("백업 실패");
                    println!("{n}개 파일을 {}에 백업했습니다.", zip.display());
                }
                println!("복원 방법: Squad/SquadExpansion → Steam 무결성 검사, 모드 → CKAN 재설치 또는 재다운로드.");
            }
        }
        Cmd::Validate { dir } => {
            let r = kerbaloc_core::pack::validate_pack(&dir, None);
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
            let r = kerbaloc_core::pack::validate_pack(&dir, None);
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
    }
}
