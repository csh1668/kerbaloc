//! 嵓・ｦｬ・・・ｬ嶸・・､嵭們圸 甯ｩ ・晧┳・ｰ.
//!
//! ・ｬ・ｩ: cargo run -p kerbaloc-core --example gen_test_packs -- <・護侃cfg> <・罹･・罷駕┣・ｬ> <ModId> <good|bad>
//!
//! - good: ・們ぎ・溢溜(pseudo-loc) 窶・・ｨ・ ・・・樌乱 "﨑・ ・瀧草, 奝增ｰ ・・・・ｴ・ｴ 竊・・・晝ｸｰ 奝ｵ・ｼ
//! - bad : ・ｬ・ｩ・・・川メ ・罹ｮｬ・溢擽・・窶・・們ぎ・溢溜 + ・ｫ Lingoona 奝增ｰ・・・ｫ・・'>' ・壱卿(3墲､・壱共),
//!         ・廷乱 ・川亨 ・滝ｴ・从 ・ｼ・・50墲､・壱共) 竊・・・晝ｸｰ・ ・ｰ・﨑ｴ・ｼ ・菩メ

use kerbaloc_core::{cfg, loc};
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("・ｬ・ｩ: gen_test_packs <・護侃cfg> <・罹･・罷駕┣・ｬ> <ModId> <good|bad>");
        std::process::exit(2);
    }
    let (src_path, out_dir, mod_id, mode) = (&args[1], &args[2], &args[3], &args[4]);
    let text = std::fs::read_to_string(src_path).expect("・護侃 ・ｽ・ｰ ・､甯ｨ");
    let root = cfg::parse(&text).expect("・護侃 甯護恭 ・､甯ｨ");
    let entries = loc::extract_localization(&root, "en-us");
    assert!(!entries.is_empty(), "en-us ・ｸ・・・・搆");

    let mut body = String::new();
    let mut n = 0usize;
    let mut translated = 0usize;
    for (k, v) in &entries {
        n += 1;
        if v.trim().is_empty() || v.contains("//") || k.contains('/') {
            continue; // ・・・陳ｷ・ｼ・・・壱卿 ・・利ﾂｷ・卓ｲｩ 墲､・・・ｴ・壱怙 (・・牟 尞ｴ・ｱ)
        }
        let mut val = format!("﨑悳v}");
        if mode == "bad" {
            if n.is_multiple_of(3) {
                // Lingoona 奝增ｰ ・川メ: ・ｫ "<<x>>"・・・溢ｧ・・'>' ・壱卿
                if let Some(i) = val.find(">>") {
                    val.replace_range(i..i + 2, ">");
                }
            }
            if n.is_multiple_of(50) {
                val = format!("{{{val}}}"); // ・川亨 ・滝ｴ・从 ・ｼ・・竊・cfg ・ｬ・ｰ 甯語ｴｴ
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
        "{mod_id} {mode}: {translated}/{} 墲､ ・晧┳ 竊・{}",
        entries.len(),
        out.display()
    );
}
