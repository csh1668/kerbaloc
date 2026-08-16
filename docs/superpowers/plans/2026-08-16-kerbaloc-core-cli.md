# KerbaLoc Core + CLI 구현 계획 (Plan 1/5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** KSP GameData를 스캔해 번역 대상을 식별하고, 번역팩을 원본 무수정으로 설치/제거하며, buildID64.txt로 언어를 전환하고, 팩을 검증하는 Rust 코어 라이브러리 + CLI — 실게임에서 한국어가 표시되는 최소 루프까지.

**Architecture:** Cargo workspace — `kerbaloc-core`(lib: cfg 파서, v1 해시, 검증기, 스캐너, 게임 적용)와 `kerbaloc-cli`(bin: clap 서브커맨드). CI와 스튜디오(Tauri)는 이후 플랜에서 같은 core를 소비한다. 모든 파일 I/O는 BOM 없는 UTF-8, 낡음 판정은 오직 소스 해시.

**Tech Stack:** Rust stable, serde/serde_json, sha2, clap v4, thiserror, unicode-normalization, walkdir, regex, zip, winreg, keyvalues-parser, insta(스냅샷 테스트)

**Spec:** `docs/superpowers/specs/2026-08-16-ksp-ko-redesign-design.md` + 부록 A~E (`appendix/`), 골든 데이터 `research/`

## Global Constraints

- 대상: KSP 1.12.x, Windows. 하위 호환 없음.
- 언어 코드는 정확히 `ko` (`ko-kr` 아님) — buildID64.txt와 Localization 노드명 동일.
- **원본 무수정**: 모드/스톡 파일을 절대 쓰지 않는다. 쓰기는 `buildID64.txt` language 줄과 `GameData/KerbaLoc/**` 만.
- 팩/DB 파일은 **BOM 없는 UTF-8, LF**. 게임 파일 읽기는 BOM 허용(제거 후 파싱).
- 해시는 `v1:sha256:<64hex>` 형식, 알고리즘 버전 접두 필수 (부록 D §3.5).
- 스캔 시 `GameData/KerbaLoc/**`와 `ModuleManager.ConfigCache`는 무조건 제외.
- ModId 문자셋 `^(local\.)?[A-Za-z0-9][A-Za-z0-9._-]*$`, 최대 128자.
- 커밋 메시지는 conventional commits(`feat:`/`test:`/`chore:`), 본 계획의 커밋 스텝 문구 사용.
- 에러는 `thiserror`로 타입화, `unwrap()`은 테스트 코드에서만.

## File Structure

```
kerbaloc/                       # 새 하위 디렉터리 (기존 src/ksp_translator는 참조용으로 보존)
├── Cargo.toml                  # [workspace] members = ["kerbaloc-core", "kerbaloc-cli"]
├── kerbaloc-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # pub mod 선언만
│       ├── cfg.rs              # ConfigNode 파서/직렬화/라운드트립 (Task 2-3)
│       ├── loc.rs              # Localization 노드 추출 (Task 4)
│       ├── hash.rs             # v1 정규화 해시 (Task 5)
│       ├── tokens.rs           # 보존 토큰 추출 (Task 6)
│       ├── validate.rs         # 번역 검증기 + 팩 검증 (Task 6, 12)
│       ├── avc.rs              # .version 관대 파서 (Task 7)
│       ├── ckan.rs             # CKAN registry 리더 (Task 8)
│       ├── scan.rs             # GameData 스캐너 + ModId 해석 (Task 9)
│       ├── game.rs             # buildID64 언어 전환 + Steam 감지 (Task 10)
│       ├── doctor.rs           # 오염 감지/백업 (Task 11)
│       └── pack.rs             # 팩 모델 + 설치/제거 (Task 12)
├── kerbaloc-cli/
│   ├── Cargo.toml
│   └── src/main.rs             # clap 서브커맨드 (Task 10부터 점진 추가)
└── tests-fixtures/             # 합성 GameData 트리 등 (각 태스크에서 생성)
```

`research/`의 실데이터(en-us.cfg 934KB, ja.cfg, Dobie 사전은 게임 폴더)는 골든 테스트 입력.

---

### Task 1: Cargo workspace 스캐폴드

**Files:**
- Create: `kerbaloc/Cargo.toml`, `kerbaloc/kerbaloc-core/Cargo.toml`, `kerbaloc/kerbaloc-core/src/lib.rs`, `kerbaloc/kerbaloc-cli/Cargo.toml`, `kerbaloc/kerbaloc-cli/src/main.rs`, `kerbaloc/rustfmt.toml`, `kerbaloc/.gitignore`

**Interfaces:**
- Produces: 빌드되는 빈 workspace. `kerbaloc_core` lib 크레이트명, `kerbaloc` bin 이름.

- [ ] **Step 1: rust 툴체인 확인**

Run: `cargo --version` — 없으면 `winget install Rustlang.Rustup; rustup default stable` 후 재확인.

- [ ] **Step 2: workspace 파일 작성**

`kerbaloc/Cargo.toml`:
```toml
[workspace]
members = ["kerbaloc-core", "kerbaloc-cli"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
sha2 = "0.10"
regex = "1"
unicode-normalization = "0.1"
walkdir = "2"
```

`kerbaloc/kerbaloc-core/Cargo.toml`:
```toml
[package]
name = "kerbaloc-core"
version.workspace = true
edition.workspace = true

[lib]
name = "kerbaloc_core"

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
sha2.workspace = true
regex.workspace = true
unicode-normalization.workspace = true
walkdir.workspace = true
```

`kerbaloc/kerbaloc-core/src/lib.rs`:
```rust
pub mod cfg;
```
(모듈 파일은 Task 2에서 생성하므로 지금은 `pub mod cfg;` 대신 빈 파일로 두고 Task 2에서 추가해도 된다 — 빌드가 통과하는 상태 유지가 우선.)

`kerbaloc/kerbaloc-cli/Cargo.toml`:
```toml
[package]
name = "kerbaloc-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "kerbaloc"
path = "src/main.rs"

[dependencies]
kerbaloc-core = { path = "../kerbaloc-core" }
clap = { version = "4", features = ["derive"] }
```

`kerbaloc/kerbaloc-cli/src/main.rs`:
```rust
fn main() {
    println!("kerbaloc 0.1.0");
}
```

`kerbaloc/.gitignore`:
```
target/
```

`kerbaloc/rustfmt.toml`:
```toml
edition = "2021"
```

- [ ] **Step 3: 빌드 확인**

Run: `cd kerbaloc; cargo build; cargo run -p kerbaloc-cli`
Expected: 빌드 성공, `kerbaloc 0.1.0` 출력. (lib.rs가 `pub mod cfg;`를 선언했다면 빈 `src/cfg.rs`도 만들어야 빌드된다.)

- [ ] **Step 4: Commit**

```bash
git add kerbaloc
git commit -m "chore: kerbaloc Cargo workspace 스캐폴드 (core lib + cli bin)"
```

---

### Task 2: ConfigNode 파서 (KSP 규칙 모사)

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/cfg.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs` (`pub mod cfg;`)
- Test: `kerbaloc/kerbaloc-core/tests/cfg_parse.rs`

**Interfaces:**
- Produces:
```rust
pub struct Node { pub name: String, pub values: Vec<(String, String)>, pub children: Vec<Node> }
pub enum CfgError { UnbalancedBraces { line: usize }, RawBraceInValue { line: usize } }  // thiserror
pub fn parse(text: &str) -> Result<Node, CfgError>   // 반환 = name=""인 합성 루트
```
- 규칙(스펙 탐색 결과 + 부록 D·E): `//` 이후는 줄 끝까지 주석 — **노드명 뒤(`en-us// 주석`)와 값 안에서도** 절단. BOM은 호출 전 제거를 가정하지 않고 parse가 직접 제거. `name { ... }` 인라인·별도 줄 `{` 모두 허용. 값은 첫 `=` 기준 분리 후 양쪽 trim. 값 내 `｢｣`는 일반 문자(파서는 통과), 원시 `{`/`}`가 값에 오면 KSP는 구조 문자로 읽으므로 파서도 동일하게 취급(그 결과 중괄호 불균형이면 에러).

- [ ] **Step 1: 실패하는 테스트 작성**

`kerbaloc/kerbaloc-core/tests/cfg_parse.rs`:
```rust
use kerbaloc_core::cfg::{parse, Node};

fn loc(root: &Node) -> &Node { &root.children[0] }

#[test]
fn parses_basic_localization_block() {
    let text = "Localization\n{\n\ten-us\n\t{\n\t\t#autoLOC_1 = Hello <<1>>\n\t}\n}\n";
    let root = parse(text).unwrap();
    let l = loc(&root);
    assert_eq!(l.name, "Localization");
    assert_eq!(l.children[0].name, "en-us");
    assert_eq!(l.children[0].values[0], ("#autoLOC_1".into(), "Hello <<1>>".into()));
}

#[test]
fn strips_inline_comment_after_node_name() {
    // 실존 사례: Dobie dictionary.cfg의 "en-us// 주석"
    let text = "Localization\n{\n\ten-us// Dobie 24.06.15\n\t{\n\t\t#a = b\n\t}\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(loc(&root).children[0].name, "en-us");
}

#[test]
fn truncates_value_at_comment() {
    let text = "N\n{\n\tkey = value // trailing\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(root.children[0].values[0].1, "value");
}

#[test]
fn inline_open_brace_on_name_line() {
    let text = "PART { name = fuelTank\n\ttitle = Tank\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(root.children[0].name, "PART");
    assert_eq!(root.children[0].values.len(), 2);
}

#[test]
fn value_may_contain_equals() {
    let text = "N\n{\n\tk = a = b\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(root.children[0].values[0], ("k".into(), "a = b".into()));
}

#[test]
fn strips_utf8_bom() {
    let text = "\u{feff}N\n{\n\tk = v\n}\n";
    assert!(parse(text).is_ok());
}

#[test]
fn unbalanced_braces_is_error() {
    assert!(parse("N\n{\n\tk = v\n").is_err());
}

#[test]
fn golden_stock_en_us_parses() {
    // research/stock-dictionary/en-us.cfg — 스톡 원본 11,932키
    let text = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../research/stock-dictionary/en-us.cfg")).unwrap();
    let root = parse(&text).unwrap();
    let en = &root.children[0].children[0];
    assert_eq!(en.name, "en-us");
    assert_eq!(en.values.len(), 11932);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cd kerbaloc; cargo test -p kerbaloc-core --test cfg_parse`
