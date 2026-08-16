use crate::{avc, cfg, ckan::CkanRegistry, hash, loc};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IdSource {
    Ckan,
    Stock,
    AvcGithub,
    AvcStem,
    Folder,
}

#[derive(Debug)]
pub struct VersionInfo {
    pub raw: Option<String>,
    pub source: &'static str, // "ckan" | "avc" | "unknown"
}

#[derive(Debug)]
pub struct ModUnit {
    pub mod_id: String,
    pub id_source: IdSource,
    pub display_name: String,
    pub version: VersionInfo,
    pub files: Vec<PathBuf>,
    pub entries: BTreeMap<String, String>,
    pub source_hash: String,
    pub keys_hash: String,
    pub official_langs: Vec<String>,
}

pub fn slug(s: &str) -> String {
    let s = Regex::new(r"^\d{3}_").unwrap().replace(s, "");
    let s = Regex::new(r"(?i)^z{1,3}_").unwrap().replace(&s, "");
    let s = s.replace('\'', "");
    let s = Regex::new(r"[^A-Za-z0-9]+").unwrap().replace_all(&s, "-");
    let s = Regex::new(r"-{2,}").unwrap().replace_all(&s, "-");
    let out = s.trim_matches('-').to_string();
    if out.is_empty() {
        "unnamed".into()
    } else {
        out
    }
}

const OFFICIAL_LANGS: &[&str] = &[
    "es-es", "ja", "ru", "zh-cn", "de-de", "fr-fr", "it-it", "pt-br",
];

struct Owner {
    key: String,
    id_source: IdSource,
    display: String,
    version: VersionInfo,
}

/// top 폴더 하위 전체의 .version 중 대상 파일과 가장 잘 맞는 것 선택:
/// ① 파일의 조상 디렉터리에 있는 것 중 가장 깊은 것
/// ② 없으면 top 폴더 하위 어디든 가장 얕은 것 (Versioning/ 등 실측 패턴)
fn find_avc(top_dir: &Path, file: &Path) -> Option<avc::AvcInfo> {
    let mut best: Option<(avc::AvcInfo, bool, i64)> = None; // (info, is_ancestor, score)
    for e in WalkDir::new(top_dir).into_iter().filter_map(Result::ok) {
        if !e.path().extension().is_some_and(|x| x == "version") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Some(info) = avc::parse_version_file(&text) else {
            continue;
        };
        let parent = e.path().parent().unwrap();
        let is_ancestor = file.starts_with(parent);
        let depth = parent.components().count() as i64;
        let score = if is_ancestor { depth } else { -depth };
        let better = match &best {
            None => true,
            Some((_, ba, bs)) => (is_ancestor, score) > (*ba, *bs),
        };
        if better {
            best = Some((info, is_ancestor, score));
        }
    }
    best.map(|(i, _, _)| i)
}

fn resolve_owner(ksp_root: &Path, file: &Path, ckan: Option<&CkanRegistry>) -> Owner {
    let rel = file
        .strip_prefix(ksp_root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    if let Some(reg) = ckan {
        if let Some(id) = reg.owner_of(&rel) {
            let m = reg.module(id);
            return Owner {
                key: id.to_string(),
                id_source: IdSource::Ckan,
                display: m
                    .and_then(|m| m.name.clone())
                    .unwrap_or_else(|| id.to_string()),
                version: VersionInfo {
                    raw: m.and_then(|m| m.version.clone()),
                    source: "ckan",
                },
            };
        }
    }
    let parts: Vec<String> = file
        .strip_prefix(ksp_root.join("GameData"))
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let top = parts[0].clone();
    if top == "Squad" {
        return Owner {
            key: "Squad".into(),
            id_source: IdSource::Stock,
            display: "Kerbal Space Program (스톡)".into(),
            version: VersionInfo { raw: None, source: "unknown" },
        };
    }
    if top == "SquadExpansion" {
        let id = match parts.get(1).map(String::as_str) {
            Some("MakingHistory") => "MakingHistory-DLC",
            Some("Serenity") => "BreakingGround-DLC",
            _ => "SquadExpansion",
        };
        return Owner {
            key: id.into(),
            id_source: IdSource::Stock,
            display: id.into(),
            version: VersionInfo { raw: None, source: "unknown" },
        };
    }
    let top_dir = ksp_root.join("GameData").join(&top);
    if top_dir.is_dir() {
        if let Some(info) = find_avc(&top_dir, file) {
            let version = VersionInfo {
                raw: info.version.clone(),
                source: "avc",
            };
            if let Some((_, repo)) = &info.github {
                return Owner {
                    key: format!("local.{}", slug(repo)),
                    id_source: IdSource::AvcGithub,
                    display: info.name.clone().unwrap_or_else(|| top.clone()),
                    version,
                };
            }
            if let Some(name) = &info.name {
                return Owner {
                    key: format!("local.{}", slug(name)),
                    id_source: IdSource::AvcStem,
                    display: name.clone(),
                    version,
                };
            }
        }
    }
    Owner {
        key: format!("local.{}", slug(&top)),
        id_source: IdSource::Folder,
        display: top.clone(),
        version: VersionInfo { raw: None, source: "unknown" },
    }
}

pub fn scan_gamedata(ksp_root: &Path) -> Vec<ModUnit> {
    let gamedata = ksp_root.join("GameData");
    let ckan = CkanRegistry::load(ksp_root);
    type Group = (Owner, Vec<PathBuf>, BTreeMap<String, String>, Vec<String>);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();

    for e in WalkDir::new(&gamedata).into_iter().filter_map(Result::ok) {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "cfg") {
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
        let entries = loc::extract_localization(&root, "en-us");
        if entries.is_empty() {
            continue;
        }
        let owner = resolve_owner(ksp_root, p, ckan.as_ref());
        let langs: Vec<String> = OFFICIAL_LANGS
            .iter()
            .filter(|l| !loc::extract_localization(&root, l).is_empty())
            .map(|l| l.to_string())
            .collect();
        let g = groups
            .entry(owner.key.clone())
            .or_insert_with(|| (owner, vec![], BTreeMap::new(), vec![]));
        g.1.push(p.to_path_buf());
        g.2.extend(entries);
        for l in langs {
            if !g.3.contains(&l) {
                g.3.push(l);
            }
        }
    }

    groups
        .into_iter()
        .map(|(key, (owner, files, entries, official_langs))| ModUnit {
            source_hash: hash::source_hash(&entries),
            keys_hash: hash::keys_hash(&entries),
            mod_id: key,
            id_source: owner.id_source,
            display_name: owner.display,
            version: owner.version,
            files,
            entries,
            official_langs,
        })
        .collect()
}
