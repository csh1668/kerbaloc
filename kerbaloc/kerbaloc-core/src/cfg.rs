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
                    // 값 줄: 첫 '=' 기준 분리. 중괄호는 값 안에서도 구조 문자이므로
                    // 값은 다음 중괄호 직전까지만 취하고 나머지는 계속 처리한다.
                    let name = rest[..e].trim().to_string();
                    let end = brace.unwrap_or(rest.len());
                    let value = rest[e + 1..end].trim().to_string();
                    nodes.last_mut().unwrap().values.push((name, value));
                    rest = rest[end..].trim();
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

fn write_node(out: &mut String, node: &Node, depth: usize) {
    let tab = "\t".repeat(depth);
    out.push_str(&format!("{tab}{}\n{tab}{{\n", node.name));
    for (k, v) in &node.values {
        out.push_str(&format!("{tab}\t{k} = {v}\n"));
    }
    for c in &node.children {
        write_node(out, c, depth + 1);
    }
    out.push_str(&format!("{tab}}}\n"));
}

pub fn serialize(root: &Node) -> String {
    let mut out = String::new();
    for (k, v) in &root.values {
        out.push_str(&format!("{k} = {v}\n"));
    }
    for c in &root.children {
        write_node(&mut out, c, 0);
    }
    out
}

pub fn roundtrip_ok(text: &str) -> Result<bool, CfgError> {
    let a = parse(text)?;
    let b = parse(&serialize(&a))?;
    Ok(a == b)
}