Expected: 컴파일 실패("cfg 모듈/parse 없음") — 이것이 이 단계의 실패다.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/cfg.rs`:
```rust
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
    let mut root = Node { name: String::new(), values: vec![], children: vec![] };
    let mut stack: Vec<Node> = vec![];
    let mut pending_name: Option<String> = None;
    let mut cur = &mut root as *mut Node; // 대신 스택 인덱스 방식으로 구현해도 됨

    // 안전한 구현: 스택에 소유권을 넣고 마지막에 되감는 방식
    let mut nodes: Vec<Node> = vec![Node { name: String::new(), values: vec![], children: vec![] }];
    let _ = (cur, &mut root);

    for (lineno, raw) in text.lines().enumerate() {
        let mut rest = strip_comment(raw).trim();
        while !rest.is_empty() {
            if let Some(eq) = rest.find('=') {
                let brace = rest.find(['{', '}']);
                if brace.map_or(true, |b| eq < b) {
                    // 값 줄: 첫 '=' 분리, '='가 중괄호보다 앞이면 나머지 전체가 값
                    let name = rest[..eq].trim().to_string();
                    let value = rest[eq + 1..].trim().to_string();
                    nodes.last_mut().unwrap().values.push((name, value));
                    rest = "";
                    continue;
                }
            }
            if let Some(b) = rest.find(['{', '}']) {
                let before = rest[..b].trim();
                if !before.is_empty() {
                    pending_name = Some(before.to_string());
                }
                if rest.as_bytes()[b] == b'{' {
                    let name = pending_name.take().unwrap_or_default();
                    nodes.push(Node { name, values: vec![], children: vec![] });
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
        return Err(CfgError::UnbalancedBraces { line: text.lines().count() });
    }
    Ok(nodes.pop().unwrap())
}
```
(`cur`/`stack`/`root` 잔재 변수는 제거하고 `nodes` 스택 방식만 남길 것.)

`lib.rs`에 `pub mod cfg;` 확인.

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p kerbaloc-core --test cfg_parse`
Expected: 8개 전부 PASS. 골든 테스트 실패 시 실제 키 수를 `research/` 데이터로 재확인하고 테스트 기대값이 아니라 파서를 의심할 것.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): KSP ConfigNode 파서 — 인라인 주석·값 내 주석 절단·BOM 처리"
```

---

### Task 3: 직렬화 + 라운드트립

**Files:**
- Modify: `kerbaloc/kerbaloc-core/src/cfg.rs`
- Test: `kerbaloc/kerbaloc-core/tests/cfg_roundtrip.rs`

**Interfaces:**
- Produces:
```rust
pub fn serialize(root: &Node) -> String            // 탭 들여쓰기, name\n{\n...\n}\n, 루트(name="")는 children만 출력
pub fn roundtrip_ok(text: &str) -> Result<bool, CfgError>  // parse→serialize→parse, AST 동등 비교
```
CI 검증 4단계(부록 E §3.2-A)와 팩 생성기의 공용 방어선.

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/cfg_roundtrip.rs`:
```rust
use kerbaloc_core::cfg::{parse, serialize, roundtrip_ok};

#[test]
fn serialize_then_reparse_equals() {
    let text = "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕 <<1>>\n\t\t#b = 줄\\n바꿈\n\t}\n}\n";
    let root = parse(text).unwrap();
    let out = serialize(&root);
    assert_eq!(parse(&out).unwrap(), root);
}

#[test]
fn roundtrip_ok_true_for_valid() {
    assert!(roundtrip_ok("N\n{\n\tk = v\n}\n").unwrap());
}

#[test]
fn golden_stock_roundtrips() {
    let text = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../research/stock-dictionary/en-us.cfg")).unwrap();
    assert!(roundtrip_ok(&text).unwrap());
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p kerbaloc-core --test cfg_roundtrip` → 컴파일 실패.

- [ ] **Step 3: 구현** — `cfg.rs`에 추가:

```rust
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
```

- [ ] **Step 4: 통과 확인** — `cargo test -p kerbaloc-core --test cfg_roundtrip` → 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): cfg 직렬화 및 AST 라운드트립 검증"
```

---

### Task 4: Localization 추출기

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/loc.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/loc_extract.rs`

**Interfaces:**
- Produces:
```rust
use std::collections::BTreeMap;
/// root 전체에서 Localization/<lang> 노드들을 찾아 합집합. 하위 노드는 "sub/#tag" 경로 키.
pub fn extract_localization(root: &cfg::Node, lang: &str) -> BTreeMap<String, String>
```
- Consumes: `cfg::Node`, `cfg::parse`.

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/loc_extract.rs`:
```rust
use kerbaloc_core::{cfg::parse, loc::extract_localization};

#[test]
fn extracts_lang_entries_including_nested() {
    let text = "Localization\n{\n\ten-us\n\t{\n\t\t#a = A\n\t\tsub\n\t\t{\n\t\t\t#b = B\n\t\t}\n\t}\n\tko\n\t{\n\t\t#a = 가\n\t}\n}\n";
    let root = parse(text).unwrap();
    let en = extract_localization(&root, "en-us");
    assert_eq!(en.get("#a").unwrap(), "A");
    assert_eq!(en.get("sub/#b").unwrap(), "B");
    let ko = extract_localization(&root, "ko");
    assert_eq!(ko.len(), 1);
}

#[test]
fn merges_multiple_localization_blocks() {
    // Tantares처럼 여러 파일/블록 분할 — 같은 root 아래 두 블록도 합집합
    let text = "Localization\n{\n\ten-us { #a = A }\n}\nLocalization\n{\n\ten-us { #b = B }\n}\n";
    let root = parse(text).unwrap();
    assert_eq!(extract_localization(&root, "en-us").len(), 2);
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p kerbaloc-core --test loc_extract` → 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/loc.rs`:
```rust
use crate::cfg::Node;
use std::collections::BTreeMap;

fn collect(node: &Node, prefix: &str, out: &mut BTreeMap<String, String>) {
    for (k, v) in &node.values {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}/{k}") };
        out.insert(key, v.clone());
    }
    for c in &node.children {
        let p = if prefix.is_empty() { c.name.clone() } else { format!("{prefix}/{}", c.name) };
        collect(c, &p, out);
    }
}

pub fn extract_localization(root: &Node, lang: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
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
    walk(root, lang, &mut out);
    out
}
```
(`collect`를 `walk`보다 위에 두고 `lib.rs`에 `pub mod loc;` 추가.)

- [ ] **Step 4: 통과 확인** — 2 PASS.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): Localization 노드 추출기 (다중 블록 합집합, 중첩 경로 키)"
```

---

### Task 5: v1 정규화 소스 해시

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/hash.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/hash.rs`

**Interfaces:**
- Produces (부록 D §3):
```rust
pub fn normalize_value(v: &str) -> String            // CRLF→LF, NFC, 줄끝 공백 제거, trim. 리터럴 \n·<<1>>·^·｢｣·내부 공백 보존
pub fn source_hash(entries: &BTreeMap<String, String>) -> String   // "v1:sha256:<hex>" — key\x1f norm(value)\x1e 연결
pub fn keys_hash(entries: &BTreeMap<String, String>) -> String     // 키 목록만
pub fn key_fingerprint(value: &str) -> String                       // norm(value) sha256 앞 8hex
```

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/hash.rs`:
```rust
use kerbaloc_core::hash::{normalize_value, source_hash, keys_hash, key_fingerprint};
use std::collections::BTreeMap;

fn m(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn hash_has_version_prefix() {
    let h = source_hash(&m(&[("#a", "x")]));
    assert!(h.starts_with("v1:sha256:"));
    assert_eq!(h.len(), "v1:sha256:".len() + 64);
}

#[test]
fn insensitive_to_crlf_and_trailing_ws() {
    let a = m(&[("#a", "line1\r\nline2  ")]);
    let b = m(&[("#a", "line1\nline2")]);
    assert_eq!(source_hash(&a), source_hash(&b));
}

#[test]
fn preserves_literal_backslash_n_and_tokens() {
    // 리터럴 "\n"(역슬래시+n)과 실제 개행은 다른 값이어야 함
    let a = m(&[("#a", "x\\ny <<1>> 커발^N ｢z｣")]);
    let b = m(&[("#a", "x\ny <<1>> 커발^N ｢z｣")]);
    assert_ne!(source_hash(&a), source_hash(&b));
}

#[test]
fn keys_hash_ignores_values() {
    assert_eq!(keys_hash(&m(&[("#a", "1")])), keys_hash(&m(&[("#a", "2")])));
    assert_ne!(keys_hash(&m(&[("#a", "1")])), keys_hash(&m(&[("#b", "1")])));
}

#[test]
fn fingerprint_is_8_hex() {
    let f = key_fingerprint("Hello");
    assert_eq!(f.len(), 8);
    assert!(f.chars().all(|c| c.is_ascii_hexdigit()));
}
```

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/hash.rs`:
```rust
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub fn normalize_value(v: &str) -> String {
    let v = v.replace("\r\n", "\n").replace('\r', "\n");
    let v: String = v.nfc().collect();
    let lines: Vec<&str> = v.split('\n').map(|l| l.trim_end()).collect();
    lines.join("\n").trim().to_string()
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn source_hash(entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    for (k, v) in entries {
        h.update(k.as_bytes());
        h.update([0x1f]);
        h.update(normalize_value(v).as_bytes());
        h.update([0x1e]);
    }
    format!("v1:sha256:{}", hex(&h.finalize()))
}

pub fn keys_hash(entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    for k in entries.keys() {
        h.update(k.as_bytes());
        h.update([0x1e]);
    }
    format!("v1:sha256:{}", hex(&h.finalize()))
}

pub fn key_fingerprint(value: &str) -> String {
    let mut h = Sha256::new();
    h.update(normalize_value(value).as_bytes());
    hex(&h.finalize())[..8].to_string()
}
```
(BTreeMap 자체가 키 정렬을 보장 — 별도 정렬 불필요. `lib.rs`에 `pub mod hash;`.)

- [ ] **Step 4: 통과 확인** — 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): v1 정규화 소스 해시 (source/keys/fingerprint)"
```

---

### Task 6: 토큰 추출 + 번역 검증기

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/tokens.rs`, `kerbaloc/kerbaloc-core/src/validate.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/validate.rs`

