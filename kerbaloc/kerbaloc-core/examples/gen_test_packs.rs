//! 프리즈 재현 실험용 팩 생성기.
//!
//! 사용: cargo run -p kerbaloc-core --example gen_test_packs -- <소스cfg> <출력디렉터리> <ModId> <good|bad>
//!
//! - good: 의사번역(pseudo-loc) — 모든 값 앞에 "한" 접두, 토큰 완전 보존 → 검증기 통과
//! - bad : 구방식 손상 시뮬레이션 — 의사번역 + 첫 Lingoona 토큰의 닫는 '>' 절단(3키마다),
//!         값에 원시 중괄호 주입(50키마다) → 검증기가 거부해야 정상

use kerbaloc_core::{cfg, loc};
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("사용: gen_test_packs <소스cfg> <출력디렉터리> <ModId> <good|bad>");
        std::process::exit(2);
    }
    let (src_path, out_dir, mod_id, mode) = (&args[1], &args[2], &args[3], &args[4]);
    let text = std::fs::read_to_string(src_path).expect("소스 읽기 실패");
    let root = cfg::parse(&text).expect("소스 파싱 실패");
    let entries = loc::extract_localization(&root, "en-us");
    assert!(!entries.is_empty(), "en-us 노드 없음");

    let mut body = String::new();
    let mut n = 0usize;
    let mut translated = 0usize;
    for (k, v) in &entries {
        n += 1;
        if v.trim().is_empty() || v.contains("//") || k.contains('/') {
            continue; // 빈 값·주석 절단 위험·중첩 키는 건너뜀 (영어 폴백)
        }
        let mut val = format!("한{v}");
        if mode == "bad" {
            if n % 3 == 0 {
                // Lingoona 토큰 손상: 첫 "<<x>>"의 마지막 '>' 절단
                if let Some(i) = val.find(">>") {
                    val.replace_range(i..i + 2, ">");
                }
            }
            if n % 50 == 0 {
                val = format!("{{{val}}}"); // 원시 중괄호 주입 → cfg 구조 파괴
            }
        }
        writeln!(body, "\t\t{k} = {val}").unwrap();
        translated += 1;
    }

    let out = std::path::Path::new(out_dir);
    std::fs::create_dir_all(out.join("Localization")).unwrap();
    let cfg_text = format!("Localization\n{{\n\tko\n\t{{\n{body}\t}}\n}}\n");
    std::fs::write(out.join("Localization/ko.cfg"), cfg_text).unwrap();
    let meta = format!(
        r#"{{
  "schema": "kerbaloc/pack@1",
  "lang": "ko",
  "mod_id": "{mod_id}",
  "variant_id": "2026-08-16-pseudo-{mode}",
  "src_sha256": "v1:sha256:{}",
  "keys_translated": {translated},
  "keys_target": {translated}
}}
"#,
        "0".repeat(64)
    );
    std::fs::write(out.join("pack.json"), meta).unwrap();
    println!(
        "{mod_id} {mode}: {translated}/{} 키 생성 → {}",
        entries.len(),
        out.display()
    );
}
