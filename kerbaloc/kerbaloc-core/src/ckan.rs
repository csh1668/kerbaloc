use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
pub struct CkanModule {
    pub identifier: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub localizations: Vec<String>,
}

#[derive(Debug)]
pub struct CkanRegistry {
    files_ci: HashMap<String, String>,
    modules: HashMap<String, CkanModule>,
}

impl CkanRegistry {
    /// <ksp_root>/CKAN/registry.json 로드. 없거나 깨지면 None (조용한 폴백).
    pub fn load(ksp_root: &Path) -> Option<CkanRegistry> {
        let text = std::fs::read_to_string(ksp_root.join("CKAN").join("registry.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut files_ci = HashMap::new();
        if let Some(files) = v.get("installed_files").and_then(|x| x.as_object()) {
            for (path, ident) in files {
                if let Some(id) = ident.as_str() {
                    files_ci.insert(path.replace('\\', "/").to_lowercase(), id.to_string());
                }
            }
        }
        let mut modules = HashMap::new();
        if let Some(mods) = v.get("installed_modules").and_then(|x| x.as_object()) {
            for (ident, m) in mods {
                let sm = m.get("source_module");
                let get = |k: &str| {
                    sm.and_then(|s| s.get(k))
                        .and_then(|x| x.as_str())
                        .map(String::from)
                };
                let locs = sm
                    .and_then(|s| s.get("localizations"))
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                modules.insert(
                    ident.clone(),
                    CkanModule {
                        identifier: ident.clone(),
                        name: get("name"),
                        version: get("version"),
                        localizations: locs,
                    },
                );
            }
        }
        Some(CkanRegistry { files_ci, modules })
    }

    /// "GameData/..." 상대 경로(슬래시 무관, 대소문자 무시)의 소유 identifier.
    pub fn owner_of(&self, rel_path: &str) -> Option<&str> {
        self.files_ci
            .get(&rel_path.replace('\\', "/").to_lowercase())
            .map(String::as_str)
    }

    pub fn module(&self, identifier: &str) -> Option<&CkanModule> {
        self.modules.get(identifier)
    }
}
