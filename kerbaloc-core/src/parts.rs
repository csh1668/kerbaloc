//! Conservative extraction of literal display text from unpatched PART nodes.
//!
//! Internal part identifiers are selectors, never translation input. Only the
//! direct `title` and `description` values of a file's top-level `PART` nodes
//! are eligible; ModuleManager patches, copies and nested nodes are excluded.

use crate::cfg::Node;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartField {
    pub part_name: String,
    pub field: String,
    pub source: String,
}

/// A collision-resistant localization key for one literal PART display field.
/// The unit separators make the three identity components unambiguous.
pub fn key(mod_id: &str, part_name: &str, field: &str) -> String {
    assert!(matches!(field, "title" | "description"));
    let mut digest = Sha256::new();
    digest.update(mod_id.as_bytes());
    digest.update([0x1f]);
    digest.update(part_name.as_bytes());
    digest.update([0x1f]);
    digest.update(field.as_bytes());
    let hex: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("#KerbaLoc_Part_{hex}_{field}")
}

/// Literal ASCII part names that cannot introduce a ModuleManager selector.
pub fn is_safe_part_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_plain_source(value: &str) -> bool {
    let literal = value.trim();
    !value.trim().is_empty()
        && !value.chars().any(char::is_control)
        && !literal.starts_with('#')
        && !literal.starts_with('@')
        && !value.contains(['{', '}', '$'])
        && !value.contains("//")
        && !value.contains("@/")
}

fn unique_part_name(node: &Node) -> Option<&str> {
    let mut names = node.values.iter().filter(|(name, _)| name == "name");
    let (_, name) = names.next()?;
    if names.next().is_some() || !is_safe_part_name(name) {
        return None;
    }
    Some(name)
}

/// Literal names in top-level PART declarations, including localized and
/// conditional definitions and explicitly named copies. Counting these before
/// extraction prevents a patch from selecting another mod's same-named part.
pub(crate) fn plain_part_names(root: &Node) -> impl Iterator<Item = &str> {
    root.children
        .iter()
        .filter(|node| {
            let name = node.name.trim_start_matches(['+', '$', '@', '%']);
            name == "PART" || name.starts_with("PART:") || name.starts_with("PART[")
        })
        .flat_map(|node| node.values.iter())
        .filter(|(name, value)| {
            matches!(name.as_str(), "name" | "@name" | "%name") && is_safe_part_name(value)
        })
        .map(|(_, value)| value.as_str())
}

/// Extract independently safe display fields from a parsed CFG file.
pub fn extract(root: &Node) -> Vec<PartField> {
    let mut result = Vec::new();
    for node in root.children.iter().filter(|node| node.name == "PART") {
        let Some(part_name) = unique_part_name(node) else {
            continue;
        };
        for field in ["title", "description"] {
            let mut values = node.values.iter().filter(|(name, _)| name == field);
            let Some((_, source)) = values.next() else {
                continue;
            };
            if values.next().is_some() || !is_plain_source(source) {
                continue;
            }
            result.push(PartField {
                part_name: part_name.to_string(),
                field: field.to_string(),
                source: source.clone(),
            });
        }
    }
    result
}