**Interfaces:**
- Produces (부록 B §4.1 / E §3.2-B):
```rust
// tokens.rs
pub fn extract_tokens(s: &str) -> Vec<String>   // 정렬된 다중집합: <<n>>, ^X, \n리터럴, ｢, ｣, 리치태그, #태그참조, {n}, %s류

// validate.rs
pub enum Severity { Error, Warning }
pub struct Finding { pub rule: &'static str, pub severity: Severity, pub message: String }
pub fn validate_translation(src: &str, dst: &str) -> Vec<Finding>
```
검사 목록(규칙 id): `empty`(E), `token-mismatch`(E), `raw-brace`(E), `caret-mismatch`(E), `newline-mismatch`(E), `richtext-mismatch`(E), `comment-in-value`(E — 값 내 `//`는 게임이 절단), `identical`(W — 원문에 라틴 있고 한글 없음), `length-ratio`(W — dst/src 문자수 [0.25, 2.0] 밖, src 8자 미만은 면제).

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/validate.rs`:
```rust
use kerbaloc_core::validate::{validate_translation, Severity};

fn errors(src: &str, dst: &str) -> Vec<String> {
    validate_translation(src, dst).into_iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .map(|f| f.rule.to_string()).collect()
}

#[test]
fn ok_translation_has_no_errors() {
    assert!(errors("Stage <<1>> ready.\\nGo", "<<1>>단 준비 완료.\\n출발").is_empty());
}

#[test]
fn missing_substitution_token() {
    assert_eq!(errors("Stage <<1>> of <<2>>", "<<1>>단"), vec!["token-mismatch"]);
}

#[test]
fn caret_marker_preserved() {
    assert!(errors("Kerbal^N", "커발^N").is_empty());
    assert_eq!(errors("Kerbal^N", "커발"), vec!["caret-mismatch"]);
}

#[test]
fn raw_brace_is_error() {
    assert!(errors("x", "한{글}").contains(&"raw-brace".to_string()));
}

#[test]
fn escaped_braces_ok_but_must_match() {
    assert!(errors("｢x｣", "｢한글｣").is_empty());
    assert!(errors("｢x｣", "한글").iter().any(|r| r == "token-mismatch"));
}

#[test]
fn literal_newline_count() {
    assert!(errors("a\\nb", "가나").iter().any(|r| r == "newline-mismatch"));
}

#[test]
fn comment_in_value_is_error() {
    assert!(errors("see docs", "참고: https://x.y//z").contains(&"comment-in-value".to_string()));
}

#[test]
fn richtext_tags_must_balance() {
    assert!(errors("<b>Hi</b>", "<b>안녕</b>").is_empty());
    assert!(errors("<b>Hi</b>", "<b>안녕").iter().any(|r| r == "richtext-mismatch"));
}

#[test]
fn empty_translation_is_error() {
    assert!(errors("Hi", "  ").contains(&"empty".to_string()));
}

#[test]
fn golden_dobie_dictionary_passes() {
    // 사람이 검증한 번역(Dobie)이 우리 검증기를 통과하지 못하면 검증기가 틀린 것 (부록 B §6)
    use kerbaloc_core::{cfg::parse, loc::extract_localization};
    let stock = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../research/stock-dictionary/en-us.cfg")).unwrap();
    let dobie_path = r"C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program\GameData\Squad\Localization\dictionary.cfg";
    let Ok(dobie) = std::fs::read_to_string(dobie_path) else { return; }; // 게임 미설치 환경은 스킵
    let en = extract_localization(&parse(&stock).unwrap(), "en-us");
    let ko = extract_localization(&parse(&dobie).unwrap(), "en-us"); // Dobie는 en-us 노드에 한국어
    let mut error_count = 0;
    for (k, dst) in &ko {
        if let Some(src) = en.get(k) {
            error_count += errors(src, dst).len();
        }
    }
    assert_eq!(error_count, 0, "Dobie 번역에서 검증기 오탐 {error_count}건");
}
```

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/tokens.rs`:
```rust
use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static Vec<Regex> {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            r"<<[^>]+>>",                 // Lingoona 치환/문법 토큰
            r"\^[A-Za-z]",                // 성별/문법 마커
            r"\\n|\\t",                   // 리터럴 개행/탭
            r"｢|｣",                      // 이스케이프 중괄호
            r"</?(?:b|i|u|color|size|sprite)[^>]*>", // TMP 리치텍스트
            r"#(?:autoLOC|LOC)_[A-Za-z0-9_]+",       // 태그 참조
            r"\{\d+\}|%(?:\.\d+)?[sdf]",  // 서식 자리표시자
        ]
        .iter().map(|p| Regex::new(p).unwrap()).collect()
    })
}

pub fn extract_tokens(s: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for re in patterns() {
        for m in re.find_iter(s) {
            out.push(m.as_str().to_string());
        }
    }
    out.sort();
    out
}
```

`kerbaloc/kerbaloc-core/src/validate.rs`:
```rust
use crate::tokens::extract_tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

#[derive(Debug)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

fn find(rule: &'static str, severity: Severity, message: String) -> Finding {
    Finding { rule, severity, message }
}

fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

pub fn validate_translation(src: &str, dst: &str) -> Vec<Finding> {
    let mut out = vec![];
    if dst.trim().is_empty() {
        out.push(find("empty", Severity::Error, "번역이 비어 있음".into()));
        return out;
    }
    if dst.contains('{') || dst.contains('}') {
        out.push(find("raw-brace", Severity::Error, "원시 중괄호 — ｢｣로 이스케이프 필요".into()));
    }
    if dst.contains("//") {
        out.push(find("comment-in-value", Severity::Error, "값 내 // 는 게임이 주석으로 절단".into()));
    }
    let ts: Vec<String> = extract_tokens(src);
    let td: Vec<String> = extract_tokens(dst);
    if ts != td {
        // 세분화된 규칙명: 어떤 부류가 어긋났는지 우선 보고
        let cls = |t: &str| -> &'static str {
            if t.starts_with('^') { "caret-mismatch" }
            else if t == "\\n" || t == "\\t" { "newline-mismatch" }
            else if t.starts_with('<') && !t.starts_with("<<") { "richtext-mismatch" }
            else { "token-mismatch" }
        };
        let mut missing: Vec<&String> = ts.iter().filter(|t| !td.contains(t)).collect();
        let extra: Vec<&String> = td.iter().filter(|t| !ts.contains(t)).collect();
        missing.extend(extra);
        let rule = missing.first().map(|t| cls(t)).unwrap_or("token-mismatch");
        out.push(find(rule, Severity::Error, format!("토큰 불일치: 원문 {ts:?} vs 번역 {td:?}")));
    }
    if !has_hangul(dst) && src == dst && src.chars().any(|c| c.is_ascii_alphabetic()) {
        out.push(find("identical", Severity::Warning, "원문과 동일 — 미번역 간주".into()));
    }
    let (sl, dl) = (src.chars().count(), dst.chars().count());
    if sl >= 8 {
        let ratio = dl as f64 / sl as f64;
        if !(0.25..=2.0).contains(&ratio) {
            out.push(find("length-ratio", Severity::Warning, format!("길이비 {ratio:.2}")));
        }
    }
    out
}
```
주의: 다중집합 비교는 정렬된 Vec 동등 비교로 충분(extract_tokens가 정렬 반환). `lib.rs`에 `pub mod tokens; pub mod validate;`.

- [ ] **Step 4: 통과 확인** — `cargo test -p kerbaloc-core --test validate`. **골든 테스트에서 오탐이 나오면 Dobie 데이터를 열어 원인 규칙을 완화**(예: 원문에 없던 리치태그를 번역이 추가하는 정당 사례) — 검증기 조정이 원칙, 테스트 기대값 완화는 금지.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): 보존 토큰 추출기 + 번역 검증기 9규칙 (Dobie 골든 통과)"
```

---

### Task 7: `.version` (KSP-AVC) 관대 파서

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/avc.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/avc.rs`

**Interfaces:**
- Produces (부록 D §0.4, §2.2):
```rust
pub struct AvcInfo {
    pub name: Option<String>,
    pub version_raw: Option<String>,      // 원본 그대로 보존
    pub version: Option<String>,          // normalize: v접두 제거, 1~4자리
    pub github: Option<(String, String)>, // (USERNAME, REPOSITORY)
}
pub fn parse_version_file(text: &str) -> Option<AvcInfo>  // 실패 시 None (조용한 폴백)
```

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/avc.rs`:
```rust
use kerbaloc_core::avc::parse_version_file;

#[test]
fn object_version() {
    let t = r#"{"NAME":"CRP","VERSION":{"MAJOR":1,"MINOR":4,"PATCH":2}}"#;
    let a = parse_version_file(t).unwrap();
    assert_eq!(a.version.as_deref(), Some("1.4.2"));
}

