use crate::cfg::Node;
use std::collections::BTreeMap;

fn collect(node: &Node, prefix: &str, out: &mut BTreeMap<String, String>) {
    for (k, v) in &node.values {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}/{k}")
        };
        out.insert(key, v.clone());
    }
    for c in &node.children {
        let p = if prefix.is_empty() {
            c.name.clone()
        } else {
            format!("{prefix}/{}", c.name)
        };
        collect(c, &p, out);
    }
}

fn walk(n: &Node, lang: &str, out: &mut BTreeMap<String, String>) {
    if n.name == "Localization" {
        for c in &n.children {
            if c.name == lang {
                collect(c, "", out);
            }
        }
    }
    for c in &n.children {
        walk(c, lang, out);
    }
}

/// root 전체에서 Localization/<lang> 노드들을 찾아 합집합. 하위 노드는 "sub/#tag" 경로 키.
pub fn extract_localization(root: &Node, lang: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    walk(root, lang, &mut out);
    out
}
