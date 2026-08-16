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
    }
}
