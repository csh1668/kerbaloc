use crate::{avc, cfg, ckan::CkanRegistry, hash, loc, parts};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
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

#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub raw: Option<String>,
    pub source: &'static str, // "ckan" | "avc" | "unknown"
}

#[derive(Debug, Clone)]
pub struct ModUnit {
    pub mod_id: String,
    pub id_source: IdSource,
    pub display_name: String,
    pub version: VersionInfo,
    pub files: Vec<PathBuf>,
    pub entries: BTreeMap<String, String>,
    pub part_fields: BTreeMap<String, parts::PartField>,
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
    let mut versions: Vec<PathBuf> = WalkDir::new(top_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|x| x == "version"))
        .map(|entry| entry.into_path())
        .collect();
    versions.sort();
    for path in versions {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(info) = avc::parse_version_file(&text) else {
            continue;
        };
        let parent = path.parent().unwrap();
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
            version: VersionInfo {
                raw: None,
                source: "unknown",
            },
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
            version: VersionInfo {
                raw: None,
                source: "unknown",
            },
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
        version: VersionInfo {
            raw: None,
            source: "unknown",
        },
    }
}

/// Collect real localization keys in every language, including language-only
/// files which do not otherwise create an English translation unit.
fn real_localization_keys(root: &cfg::Node, keys: &mut BTreeSet<String>) {
    if root.name == "Localization" {
        for language in &root.children {
            keys.extend(loc::extract_localization(root, &language.name).into_keys());
        }
    }
    for child in &root.children {
        real_localization_keys(child, keys);
    }
}

struct ScannedFile {
    path: PathBuf,
    entries: BTreeMap<String, String>,
    part_fields: Vec<parts::PartField>,
    official_langs: Vec<String>,
}

struct Group {
    owner: Owner,
    files: Vec<PathBuf>,
    entries: BTreeMap<String, String>,
    part_fields: BTreeMap<String, parts::PartField>,
    official_langs: Vec<String>,
}

pub fn scan_gamedata(ksp_root: &Path) -> Vec<ModUnit> {
    let gamedata = ksp_root.join("GameData");
    let ckan = CkanRegistry::load(ksp_root);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut paths: Vec<PathBuf> = WalkDir::new(&gamedata)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() != 1 || !entry.file_name().to_string_lossy().eq_ignore_ascii_case("KerbaLoc")
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|x| x == "cfg"))
        .map(|entry| entry.into_path())
        .collect();
    paths.sort();
    let mut name_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut real_keys = BTreeSet::new();
    let mut scanned = Vec::new();

    // Discover names globally before selecting any synthetic entries. A second
    // plain PART is ambiguous even if its text already uses localization tags.
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = cfg::parse(&text) else {
            continue;
        };
        for name in parts::plain_part_names(&root) {
            *name_counts.entry(name.to_string()).or_default() += 1;
        }
        real_localization_keys(&root, &mut real_keys);
        let entries = loc::extract_localization(&root, "en-us");
        let part_fields = parts::extract(&root);
        if entries.is_empty() && part_fields.is_empty() {
            continue;
        }
        let langs: Vec<String> = OFFICIAL_LANGS
            .iter()
            .filter(|l| !loc::extract_localization(&root, l).is_empty())
            .map(|l| l.to_string())
            .collect();
        scanned.push(ScannedFile {
            path,
            entries,
            part_fields,
            official_langs: langs,
        });
    }

    for file in scanned {
        let owner = resolve_owner(ksp_root, &file.path, ckan.as_ref());
        let mut entries = file.entries;
        let mut part_fields = BTreeMap::new();
        for part_field in file.part_fields {
            if name_counts.get(&part_field.part_name) != Some(&1) {
                continue;
            }
            let key = parts::key(&owner.key, &part_field.part_name, &part_field.field);
            if real_keys.contains(&key) {
                continue;
            }
            entries.insert(key.clone(), part_field.source.clone());
            part_fields.insert(key, part_field);
        }
        if entries.is_empty() {
            continue;
        }
        let g = groups
            .entry(owner.key.clone())
            .or_insert_with(|| Group {
                owner,
                files: vec![],
                entries: BTreeMap::new(),
                part_fields: BTreeMap::new(),
                official_langs: vec![],
            });
        g.files.push(file.path);
        g.entries.extend(entries);
        g.part_fields.extend(part_fields);
        for l in file.official_langs {
            if !g.official_langs.contains(&l) {
                g.official_langs.push(l);
            }
        }
    }

    groups
        .into_iter()
        .map(|(key, group)| ModUnit {
            source_hash: hash::source_hash(&group.entries),
            keys_hash: hash::keys_hash(&group.entries),
            mod_id: key,
            id_source: group.owner.id_source,
            display_name: group.owner.display,
            version: group.owner.version,
            files: group.files,
            entries: group.entries,
            part_fields: group.part_fields,
            official_langs: group.official_langs,
        })
        .collect()
}
