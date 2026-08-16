use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static Vec<Regex> {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            r"<<[^<>]+>>",                             // Lingoona 치환/문법 토큰
            r"\^[A-Za-z]",                             // 성별/문법 마커
            r"\\n|\\t",                                // 리터럴 개행/탭
            r"｢|｣",                                   // 이스케이프 중괄호
            // TMP 리치텍스트. b/i/u는 단어 경계 필수 — <br>은 별개의 줄바꿈 태그라 제외
            r"</?(?:b|i|u)>|</?(?:color|size|sprite)[^<>]*>",
            r"#(?:autoLOC|LOC)_[A-Za-z0-9_]+",         // 태그 참조
            r"\{\d+\}|%(?:\.\d+)?[sdf]",               // 서식 자리표시자
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

/// Lingoona 선택/복수 토큰(`<<1[a/b/c]>>`)의 내부 텍스트는 번역 대상이므로
/// 구조만 남긴다: 파라미터 접두 + 선택지 개수 → `<<1[#3]>>`.
fn canonicalize(token: &str) -> String {
    if let Some(open) = token.find('[') {
        if token.starts_with("<<") && token.ends_with("]>>") {
            let prefix = &token[..open];
            let inner = &token[open + 1..token.len() - 3];
            let options = inner.split('/').count();
            return format!("{prefix}[#{options}]>>");
        }
    }
    token.to_string()
}

/// 보존 필수 토큰의 정렬된 다중집합.
pub fn extract_tokens(s: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for re in patterns() {
        for m in re.find_iter(s) {
            out.push(canonicalize(m.as_str()));
        }
    }
    out.sort();
    out
}
