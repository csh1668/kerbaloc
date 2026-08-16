use crate::{cfg, loc};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Pollution {
    pub path: PathBuf,
    pub korean_values: usize,
    pub total: usize,
}

fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

/// en-us 노드 값에 한글이 포함된 cfg 목록 (= 구방식 교체 번역으로 오염된 파일).
pub fn detect_pollution(ksp_root: &Path) -> Vec<Pollution> {
    let gamedata = ksp_root.join("GameData");
    let mut out = vec![];
    for e in WalkDir::new(&gamedata).into_iter().filter_map(Result::ok) {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "cfg") {
            continue;
        }
        let rel = p
            .strip_prefix(&gamedata)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("KerbaLoc/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(p) else {
            continue;
        };
        let Ok(root) = cfg::parse(&text) else {
            continue;
        };
        let en = loc::extract_localization(&root, "en-us");
        if en.is_empty() {
            continue;
        }
        let korean = en.values().filter(|v| has_hangul(v)).count();
        if korean > 0 {
            out.push(Pollution {
                path: p.to_path_buf(),
                korean_values: korean,
                total: en.len(),
            });
        }
    }
    out
}

pub fn backup_polluted(
    ksp_root: &Path,
    out_zip: &Path,
    items: &[Pollution],
) -> std::io::Result<usize> {
    let f = std::fs::File::create(out_zip)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    let mut n = 0;
    for it in items {
        let rel = it
            .path
            .strip_prefix(ksp_root)
            .unwrap_or(&it.path)
            .to_string_lossy()
            .replace('\\', "/");
        z.start_file(rel, opts)?;
        z.write_all(&std::fs::read(&it.path)?)?;
        n += 1;
    }
    z.finish()?;
    Ok(n)
}
