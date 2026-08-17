//! 번역 블랙리스트 — 로컬라이제이션 값을 로직에 쓰는 모드는 번역하면 깨진다.
//! 매칭은 키 프리픽스 기반(폴더/모드명 변화에 무관). 데이터는 바이너리 내장.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct BlacklistEntry {
    pub name: String,
    pub key_prefixes: Vec<String>,
    pub reason: String,
}

#[derive(Deserialize)]
struct BlacklistFile {
    schema: String,
    mods: Vec<BlacklistEntry>,
}

fn entries() -> &'static [BlacklistEntry] {
    static CACHE: OnceLock<Vec<BlacklistEntry>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let f: BlacklistFile =
            serde_json::from_str(include_str!("../data/blacklist.json")).expect("내장 블랙리스트 파싱");
        assert_eq!(f.schema, "kerbaloc/blacklist@1");
        f.mods
    })
}

/// 유닛의 키들이 블랙리스트 모드에 해당하면 그 엔트리를 반환.
pub fn check(unit_entries: &BTreeMap<String, String>) -> Option<&'static BlacklistEntry> {
    entries().iter().find(|b| {
        b.key_prefixes
            .iter()
            .any(|p| unit_entries.keys().any(|k| k.starts_with(p.as_str())))
    })
}