#[test]
fn string_version_with_v_prefix() {
    let a = parse_version_file(r#"{"NAME":"X","VERSION":"v1.2.0"}"#).unwrap();
    assert_eq!(a.version_raw.as_deref(), Some("v1.2.0"));
    assert_eq!(a.version.as_deref(), Some("1.2.0"));
}

#[test]
fn trailing_commas_tolerated() {
    // 실측: ASET 등 10/162 파일
    let t = "{\"NAME\":\"A\",\"VERSION\":{\"MAJOR\":2,\"MINOR\":0,\"PATCH\":2,},}";
    assert!(parse_version_file(t).is_some());
}

#[test]
fn bom_and_case_insensitive_keys() {
    let t = "\u{feff}{\"name\":\"A\",\"version\":\"3.3.1.0\",\"GITHUB\":{\"USERNAME\":\"u\",\"REPOSITORY\":\"r\"}}";
    let a = parse_version_file(t).unwrap();
    assert_eq!(a.version.as_deref(), Some("3.3.1.0"));
    assert_eq!(a.github, Some(("u".into(), "r".into())));
}

#[test]
fn garbage_returns_none() {
    assert!(parse_version_file("not json at all").is_none());
}
```

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/avc.rs`:
```rust
use regex::Regex;
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct AvcInfo {
    pub name: Option<String>,
    pub version_raw: Option<String>,
    pub version: Option<String>,
    pub github: Option<(String, String)>,
}

fn lenient_json(text: &str) -> Option<Value> {
    let t = text.trim_start_matches('\u{feff}');
    let start = t.find('{')?;
    let t = &t[start..];
    // 후행 쉼표 제거: ",}" / ",]" (문자열 내부의 쉼표는 이 패턴에 걸리지 않도록 단순 접근 후 실패 시 원본 시도)
    let re = Regex::new(r",\s*([}\]])").unwrap();
    let cleaned = re.replace_all(t, "$1").to_string();
    serde_json::from_str(&cleaned).ok()
        .or_else(|| serde_json::from_str(t).ok())
        .or_else(|| {
            // 과다 닫는 중괄호: 균형 지점까지 절단 후 재시도
            let mut depth = 0i32;
            for (i, c) in cleaned.char_indices() {
                if c == '{' { depth += 1; }
                if c == '}' { depth -= 1; if depth == 0 { return serde_json::from_str(&cleaned[..=i]).ok(); } }
            }
            None
        })
}

fn get_ci<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
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
            let parts: Vec<String> = ["MAJOR", "MINOR", "PATCH", "BUILD"].iter()
                .map_while(|k| get_ci(obj, k).and_then(|x| x.as_i64()).map(|n| n.to_string()))
                .collect();
            if parts.is_empty() { None } else { Some(parts.join(".")) }
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
```
`lib.rs`에 `pub mod avc;`.

- [ ] **Step 4: 통과 확인** — 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): KSP-AVC .version 관대 파서 (후행쉼표·문자열형·BOM·대소문자)"
```

---

### Task 8: CKAN registry 리더

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/ckan.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/ckan.rs`

**Interfaces:**
- Produces (부록 D §0.1):
```rust
pub struct CkanRegistry { /* 내부: files_ci: HashMap<String,String>(소문자 경로→identifier), modules: HashMap<identifier, CkanModule> */ }
pub struct CkanModule { pub identifier: String, pub name: Option<String>, pub version: Option<String>, pub localizations: Vec<String> }
impl CkanRegistry {
    pub fn load(ksp_root: &Path) -> Option<CkanRegistry>          // <root>/CKAN/registry.json, 실패 시 None
    pub fn owner_of(&self, rel_path: &str) -> Option<&str>        // "GameData/..." 슬래시 경로, 대소문자 무시
    pub fn module(&self, identifier: &str) -> Option<&CkanModule>
}
```
`available_modules`/`download_counts`는 읽지 않는다(레거시).

- [ ] **Step 1: 실패하는 테스트** (합성 registry 픽스처)

`kerbaloc/kerbaloc-core/tests/ckan.rs`:
```rust
use kerbaloc_core::ckan::CkanRegistry;
use std::fs;

fn make_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(d.path().join("CKAN/registry.json"), r#"{
        "installed_modules": {
            "Toolbar": {"source_module": {"identifier":"Toolbar","name":"Toolbar Continued","version":"1.8.1.2","localizations":["en-us","ja"]}}
        },
        "installed_files": {
            "GameData/000_Toolbar/Localization/en-us.cfg": "Toolbar",
            "GameData/000_Toolbar/Toolbar.dll": "Toolbar"
        }
    }"#).unwrap();
    d
}

#[test]
fn loads_and_resolves_owner_case_insensitive() {
    let d = make_root();
    let r = CkanRegistry::load(d.path()).unwrap();
    assert_eq!(r.owner_of("gamedata/000_toolbar/localization/EN-US.CFG"), Some("Toolbar"));
    assert_eq!(r.owner_of("GameData/Unknown/x.cfg"), None);
}

#[test]
fn module_metadata() {
    let d = make_root();
    let r = CkanRegistry::load(d.path()).unwrap();
    let m = r.module("Toolbar").unwrap();
    assert_eq!(m.version.as_deref(), Some("1.8.1.2"));
    assert_eq!(m.localizations, vec!["en-us", "ja"]);
}

#[test]
fn missing_or_broken_registry_is_none() {
    let d = tempfile::tempdir().unwrap();
    assert!(CkanRegistry::load(d.path()).is_none());
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(d.path().join("CKAN/registry.json"), "{broken").unwrap();
    assert!(CkanRegistry::load(d.path()).is_none());
}
```
`kerbaloc-core/Cargo.toml`의 `[dev-dependencies]`에 `tempfile = "3"` 추가.

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/ckan.rs`:
```rust
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
                let get = |k: &str| sm.and_then(|s| s.get(k)).and_then(|x| x.as_str()).map(String::from);
                let locs = sm.and_then(|s| s.get("localizations")).and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                modules.insert(ident.clone(), CkanModule {
                    identifier: ident.clone(),
                    name: get("name"),
                    version: get("version"),
                    localizations: locs,
                });
            }
        }
        Some(CkanRegistry { files_ci, modules })
    }

    pub fn owner_of(&self, rel_path: &str) -> Option<&str> {
        self.files_ci.get(&rel_path.replace('\\', "/").to_lowercase()).map(String::as_str)
    }

    pub fn module(&self, identifier: &str) -> Option<&CkanModule> {
        self.modules.get(identifier)
    }
}
```
`lib.rs`에 `pub mod ckan;`.

- [ ] **Step 4: 통과 확인** — 3 PASS.

- [ ] **Step 5: 실제 registry.json 스모크 (수동)**

Run: `cargo test -p kerbaloc-core --test ckan -- --nocapture` 후, 별도 예제로 실제 파일 확인:
```bash
cargo run -p kerbaloc-cli 2>/dev/null || true
```
(실제 4.8MB registry 로드는 Task 9의 스캐너 통합 스모크에서 확인한다.)

- [ ] **Step 6: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): CKAN registry 리더 (installed_files 역인덱스, 대소문자 무시)"
```

---

### Task 9: GameData 스캐너 + ModId 해석

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/scan.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`
- Test: `kerbaloc/kerbaloc-core/tests/scan.rs`

**Interfaces:**
- Consumes: `cfg::parse`, `loc::extract_localization`, `hash::{source_hash, keys_hash}`, `avc::parse_version_file`, `ckan::CkanRegistry`
- Produces (부록 D §1):
```rust
pub enum IdSource { Ckan, Stock, AvcGithub, AvcStem, Folder }
pub struct VersionInfo { pub raw: Option<String>, pub source: &'static str /* "ckan"|"avc"|"unknown" */ }
pub struct ModUnit {
    pub mod_id: String,
    pub id_source: IdSource,
    pub display_name: String,
    pub version: VersionInfo,
    pub files: Vec<std::path::PathBuf>,          // 이 유닛이 소유한 로컬라이제이션 소스 cfg
    pub entries: std::collections::BTreeMap<String, String>,  // en-us 합집합
    pub source_hash: String,
    pub keys_hash: String,
    pub official_langs: Vec<String>,             // 파일에서 실제 발견된 en-us 외 언어 노드
}
pub fn scan_gamedata(ksp_root: &Path) -> Vec<ModUnit>
pub fn slug(s: &str) -> String                   // 부록 D §1.2: 접두 제거, 아포스트로피 제거, 비영숫자→'-'
```

- [ ] **Step 1: 실패하는 테스트** (합성 GameData 트리)

`kerbaloc/kerbaloc-core/tests/scan.rs`:
```rust
use kerbaloc_core::scan::{scan_gamedata, slug, IdSource};
use std::fs;

fn write(p: &std::path::Path, rel: &str, content: &str) {
    let f = p.join(rel);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(f, content).unwrap();
}

const LOC: &str = "Localization\n{\n\ten-us\n\t{\n\t\t#LOC_A_x = Hello\n\t}\n\tja\n\t{\n\t\t#LOC_A_x = こんにちは\n\t}\n}\n";

fn make_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    // CKAN 소유 모드 (폴더명 ≠ identifier)
    write(d.path(), "GameData/000_Toolbar/Localization/en-us.cfg", LOC);
    fs::create_dir_all(d.path().join("CKAN")).unwrap();
    fs::write(d.path().join("CKAN/registry.json"), r#"{
      "installed_modules": {"Toolbar": {"source_module": {"identifier":"Toolbar","name":"Toolbar Continued","version":"1.8.1.2","localizations":["en-us","ja"]}}},
      "installed_files": {"GameData/000_Toolbar/Localization/en-us.cfg": "Toolbar"}
    }"#).unwrap();
    // 수동 설치 + .version(GITHUB)
    write(d.path(), "GameData/MyMod/Lang/strings.cfg", LOC);
    write(d.path(), "GameData/MyMod/MyMod.version",
        r#"{"NAME":"MyMod","VERSION":"2.0.0","GITHUB":{"USERNAME":"u","REPOSITORY":"CoolMod"}}"#);
    // 스톡
    write(d.path(), "GameData/Squad/Localization/dictionary.cfg", LOC);
    // 우리 산출물(제외 대상)
    write(d.path(), "GameData/KerbaLoc/ko/X/Localization/ko.cfg",
        "Localization\n{\n\tko\n\t{\n\t\t#a = 가\n\t}\n}\n");
    d
}

#[test]
fn resolves_modids_by_priority() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let ids: Vec<&str> = units.iter().map(|u| u.mod_id.as_str()).collect();
    assert!(ids.contains(&"Toolbar"));          // CKAN identifier (폴더명 000_Toolbar 아님)
    assert!(ids.contains(&"local.CoolMod"));    // AVC GITHUB repo
    assert!(ids.contains(&"Squad"));            // 예약 스톡
    assert!(!ids.iter().any(|i| i.contains("KerbaLoc"))); // 자기 산출물 제외
}

