use std::path::{Path, PathBuf};

/// buildID64.txt의 `language = xx` 값.
pub fn read_language(ksp_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(ksp_root.join("buildID64.txt")).ok()?;
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "language" {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// language 줄만 교체하고 나머지 줄과 개행 형식(CRLF/LF)은 보존.
pub fn set_language(ksp_root: &Path, lang: &str) -> std::io::Result<()> {
    let path = ksp_root.join("buildID64.txt");
    let text = std::fs::read_to_string(&path)?;
    let out: Vec<String> = text
        .lines()
        .map(|line| match line.split_once('=') {
            Some((k, _)) if k.trim() == "language" => format!("{}= {lang}", k),
            _ => line.to_string(),
        })
        .collect();
    let sep = if text.contains("\r\n") { "\r\n" } else { "\n" };
    std::fs::write(&path, out.join(sep) + sep)
}

#[cfg(windows)]
pub fn detect_ksp_roots() -> Vec<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let mut roots = vec![];
    let Ok(steam) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") else {
        return roots;
    };
    let Ok(path) = steam.get_value::<String, _>("SteamPath") else {
        return roots;
    };
    let steam_dir = PathBuf::from(path);
    let mut libs = vec![steam_dir.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam_dir.join("steamapps/libraryfolders.vdf")) {
        // "path" "..." 값만 정규식으로 추출 (VDF 전체 파싱 불필요)
        let re = regex::Regex::new(r#""path"\s+"((?:[^"\\]|\\.)*)""#).unwrap();
        for c in re.captures_iter(&vdf) {
            libs.push(PathBuf::from(c[1].replace("\\\\", "\\")));
        }
    }
    for lib in libs {
        let cand = lib.join("steamapps/common/Kerbal Space Program");
        if cand.join("buildID64.txt").is_file() {
            // 레지스트리(소문자/슬래시)와 VDF(원표기) 경로 표기가 달라도 같은 폴더면 중복 제거
            let canon = std::fs::canonicalize(&cand).unwrap_or(cand);
            if !roots.contains(&canon) {
                roots.push(canon);
            }
        }
    }
    roots
}

#[cfg(not(windows))]
pub fn detect_ksp_roots() -> Vec<PathBuf> {
    vec![]
}
