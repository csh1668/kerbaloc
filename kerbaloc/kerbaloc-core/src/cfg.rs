use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub values: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Error)]
pub enum CfgError {
    #[error("중괄호 불균형 (줄 {line})")]
    UnbalancedBraces { line: usize },
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

pub fn parse(text: &str) -> Result<Node, CfgError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut nodes: Vec<Node> = vec![Node {
        name: String::new(),
        values: vec![],
        children: vec![],
    }];
    let mut pending_name: Option<String> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let mut rest = strip_comment(raw).trim();
        while !rest.is_empty() {
            let eq = rest.find('=');
            let brace = rest.find(['{', '}']);
            if let Some(e) = eq {
                if brace.map_or(true, |b| e < b) {
                    // 값 줄: 첫 '=' 기준 분리, 나머지 전체가 값
                    let name = rest[..e].trim().to_string();
                    let value = rest[e + 1..].trim().to_string();
                    nodes.last_mut().unwrap().values.push((name, value));
                    rest = "";
                    continue;
                }
            }
            if let Some(b) = brace {
                let before = rest[..b].trim();
                if !before.is_empty() {
                    pending_name = Some(before.to_string());
                }
                if rest.as_bytes()[b] == b'{' {
                    let name = pending_name.take().unwrap_or_default();
                    nodes.push(Node {
                        name,
                        values: vec![],
                        children: vec![],
                    });
                } else {
                    if nodes.len() < 2 {
                        return Err(CfgError::UnbalancedBraces { line: lineno + 1 });
                    }
                    let done = nodes.pop().unwrap();
                    nodes.last_mut().unwrap().children.push(done);
                }
                rest = rest[b + 1..].trim();
            } else {
                // 중괄호도 '='도 없는 토큰 = 다음 '{'의 노드 이름
                pending_name = Some(rest.to_string());
                rest = "";
            }
        }
    }
    if nodes.len() != 1 {
        return Err(CfgError::UnbalancedBraces {
            line: text.lines().count(),
        });
    }
    Ok(nodes.pop().unwrap())
}