#[test]
fn unit_carries_hash_version_and_official_langs() {
    let d = make_root();
    let units = scan_gamedata(d.path());
    let t = units.iter().find(|u| u.mod_id == "Toolbar").unwrap();
    assert!(t.source_hash.starts_with("v1:sha256:"));
    assert_eq!(t.version.raw.as_deref(), Some("1.8.1.2"));
    assert_eq!(t.version.source, "ckan");
    assert!(t.official_langs.contains(&"ja".to_string()));
    assert_eq!(t.entries.len(), 1);
}

#[test]
fn slug_rules() {
    assert_eq!(slug("000_Toolbar"), "Toolbar");
    assert_eq!(slug("zzz_Deferred"), "Deferred");
    assert_eq!(slug("Adiri's TUFX Profiles"), "Adiris-TUFX-Profiles");
    assert_eq!(slug("[x]_Science!"), "x-Science");
}
```

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/scan.rs`:
```rust
use crate::{avc, cfg, ckan::CkanRegistry, hash, loc};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IdSource { Ckan, Stock, AvcGithub, AvcStem, Folder }

#[derive(Debug)]
pub struct VersionInfo { pub raw: Option<String>, pub source: &'static str }

#[derive(Debug)]
pub struct ModUnit {
    pub mod_id: String,
    pub id_source: IdSource,
    pub display_name: String,
    pub version: VersionInfo,
    pub files: Vec<PathBuf>,
    pub entries: BTreeMap<String, String>,
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
    if out.is_empty() { "unnamed".into() } else { out }
}

const OFFICIAL_LANGS: &[&str] = &["es-es", "ja", "ru", "zh-cn", "de-de", "fr-fr", "it-it", "pt-br"];

struct Owner { key: String, id_source: IdSource, display: String, version: VersionInfo }

fn find_avc(top_dir: &Path, file: &Path) -> Option<(avc::AvcInfo, PathBuf)> {
    // top 폴더 하위 전체의 .version 수집, file과 공통 조상이 가장 깊은 것 선택 (부록 D 엣지 18-19)
    let mut best: Option<(avc::AvcInfo, PathBuf, usize)> = None;
    for e in WalkDir::new(top_dir).into_iter().filter_map(Result::ok) {
        if e.path().extension().is_some_and(|x| x == "version") {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Some(info) = avc::parse_version_file(&text) {
                    let anc = e.path().parent().unwrap();
                    let depth = file.ancestors().filter(|a| a.starts_with(anc)).count();
                    let anc_depth = anc.components().count();
                    if file.starts_with(anc) || anc_depth == top_dir.components().count() {
                        let score = if file.starts_with(anc) { anc_depth + depth } else { 0 };
                        if best.as_ref().map_or(true, |(_, _, s)| score > *s) {
                            best = Some((info, e.path().to_path_buf(), score));
                        }
                    }
                }
            }
        }
    }
    best.map(|(i, p, _)| (i, p))
}

fn resolve_owner(ksp_root: &Path, file: &Path, ckan: Option<&CkanRegistry>) -> Owner {
    let rel = file.strip_prefix(ksp_root).unwrap().to_string_lossy().replace('\\', "/");
    if let Some(reg) = ckan {
        if let Some(id) = reg.owner_of(&rel) {
            let m = reg.module(id);
            return Owner {
                key: id.to_string(),
                id_source: IdSource::Ckan,
                display: m.and_then(|m| m.name.clone()).unwrap_or_else(|| id.to_string()),
                version: VersionInfo { raw: m.and_then(|m| m.version.clone()), source: "ckan" },
            };
        }
    }
    let parts: Vec<String> = file.strip_prefix(ksp_root.join("GameData")).unwrap()
        .components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let top = parts[0].clone();
    if top == "Squad" {
        return Owner { key: "Squad".into(), id_source: IdSource::Stock,
            display: "Kerbal Space Program (스톡)".into(),
            version: VersionInfo { raw: None, source: "unknown" } };
    }
    if top == "SquadExpansion" {
        let id = match parts.get(1).map(String::as_str) {
            Some("MakingHistory") => "MakingHistory-DLC",
            Some("Serenity") => "BreakingGround-DLC",
            _ => "SquadExpansion",
        };
        return Owner { key: id.into(), id_source: IdSource::Stock, display: id.into(),
            version: VersionInfo { raw: None, source: "unknown" } };
    }
    let top_dir = ksp_root.join("GameData").join(&top);
    if top_dir.is_dir() {
        if let Some((info, _)) = find_avc(&top_dir, file) {
            let version = VersionInfo { raw: info.version.clone(), source: "avc" };
            if let Some((_, repo)) = &info.github {
                return Owner { key: format!("local.{}", slug(repo)), id_source: IdSource::AvcGithub,
                    display: info.name.clone().unwrap_or_else(|| top.clone()), version };
            }
            if let Some(name) = &info.name {
                return Owner { key: format!("local.{}", slug(name)), id_source: IdSource::AvcStem,
                    display: name.clone(), version };
            }
        }
    }
    Owner { key: format!("local.{}", slug(&top)), id_source: IdSource::Folder,
        display: top.clone(), version: VersionInfo { raw: None, source: "unknown" } }
}

pub fn scan_gamedata(ksp_root: &Path) -> Vec<ModUnit> {
    let gamedata = ksp_root.join("GameData");
    let ckan = CkanRegistry::load(ksp_root);
    let mut groups: BTreeMap<String, (Owner, Vec<PathBuf>, BTreeMap<String, String>, Vec<String>)> = BTreeMap::new();

    for e in WalkDir::new(&gamedata).into_iter().filter_map(Result::ok) {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "cfg") { continue; }
        let rel = p.strip_prefix(&gamedata).unwrap().to_string_lossy().replace('\\', "/");
        if rel.starts_with("KerbaLoc/") { continue; }
        let Ok(text) = std::fs::read_to_string(p) else { continue; };
        let Ok(root) = cfg::parse(&text) else { continue; };
        let entries = loc::extract_localization(&root, "en-us");
        if entries.is_empty() { continue; }
        let owner = resolve_owner(ksp_root, p, ckan.as_ref());
        let langs: Vec<String> = OFFICIAL_LANGS.iter()
            .filter(|l| !loc::extract_localization(&root, l).is_empty())
            .map(|l| l.to_string()).collect();
        let g = groups.entry(owner.key.clone())
            .or_insert_with(|| (owner, vec![], BTreeMap::new(), vec![]));
        g.1.push(p.to_path_buf());
        g.2.extend(entries);
        for l in langs { if !g.3.contains(&l) { g.3.push(l); } }
    }

    groups.into_iter().map(|(key, (owner, files, entries, official_langs))| ModUnit {
        source_hash: hash::source_hash(&entries),
        keys_hash: hash::keys_hash(&entries),
        mod_id: key,
        id_source: owner.id_source,
        display_name: owner.display,
        version: owner.version,
        files, entries, official_langs,
    }).collect()
}
```
`kerbaloc-core/Cargo.toml` `[dev-dependencies]` `tempfile = "3"` (Task 8에서 이미 추가). `lib.rs`에 `pub mod scan;`.

- [ ] **Step 4: 통과 확인** — 3 PASS. `find_avc`의 스코프 점수 로직이 테스트를 못 넘기면 "file.starts_with(anc)인 것 중 anc가 가장 깊은 것, 없으면 None"으로 단순화할 것 — 그것이 부록 D의 정의다.

- [ ] **Step 5: 실설치본 스모크**

`kerbaloc-cli/src/main.rs`를 임시 확장하지 말고, 통합 테스트로:
```bash
cargo test -p kerbaloc-core --test scan -- --nocapture
```
이후 Task 13의 `kerbaloc scan` CLI에서 실제 게임 폴더로 확인한다.

- [ ] **Step 6: Commit**

```bash
git add kerbaloc
git commit -m "feat(core): GameData 스캐너 — CKAN→스톡→AVC→폴더 ModId 해석, 유닛별 해시"
```

---

