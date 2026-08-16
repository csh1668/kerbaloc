use regex::Regex;
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct AvcInfo {
    pub name: Option<String>,
    pub version_raw: Option<String>,
    pub version: Option<String>,
    pub github: Option<(String, String)>,
}

fn balanced_prefix(t: &str) -> Option<&str> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in t.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&t[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn lenient_json(text: &str) -> Option<Value> {
    let t = text.trim_start_matches('\u{feff}');
    let start = t.find('{')?;
    let t = balanced_prefix(&t[start..])?;
    // 후행 쉼표 제거 (",}" / ",]")
    let re = Regex::new(r",\s*([}\]])").unwrap();
    let cleaned = re.replace_all(t, "$1").to_string();
    serde_json::from_str(&cleaned)
        .ok()
        .or_else(|| serde_json::from_str(t).ok())
}

fn get_ci<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

fn normalize(raw: &str) -> String {
    raw.trim().trim_start_matches(['v', 'V']).to_string()
}

pub fn parse_version_file(text: &str) -> Option<AvcInfo> {
    let v = lenient_json(text)?;
    let name = get_ci(&v, "NAME").and_then(|x| x.as_str()).map(String::from);
    let version_raw = match get_ci(&v, "VERSION") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(obj @ Value::Object(_)) => {
            let parts: Vec<String> = ["MAJOR", "MINOR", "PATCH", "BUILD"]
                .iter()
                .map_while(|k| get_ci(obj, k).and_then(|x| x.as_i64()).map(|n| n.to_string()))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("."))
            }
        }
        _ => None,
    };
    let github = get_ci(&v, "GITHUB").and_then(|g| {
        Some((
            get_ci(g, "USERNAME")?.as_str()?.to_string(),
            get_ci(g, "REPOSITORY")?.as_str()?.to_string(),
        ))
    });
    Some(AvcInfo {
        name,
        version: version_raw.as_deref().map(normalize),
        version_raw,
        github,
    })
}
