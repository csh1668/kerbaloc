use crate::tokens::extract_tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

fn find(rule: &'static str, severity: Severity, message: String) -> Finding {
    Finding {
        rule,
        severity,
        message,
    }
}

fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

/// 키 컨텍스트 포함 검증: validate_translation + DisplayName 2자 규칙 (부록 G).
pub fn validate_key_translation(key: &str, src: &str, dst: &str) -> Vec<Finding> {
    let mut out = validate_translation(src, dst);
    if key.to_lowercase().contains("_displayname") && dst.chars().count() < 2 {
        out.push(find(
            "displayname-too-short",
            Severity::Error,
            "2자 미만 DisplayName — GetShortName Substring 예외로 로딩 프리즈".into(),
        ));
    }
    out
}

pub fn validate_translation(src: &str, dst: &str) -> Vec<Finding> {
    let mut out = vec![];
    if dst.trim().is_empty() {
        out.push(find("empty", Severity::Error, "번역이 비어 있음".into()));
        return out;
    }
    if dst.contains('{') || dst.contains('}') {
        out.push(find(
            "raw-brace",
            Severity::Error,
            "원시 중괄호 — ｢｣로 이스케이프 필요".into(),
        ));
    }
    if dst.contains("//") {
        out.push(find(
            "comment-in-value",
            Severity::Error,
            "값 내 // 는 게임이 주석으로 절단".into(),
        ));
    }
    let ts = extract_tokens(src);
    let td = extract_tokens(dst);
    if ts != td {
        // \n·\t는 장식용이라 개수 불일치가 게임을 깨지 않는다(Dobie 실측) → Warning.
        // 나머지 토큰은 엔진/소비 코드가 파싱하므로 Error.
        let cls = |t: &str| -> (&'static str, Severity) {
            if t.starts_with('^') {
                ("caret-mismatch", Severity::Error)
            } else if t == "\\n" || t == "\\t" {
                ("newline-mismatch", Severity::Warning)
            } else if t.starts_with('<') && !t.starts_with("<<") {
                ("richtext-mismatch", Severity::Error)
            } else {
                ("token-mismatch", Severity::Error)
            }
        };
        let mut diff: Vec<&String> = ts.iter().filter(|t| !td.contains(t)).collect();
        diff.extend(td.iter().filter(|t| !ts.contains(t)));
        // 불일치 토큰들을 규칙별로 묶어 각각 보고 (개행 차이가 실제 오류를 가리지 않도록)
        let mut rules: Vec<(&'static str, Severity)> = diff.iter().map(|t| cls(t)).collect();
        rules.sort_by_key(|(r, _)| *r);
        rules.dedup_by_key(|(r, _)| *r);
        for (rule, severity) in rules {
            out.push(find(
                rule,
                severity,
                format!("토큰 불일치: 원문 {ts:?} vs 번역 {td:?}"),
            ));
        }
    }
    if !has_hangul(dst) && src == dst && src.chars().any(|c| c.is_ascii_alphabetic()) {
        out.push(find(
            "identical",
            Severity::Warning,
            "원문과 동일 — 미번역 간주".into(),
        ));
    }
    let (sl, dl) = (src.chars().count(), dst.chars().count());
    if sl >= 8 {
        let ratio = dl as f64 / sl as f64;
        if !(0.25..=2.0).contains(&ratio) {
            out.push(find(
                "length-ratio",
                Severity::Warning,
                format!("길이비 {ratio:.2}"),
            ));
        }
    }
    out
}