### Task 10: 언어 전환 + Steam 감지 + CLI 뼈대

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/game.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`, `kerbaloc/kerbaloc-cli/src/main.rs`, `kerbaloc/kerbaloc-cli/Cargo.toml`
- Test: `kerbaloc/kerbaloc-core/tests/game.rs`

**Interfaces:**
- Produces:
```rust
pub fn read_language(ksp_root: &Path) -> Option<String>            // buildID64.txt의 language 값
pub fn set_language(ksp_root: &Path, lang: &str) -> std::io::Result<()>  // language 줄만 교체, 나머지 바이트 보존
pub fn detect_ksp_roots() -> Vec<PathBuf>                          // winreg SteamPath + libraryfolders.vdf
```
CLI: `kerbaloc status|enable|disable|scan [--root <path>]` — root 미지정 시 detect 결과 1개면 자동 채택.

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/game.rs`:
```rust
use kerbaloc_core::game::{read_language, set_language};
use std::fs;

const BUILDID: &str = "[config]\r\nbuild id = 03190\r\n2022.12.12 at 14:02:28 PST\r\nBranch: master\r\nlanguage = en-us\r\ndistribution name = Steam\r\n";

#[test]
fn read_and_switch_language_preserving_other_lines() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("buildID64.txt"), BUILDID).unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("en-us"));
    set_language(d.path(), "ko").unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("ko"));
    let text = fs::read_to_string(d.path().join("buildID64.txt")).unwrap();
    assert!(text.contains("build id = 03190"));
    assert!(text.contains("distribution name = Steam"));
    set_language(d.path(), "en-us").unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("en-us"));
}

#[test]
fn missing_file_returns_none() {
    let d = tempfile::tempdir().unwrap();
    assert!(read_language(d.path()).is_none());
}
```

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/game.rs`:
```rust
use std::path::{Path, PathBuf};

pub fn read_language(ksp_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(ksp_root.join("buildID64.txt")).ok()?;
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "language" {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub fn set_language(ksp_root: &Path, lang: &str) -> std::io::Result<()> {
    let path = ksp_root.join("buildID64.txt");
    let text = std::fs::read_to_string(&path)?;
    let out: Vec<String> = text.lines().map(|line| {
        match line.split_once('=') {
            Some((k, _)) if k.trim() == "language" => format!("{}= {lang}", k),
            _ => line.to_string(),
        }
    }).collect();
    // 원본이 CRLF였으면 CRLF 유지
    let sep = if text.contains("\r\n") { "\r\n" } else { "\n" };
    std::fs::write(&path, out.join(sep) + sep)
}

#[cfg(windows)]
pub fn detect_ksp_roots() -> Vec<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let mut roots = vec![];
    let Ok(steam) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") else { return roots; };
    let Ok(path): Result<String, _> = steam.get_value("SteamPath") else { return roots; };
    let steam_dir = PathBuf::from(path);
    let mut libs = vec![steam_dir.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam_dir.join("steamapps/libraryfolders.vdf")) {
        if let Ok(kv) = keyvalues_parser::Vdf::parse(&vdf) {
            fn walk(obj: &keyvalues_parser::Obj, libs: &mut Vec<PathBuf>) {
                for (k, vals) in obj.iter() {
                    for v in vals {
                        match v {
                            keyvalues_parser::Value::Str(s) if k == "path" => libs.push(PathBuf::from(s.as_ref())),
                            keyvalues_parser::Value::Obj(o) => walk(o, libs),
                            _ => {}
                        }
                    }
                }
            }
            if let keyvalues_parser::Value::Obj(o) = &kv.value { walk(o, &mut libs); }
        }
    }
    for lib in libs {
        let cand = lib.join("steamapps/common/Kerbal Space Program");
        if cand.join("buildID64.txt").is_file() && !roots.contains(&cand) {
            roots.push(cand);
        }
    }
    roots
}

#[cfg(not(windows))]
pub fn detect_ksp_roots() -> Vec<PathBuf> { vec![] }
```
`kerbaloc-core/Cargo.toml`에 (Windows 전용):
```toml
[target.'cfg(windows)'.dependencies]
winreg = "0.55"
keyvalues-parser = "0.2"
```

`kerbaloc/kerbaloc-cli/src/main.rs` 교체:
```rust
use clap::{Parser, Subcommand};
use kerbaloc_core::{game, scan};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "kerbaloc", version)]
struct Cli {
    /// KSP 설치 경로 (미지정 시 Steam에서 자동 감지)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 현재 언어/설치 상태 표시
    Status,
    /// 게임 언어를 ko로 전환
    Enable,
    /// 게임 언어를 en-us로 복원
    Disable,
    /// GameData 스캔: 번역 대상 모드 목록
    Scan,
}

fn resolve_root(cli_root: Option<PathBuf>) -> PathBuf {
    if let Some(r) = cli_root { return r; }
    let roots = game::detect_ksp_roots();
    match roots.len() {
        1 => roots.into_iter().next().unwrap(),
        0 => { eprintln!("KSP 설치를 찾지 못했습니다. --root 로 지정하세요."); std::process::exit(1); }
        _ => { eprintln!("KSP 설치가 여러 개입니다. --root 로 지정하세요:"); 
               for r in roots { eprintln!("  {}", r.display()); } std::process::exit(1); }
    }
}

fn main() {
    let cli = Cli::parse();
    let root = resolve_root(cli.root);
    match cli.cmd {
        Cmd::Status => {
            println!("KSP: {}", root.display());
            println!("언어: {}", game::read_language(&root).unwrap_or_else(|| "?".into()));
        }
        Cmd::Enable => {
            game::set_language(&root, "ko").expect("buildID64.txt 쓰기 실패");
            println!("언어를 ko로 전환했습니다. 게임을 재시작하세요.");
        }
        Cmd::Disable => {
            game::set_language(&root, "en-us").expect("buildID64.txt 쓰기 실패");
            println!("언어를 en-us로 복원했습니다.");
        }
        Cmd::Scan => {
            let units = scan::scan_gamedata(&root);
            println!("{:<40} {:<10} {:>6}  {}", "ModId", "버전", "키수", "해시");
            for u in &units {
                println!("{:<40} {:<10} {:>6}  {}", u.mod_id,
                    u.version.raw.as_deref().unwrap_or("-"),
                    u.entries.len(), &u.source_hash[..22]);
            }
            println!("총 {}개 유닛", units.len());
        }
    }
}
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p kerbaloc-core --test game` → 2 PASS.
Run: `cargo run -p kerbaloc-cli -- status` → 실제 설치 감지 + 현재 언어 출력.
Run: `cargo run -p kerbaloc-cli -- scan` → 실설치본(225 모드 디렉터리)에서 유닛 목록·키수 출력, 패닉 없음. Squad/Toolbar류 ModId가 부록 D 기대와 일치하는지 눈으로 확인.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat: 언어 전환(enable/disable)·Steam 감지·scan CLI"
```

---

### Task 11: doctor — 오염 감지 + 백업

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/doctor.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`, `kerbaloc/kerbaloc-cli/src/main.rs`
- Test: `kerbaloc/kerbaloc-core/tests/doctor.rs`

**Interfaces:**
- Consumes: `cfg::parse`, `loc::extract_localization`
- Produces:
```rust
pub struct Pollution { pub path: PathBuf, pub korean_values: usize, pub total: usize }
pub fn detect_pollution(ksp_root: &Path) -> Vec<Pollution>   // en-us 노드 값에 한글 포함 cfg 목록
pub fn backup_polluted(ksp_root: &Path, out_zip: &Path, items: &[Pollution]) -> std::io::Result<usize>
```
CLI: `kerbaloc doctor [--backup <zip>]` — 목록 표시, `--backup` 시 zip 생성 후 복원 안내 출력(Squad→Steam 무결성 검사, 모드→CKAN/재다운로드).

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/doctor.rs`:
```rust
use kerbaloc_core::doctor::{detect_pollution, backup_polluted};
use std::fs;

#[test]
fn detects_korean_in_en_us_and_backs_up() {
    let d = tempfile::tempdir().unwrap();
    let gd = d.path().join("GameData");
    fs::create_dir_all(gd.join("A/Localization")).unwrap();
    fs::write(gd.join("A/Localization/en-us.cfg"),
        "Localization\n{\n\ten-us\n\t{\n\t\t#a = 안녕하세요\n\t}\n}\n").unwrap();
    fs::create_dir_all(gd.join("B/Localization")).unwrap();
    fs::write(gd.join("B/Localization/en-us.cfg"),
        "Localization\n{\n\ten-us\n\t{\n\t\t#a = Hello\n\t}\n}\n").unwrap();

    let p = detect_pollution(d.path());
    assert_eq!(p.len(), 1);
    assert!(p[0].path.ends_with("A/Localization/en-us.cfg") || p[0].path.to_string_lossy().contains("A"));
    assert_eq!(p[0].korean_values, 1);

    let zip_path = d.path().join("backup.zip");
    let n = backup_polluted(d.path(), &zip_path, &p).unwrap();
    assert_eq!(n, 1);
    assert!(zip_path.is_file());
    let f = fs::File::open(&zip_path).unwrap();
    let mut z = zip::ZipArchive::new(f).unwrap();
    assert_eq!(z.len(), 1);
    assert!(z.by_index(0).unwrap().name().contains("en-us.cfg"));
}
```
`kerbaloc-core/Cargo.toml` `[dependencies]`에 `zip = { version = "2", default-features = false, features = ["deflate"] }`, `[dev-dependencies]` `tempfile`.

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/doctor.rs`:
```rust
use crate::{cfg, loc};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Pollution { pub path: PathBuf, pub korean_values: usize, pub total: usize }

fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

pub fn detect_pollution(ksp_root: &Path) -> Vec<Pollution> {
    let gamedata = ksp_root.join("GameData");
    let mut out = vec![];
    for e in WalkDir::new(&gamedata).into_iter().filter_map(Result::ok) {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "cfg") { continue; }
        let rel = p.strip_prefix(&gamedata).unwrap().to_string_lossy().replace('\\', "/");
        if rel.starts_with("KerbaLoc/") { continue; }
        let Ok(text) = std::fs::read_to_string(p) else { continue; };
        let Ok(root) = cfg::parse(&text) else { continue; };
        let en = loc::extract_localization(&root, "en-us");
        if en.is_empty() { continue; }
        let korean = en.values().filter(|v| has_hangul(v)).count();
        if korean > 0 {
            out.push(Pollution { path: p.to_path_buf(), korean_values: korean, total: en.len() });
        }
    }
    out
}

pub fn backup_polluted(ksp_root: &Path, out_zip: &Path, items: &[Pollution]) -> std::io::Result<usize> {
    let f = std::fs::File::create(out_zip)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    let mut n = 0;
    for it in items {
        let rel = it.path.strip_prefix(ksp_root).unwrap().to_string_lossy().replace('\\', "/");
        z.start_file(rel, opts)?;
        z.write_all(&std::fs::read(&it.path)?)?;
        n += 1;
    }
    z.finish()?;
    Ok(n)
}
```

`main.rs`의 `Cmd`에 추가:
```rust
    /// 구방식 번역 오염(en-us 노드 내 한글) 감지 및 백업
    Doctor {
        #[arg(long)]
        backup: Option<PathBuf>,
    },
```
매치 팔:
```rust
        Cmd::Doctor { backup } => {
            let items = kerbaloc_core::doctor::detect_pollution(&root);
            if items.is_empty() {
                println!("오염 없음 — en-us 노드에 한글이 든 파일이 없습니다.");
            } else {
                for it in &items {
                    println!("{}  ({}/{}키 한글)", it.path.display(), it.korean_values, it.total);
                }
                println!("총 {}개 파일이 구방식 번역으로 오염되어 있습니다.", items.len());
                if let Some(zip) = backup {
                    let n = kerbaloc_core::doctor::backup_polluted(&root, &zip, &items).expect("백업 실패");
                    println!("{n}개 파일을 {}에 백업했습니다.", zip.display());
                }
                println!("복원 방법: Squad/SquadExpansion → Steam 무결성 검사, 모드 → CKAN 재설치 또는 재다운로드.");
            }
        }
```

- [ ] **Step 4: 통과 확인** — 테스트 PASS 후 실설치본 스모크: `cargo run -p kerbaloc-cli -- doctor` → Squad dictionary.cfg(Dobie)와 구도구가 교체한 모드들이 목록에 떠야 한다.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat: doctor — 구방식 오염 감지·zip 백업·복원 안내"
```

---

### Task 12: 팩 모델 + install/remove + validate

**Files:**
- Create: `kerbaloc/kerbaloc-core/src/pack.rs`
- Modify: `kerbaloc/kerbaloc-core/src/lib.rs`, `kerbaloc/kerbaloc-cli/src/main.rs`
- Test: `kerbaloc/kerbaloc-core/tests/pack.rs`

**Interfaces:**
- Consumes: `cfg`, `loc`, `hash`, `validate::validate_translation`
- Produces (부록 E §1.4 스키마의 최소 부분집합 — 이후 플랜에서 확장):
```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackMeta {              // pack.json
    pub schema: String,            // "kerbaloc/pack@1"
    pub lang: String, pub mod_id: String, pub variant_id: String,
    pub src_sha256: String,        // "v1:sha256:..."
    pub keys_translated: usize, pub keys_target: usize,
}
pub struct ValidationReport { pub errors: Vec<String>, pub warnings: Vec<String> }
pub fn validate_pack(dir: &Path, source_entries: Option<&BTreeMap<String,String>>) -> ValidationReport
    // 검사: pack.json 스키마, ko.cfg 파싱+라운드트립, ko 외 언어 노드 금지, BOM 금지,
    //       source_entries 있으면 키별 validate_translation + 커버리지 재계산 일치
pub fn install_pack(ksp_root: &Path, dir: &Path) -> std::io::Result<PathBuf>  // GameData/KerbaLoc/<lang>/<ModId>/로 복사, 반환=설치 경로
pub fn remove_pack(ksp_root: &Path, lang: &str, mod_id: &str) -> std::io::Result<bool>
```
팩 디렉터리 구조: `pack.json` + `Localization/ko.cfg` (+ `Patches/*.cfg`).

- [ ] **Step 1: 실패하는 테스트**

`kerbaloc/kerbaloc-core/tests/pack.rs`:
```rust
use kerbaloc_core::pack::{validate_pack, install_pack, remove_pack, PackMeta};
use std::fs;

fn make_pack(dir: &std::path::Path, cfg_body: &str) {
    fs::create_dir_all(dir.join("Localization")).unwrap();
    let meta = PackMeta {
        schema: "kerbaloc/pack@1".into(), lang: "ko".into(),
        mod_id: "TestMod".into(), variant_id: "2026-08-16-manual-test".into(),
        src_sha256: "v1:sha256:".to_string() + &"0".repeat(64),
        keys_translated: 1, keys_target: 1,
    };
    fs::write(dir.join("pack.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    fs::write(dir.join("Localization/ko.cfg"), cfg_body).unwrap();
}

const GOOD: &str = "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕\n\t}\n}\n";
const OTHER_LANG: &str = "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕\n\t}\n\tja\n\t{\n\t\t#a = x\n\t}\n}\n";

#[test]
fn valid_pack_passes() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), GOOD);
    let r = validate_pack(d.path(), None);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn other_language_node_is_error() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), OTHER_LANG);
    assert!(!validate_pack(d.path(), None).errors.is_empty());
}

#[test]
fn bom_is_error() {
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), GOOD);
    let with_bom = [b"\xef\xbb\xbf".to_vec(), GOOD.as_bytes().to_vec()].concat();
    fs::write(d.path().join("Localization/ko.cfg"), with_bom).unwrap();
    assert!(!validate_pack(d.path(), None).errors.is_empty());
}

#[test]
fn token_check_against_source() {
    use std::collections::BTreeMap;
    let d = tempfile::tempdir().unwrap();
    make_pack(d.path(), "Localization\n{\n\tko\n\t{\n\t\t#a = 안녕\n\t}\n}\n");
    let mut src = BTreeMap::new();
    src.insert("#a".to_string(), "Hi <<1>>".to_string());  // 원문엔 토큰이 있는데 번역엔 없음
    let r = validate_pack(d.path(), Some(&src));
    assert!(!r.errors.is_empty());
}

#[test]
fn install_and_remove() {
    let ksp = tempfile::tempdir().unwrap();
    fs::create_dir_all(ksp.path().join("GameData")).unwrap();
    let pack = tempfile::tempdir().unwrap();
    make_pack(pack.path(), GOOD);
    let dest = install_pack(ksp.path(), pack.path()).unwrap();
    assert!(dest.join("Localization/ko.cfg").is_file());
    assert!(dest.to_string_lossy().replace('\\', "/").contains("GameData/KerbaLoc/ko/TestMod"));
    assert!(remove_pack(ksp.path(), "ko", "TestMod").unwrap());
    assert!(!dest.exists());
    assert!(!remove_pack(ksp.path(), "ko", "TestMod").unwrap()); // 멱등
}
```
`kerbaloc-core/Cargo.toml`에 `serde`/`serde_json`은 이미 있음.

- [ ] **Step 2: 실패 확인** — 컴파일 실패.

- [ ] **Step 3: 구현**

`kerbaloc/kerbaloc-core/src/pack.rs`:
```rust
use crate::{cfg, loc, validate::{validate_translation, Severity}};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct PackMeta {
    pub schema: String,
    pub lang: String,
    pub mod_id: String,
    pub variant_id: String,
    pub src_sha256: String,
    pub keys_translated: usize,
    pub keys_target: usize,
}

pub struct ValidationReport { pub errors: Vec<String>, pub warnings: Vec<String> }

pub fn validate_pack(dir: &Path, source_entries: Option<&BTreeMap<String, String>>) -> ValidationReport {
    let mut r = ValidationReport { errors: vec![], warnings: vec![] };
    let meta: PackMeta = match std::fs::read_to_string(dir.join("pack.json"))
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(m) => m,
        Err(e) => { r.errors.push(format!("pack.json: {e}")); return r; }
    };
    if meta.schema != "kerbaloc/pack@1" {
        r.errors.push(format!("알 수 없는 스키마: {}", meta.schema));
    }
    let cfg_path = dir.join("Localization").join(format!("{}.cfg", meta.lang));
    let bytes = match std::fs::read(&cfg_path) {
        Ok(b) => b,
        Err(e) => { r.errors.push(format!("{}: {e}", cfg_path.display())); return r; }
    };
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        r.errors.push("BOM 있는 UTF-8 — 팩 파일은 BOM 없이".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    let root = match cfg::parse(&text) {
        Ok(x) => x,
        Err(e) => { r.errors.push(format!("cfg 파싱 실패: {e}")); return r; }
    };
    match cfg::roundtrip_ok(&text) {
        Ok(true) => {}
        _ => r.errors.push("라운드트립 불일치 — 구조 문자 손상 의심".into()),
    }
    // ko 외 언어 노드 금지
    fn langs_in(n: &cfg::Node, out: &mut Vec<String>) {
        if n.name == "Localization" {
            for c in &n.children { out.push(c.name.clone()); }
        }
        for c in &n.children { langs_in(c, out); }
    }
    let mut langs = vec![];
    langs_in(&root, &mut langs);
    for l in &langs {
        if l != &meta.lang {
            r.errors.push(format!("{l} 노드 포함 — 팩은 {} 노드만 가질 수 있음", meta.lang));
        }
    }
    let entries = loc::extract_localization(&root, &meta.lang);
    if entries.is_empty() {
        r.errors.push("번역 항목이 비어 있음".into());
    }
    if let Some(src) = source_entries {
        let mut translated = 0;
        for (k, dst) in &entries {
            match src.get(k) {
                None => r.warnings.push(format!("{k}: 원문에 없는 키")),
                Some(s) => {
                    if s != dst { translated += 1; }
                    for f in validate_translation(s, dst) {
                        let msg = format!("{k}: [{}] {}", f.rule, f.message);
                        match f.severity {
                            Severity::Error => r.errors.push(msg),
                            Severity::Warning => r.warnings.push(msg),
                        }
                    }
                }
            }
        }
        if translated != meta.keys_translated {
            r.warnings.push(format!("커버리지 재계산 {translated} ≠ 신고값 {}", meta.keys_translated));
        }
    }
    r
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

pub fn install_pack(ksp_root: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let meta: PackMeta = serde_json::from_str(&std::fs::read_to_string(dir.join("pack.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let dest = ksp_root.join("GameData").join("KerbaLoc").join(&meta.lang).join(&meta.mod_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    copy_dir(dir, &dest)?;
    Ok(dest)
}

pub fn remove_pack(ksp_root: &Path, lang: &str, mod_id: &str) -> std::io::Result<bool> {
    let dest = ksp_root.join("GameData").join("KerbaLoc").join(lang).join(mod_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
```

`main.rs`의 `Cmd`에 추가:
```rust
    /// 팩 디렉터리 검증 (CI에서도 사용)
    Validate { dir: PathBuf },
    /// 팩 설치 (GameData/KerbaLoc/<lang>/<ModId>/)
    Install { dir: PathBuf },
    /// 설치된 팩 제거
    Remove { mod_id: String, #[arg(long, default_value = "ko")] lang: String },
```
매치 팔:
```rust
        Cmd::Validate { dir } => {
            let r = kerbaloc_core::pack::validate_pack(&dir, None);
            for w in &r.warnings { println!("경고: {w}"); }
            for e in &r.errors { println!("오류: {e}"); }
            if r.errors.is_empty() { println!("검증 통과 (경고 {}건)", r.warnings.len()); }
            else { std::process::exit(1); }
        }
        Cmd::Install { dir } => {
            let r = kerbaloc_core::pack::validate_pack(&dir, None);
            if !r.errors.is_empty() {
                for e in &r.errors { eprintln!("오류: {e}"); }
                std::process::exit(1);
            }
            let dest = kerbaloc_core::pack::install_pack(&root, &dir).expect("설치 실패");
            println!("설치됨: {}", dest.display());
        }
        Cmd::Remove { mod_id, lang } => {
            if kerbaloc_core::pack::remove_pack(&root, &lang, &mod_id).expect("제거 실패") {
                println!("제거됨: {lang}/{mod_id}");
            } else {
                println!("설치되어 있지 않음: {lang}/{mod_id}");
            }
        }
```

- [ ] **Step 4: 통과 확인** — `cargo test -p kerbaloc-core --test pack` → 5 PASS. `cargo build` 전체 성공.

- [ ] **Step 5: Commit**

```bash
git add kerbaloc
git commit -m "feat: 팩 모델·validate·install/remove — 원본 무수정 설치 레이어"
```

---

### Task 13: 실게임 스파이크 — ko 언어 + 테스트 팩 검증 (사람 개입 필요)

**Files:**
- Create: `kerbaloc/tests-fixtures/spike-pack/pack.json`, `kerbaloc/tests-fixtures/spike-pack/Localization/ko.cfg`
- Create: `docs/superpowers/specs/appendix/2026-08-16-F-game-spike-results.md` (결과 기록)

**Interfaces:**
- Consumes: `kerbaloc enable/install/disable/remove` CLI (Task 10, 12)
- Produces: 스펙 "리스크 및 미지수 — 검증으로 해소 예정" 체크박스 4개의 답. **이 태스크의 산출물은 코드가 아니라 검증 결과 문서다.**

- [ ] **Step 1: 스파이크 팩 작성**

`kerbaloc/tests-fixtures/spike-pack/pack.json`:
```json
{
  "schema": "kerbaloc/pack@1",
  "lang": "ko",
  "mod_id": "Squad",
  "variant_id": "2026-08-16-manual-spike",
  "src_sha256": "v1:sha256:0000000000000000000000000000000000000000000000000000000000000000",
  "keys_translated": 4,
  "keys_target": 4
}
```

`kerbaloc/tests-fixtures/spike-pack/Localization/ko.cfg` — 메인 메뉴에서 즉시 보이는 키(스톡 `#autoLOC_190726` = "Start Game" 등, `research/stock-dictionary/en-us.cfg`에서 실키 확인 후 기입):
```
Localization
{
	ko
	{
		#autoLOC_190726 = 게임 시작 (스파이크)
		#autoLOC_190733 = 설정 (스파이크)
		#autoLOC_190738 = 제작진 (스파이크)
		#autoLOC_453329 = 커뮤니티 (스파이크)
	}
}
```
(키 4개는 작성 시점에 en-us.cfg를 grep해서 메인 메뉴 문자열로 확정할 것 — 위 번호는 예시이며 반드시 실제 값으로 대체. "Start Game"/"Settings"/"Credits"/"Community"를 검색.)

- [ ] **Step 2: 사전 확인 — doctor**

Run: `cargo run -p kerbaloc-cli -- doctor`
Dobie 오염이 보고되는 상태 그대로 진행한다(스파이크는 폴백 검증이 아니라 렌더링 검증이 1차 목적). 단, **폴백 검증(체크 c)을 위해서는 Steam 무결성 검사로 Squad 원복 후 재실행이 정확**함을 결과 문서에 명시.

- [ ] **Step 3: 적용**

```bash
cargo run -p kerbaloc-cli -- install kerbaloc/tests-fixtures/spike-pack
cargo run -p kerbaloc-cli -- enable
cargo run -p kerbaloc-cli -- status
```
Expected: `언어: ko`, 설치 경로 `GameData/KerbaLoc/ko/Squad` 출력.

- [ ] **Step 4: 게임 실행 확인 (사람)**

사용자에게 KSP 실행을 요청하고 다음 4개를 확인받는다:
- (a) 메인 메뉴에 "게임 시작 (스파이크)" 등 **한글 렌더링** 여부 (폰트 박스□ 없이)
- (b) 메뉴 폰트 매칭 — 메인 메뉴 글꼴이 깨지지 않는지
- (c) 스파이크 팩에 없는 문자열이 **영어로 폴백**되는지 (Dobie 잔재로 한국어가 보일 수 있음 — 그 경우 이 체크는 Squad 원복 후 재검)
- (d) `KSP.log`에 새 오류/예외 없음: `Select-String -Path "<root>\KSP.log" -Pattern "Exception|error" | Select-Object -Last 30`

- [ ] **Step 5: 결과 기록 + 원복**

결과(스크린샷 요지 포함)를 `docs/superpowers/specs/appendix/2026-08-16-F-game-spike-results.md`에 기록하고 스펙의 "검증으로 해소 예정" 체크박스를 갱신. 실패 시 플랜 B(FontLoader `AddMenuSubFont` 플러그인)로 스펙 개정 — 이 플랜을 중단하고 사용자와 논의.

```bash
cargo run -p kerbaloc-cli -- disable
cargo run -p kerbaloc-cli -- remove Squad
```

- [ ] **Step 6: Commit**

```bash
git add kerbaloc/tests-fixtures docs
git commit -m "test: 실게임 스파이크 팩 + 검증 결과 기록"
```

---

### Task 14: 마무리 — clippy/fmt 정리 + README

**Files:**
- Modify: 전체 (clippy 지적 사항), `README.md`

- [ ] **Step 1: 린트 클린**

Run: `cd kerbaloc; cargo fmt --all; cargo clippy --all-targets -- -D warnings`
지적 사항을 전부 수정하고 재실행해 경고 0 확인.

- [ ] **Step 2: 전체 테스트**

Run: `cargo test --workspace`
Expected: 전부 PASS.

- [ ] **Step 3: README 작성** (루트 `README.md` — 현재 빈 파일)

```markdown
# KerbaLoc

KSP(Kerbal Space Program) 1.12.x 한국어화 도구 모음.

- 게임에 `ko` 언어를 **추가**한다 (en-US 교체 방식 아님 — 미번역 키는 자동 영어 폴백)
- 번역팩은 `GameData/KerbaLoc/` 아래에만 설치되어 **모드 원본을 수정하지 않는다**
- 언어 전환은 `buildID64.txt`의 한 줄만 변경 (Steam 무결성 검사로 언제든 복구)

## 사용 (CLI)

    kerbaloc status              # 설치 감지·현재 언어
    kerbaloc scan                # 번역 대상 모드 스캔
    kerbaloc doctor [--backup x.zip]   # 구방식 오염 감지/백업
    kerbaloc install <팩 디렉터리>
    kerbaloc remove <ModId>
    kerbaloc enable | disable    # 언어 ko ↔ en-us
    kerbaloc validate <팩 디렉터리>

## 개발

    cd kerbaloc && cargo test --workspace

설계 문서: `docs/superpowers/specs/2026-08-16-ksp-ko-redesign-design.md` (+ appendix A–E)
```

- [ ] **Step 4: Commit**

```bash
git add -u
git add README.md
git commit -m "chore: clippy/fmt 정리 및 README"
```

---

## Self-Review 결과

- **스펙 커버리지**: 이 플랜은 스펙 중 "게임 적용 방식" 전체, "모드 식별과 버전 파악"(부록 D), 검증기 코어(부록 B §4.1 부분집합), doctor/마이그레이션 감지, 실게임 스파이크를 구현한다. 의도적으로 다음 플랜으로 미룬 것: DB 클라이언트/다운로드(Plan 2), LLM 파이프라인·mask.json·source.sig.json·용어집(Plan 3), 스튜디오 UX(Plan 4), 프록시(Plan 5), MM 패치(비태그형) 생성기(Plan 3에서 팩 생성과 함께).
- **타입 일관성**: `cfg::Node`/`parse`/`serialize`/`roundtrip_ok`(T2-3) ← loc(T4) ← scan(T9)/doctor(T11)/pack(T12); `hash::*`(T5) ← scan(T9); `validate_translation`(T6) ← `validate_pack`(T12) — 시그니처 상호 참조 확인 완료.
- **플레이스홀더**: Task 13 Step 1의 autoLOC 키 번호만 "실제 값으로 대체" 지시가 있으며, 이는 실행 시점에 `research/stock-dictionary/en-us.cfg`를 grep해 확정하는 명시적 단계로 처리했다(구현 세부가 아니라 데이터 조회).
