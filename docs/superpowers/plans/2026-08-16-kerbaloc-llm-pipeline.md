# KerbaLoc LLM 번역 파이프라인 구현 계획 (Plan 3/5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kerbaloc translate <ModId>` 한 명령으로 — 스캔된 모드의 en-us를 Gemini로 병렬 번역하고, 검증 루프(재시도 2회+에스컬레이션)를 돌리고, 중단-재개 가능한 잡 상태를 남기고, 검증 통과한 ko 팩을 생성한다.

**Architecture:** 부록 B 설계의 Rust 이식. `kerbaloc-core`에 `llm/`(Provider trait + Gemini REST), `prompt`, `batching`, `jobstore`, `glossary`, `packgen` 모듈 추가. 네트워크는 tokio+reqwest, 구조화 출력은 Gemini `responseSchema`. 실패 키는 조용히 사라지지 않고 반드시 ok/review/failed 세 버킷 중 하나에 속한다(불변식).

**Tech Stack:** tokio, reqwest(rustls), dotenvy, 기존 core 모듈(cfg/loc/hash/validate/scan/pack)

**Spec:** `docs/superpowers/specs/2026-08-16-ksp-ko-redesign-design.md` + 부록 B(파이프라인 상세)·G(2자 규칙)

## Global Constraints

- 기본 모델 `gemini-3.1-flash-lite`, 에스컬레이션 `gemini-3-flash` (env `KERBALOC_MODEL`/`KERBALOC_MODEL_ESCALATION`로 재정의)
- API 키는 env `GEMINI_API_KEY` (.env 로드는 dotenvy)
- 병렬 8, 429 지수 백오프(0.5s→2s→8s, 최대 3회). AIMD는 v1 범위 외(주석으로 남김)
- 배치 상한: 페이로드 예상 입력 ≤2,000tok(4자=1tok 근사) **그리고** ≤40키. 값 400자 초과는 별도 소배치(≤3키)
- 응답 매칭은 인덱스 `i`로만. 모든 입력 키 = ok ∪ review ∪ failed (단위 테스트로 고정)
- 모든 산출 파일은 BOM 없는 UTF-8. 잡 상태는 `<KSP루트>/KerbaLoc-jobs/<ModId>/` (GameData 밖)
- 재시도: 시도1 배치≤40 기본모델 / 시도2 실패키만 ≤8 기본모델+위반 피드백 / 시도3 단건 에스컬레이션 모델 / 그래도 실패 → review
- Plan 1의 검증기(validate_translation + displayname-too-short)를 그대로 재사용
- 용어집 v1: 코어 시드 파일만(자동 초안 생성은 Plan 4 스튜디오에서). `Water → 식수`(2자 규칙), `Kerbal → 커벌` 등 포함
- 커밋 규약·clippy -D warnings·rustfmt은 Plan 1과 동일

## File Structure

```
kerbaloc/kerbaloc-core/src/
├── llm/
│   ├── mod.rs        # Provider trait, TranslateItem/Usage/BatchOutcome, 가격표
│   └── gemini.rs     # Gemini REST 구현 (responseSchema 구조화 출력)
├── prompt.rs         # 3층 프롬프트: 시스템(고정)+모드 컨텍스트(용어집 주입)+페이로드 JSON
├── batching.rs       # 접두 정렬→이중 상한 절단→긴 값 분리
├── glossary.rs       # kerbaloc/glossary@1 로드 + 단어 경계 매칭
├── jobstore.rs       # manifest.json + results.jsonl append/재개
├── pipeline.rs       # translate_job: 배치 실행→검증→재시도→버킷 분류
└── packgen.rs        # 번역 결과 → 팩 디렉터리 (ko.cfg + pack.json, 실제 소스 해시)
kerbaloc/kerbaloc-cli/src/main.rs   # translate 서브커맨드
glossary/core.ko.json               # 코어 용어집 시드 (레포 루트)
```

---

### Task 1: LLM 타입 + Provider trait + 비용 집계

**Files:** Create `llm/mod.rs`, Modify `lib.rs`, `kerbaloc-core/Cargo.toml` (tokio/reqwest/dotenvy/async-trait), Test `tests/llm_types.rs`

**Interfaces (Produces):**
```rust
pub struct TranslateItem { pub i: usize, pub key: String, pub en: String,
    pub prev: Option<String>, pub fix: Option<String> }   // prev/fix = 재시도 피드백
pub struct TranslatedItem { pub i: usize, pub ko: String }
pub struct Usage { pub input_tokens: u64, pub output_tokens: u64 }
impl Usage { pub fn add(&mut self, o: &Usage); pub fn cost_usd(&self, price_in: f64, price_out: f64) -> f64 }
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn prices(&self) -> (f64, f64);   // per 1M tokens (in, out)
    async fn translate(&self, system: &str, context: &str, payload: &str)
        -> anyhow::Result<(Vec<TranslatedItem>, Usage)>;
}
```
Steps: 실패 테스트(Usage 합산·비용 계산) → 구현 → 통과 → 커밋 `feat(core): LLM Provider trait와 비용 집계`.

### Task 2: 코어 용어집 + 매칭

**Files:** Create `glossary.rs`, `glossary/core.ko.json`(레포 루트), Test `tests/glossary.rs`

**Interfaces:**
```rust
pub struct GlossaryEntry { pub en: String, pub ko: Option<String>, pub aliases: Vec<String>,
    pub policy: Policy }   // Policy::Translate | Keep
pub struct Glossary(Vec<GlossaryEntry>);
impl Glossary {
    pub fn load(path: &Path) -> anyhow::Result<Glossary>;
    pub fn matches(&self, texts: &[&str]) -> Vec<&GlossaryEntry>;  // 대소문자 무시+단어 경계, 상한 60
    pub fn prompt_block(entries: &[&GlossaryEntry]) -> String;     // "delta-v => 델타-V" / "Karbonite => (영어 유지)"
}
```
시드 내용(발췌): Kerbal→커벌, Kerbin/Mun/Minmus→커빈/문/민무스, delta-v→델타-V, apoapsis/periapsis→원점/근점, thrust→추력, **Water→식수(주석: 1자 '물'은 DisplayName 프리즈)**, Ore→광물, LiquidFuel→액체 연료, Oxidizer→산화제, m/s·kN·EC/s→Keep.
Steps: 실패 테스트(매칭 단어 경계·alias·상한, prompt_block 형식) → 구현+시드 작성 → 통과 → 커밋 `feat(core): 코어 용어집 로드·매칭·프롬프트 블록`.

### Task 3: 배치 계획

**Files:** Create `batching.rs`, Test `tests/batching.rs`

**Interfaces:**
```rust
pub struct Batch { pub id: String, pub items: Vec<TranslateItem> }  // id = "b0001"
pub fn plan_batches(entries: &BTreeMap<String, String>) -> Vec<Batch>
// 키 정렬(접두 응집)→순차 절단(입력 ≤2000tok 근사(4자=1tok) AND ≤40키), 값>400자는 ≤3키 소배치
```
Steps: 실패 테스트(전키 커버·중복 없음 property, 상한 준수, 긴 값 분리) → 구현 → 통과 → 커밋 `feat(core): 배치 계획 (이중 상한·긴 값 분리)`.

### Task 4: 프롬프트 빌더

**Files:** Create `prompt.rs`, Test `tests/prompt.rs`

**Interfaces:**
```rust
pub fn system_prompt() -> &'static str        // 부록 B §3.2 한국어 프롬프트 + few-shot 8 (바이트 안정)
pub fn mod_context(mod_name: &str, glossary_block: &str) -> String
pub fn payload_json(items: &[TranslateItem]) -> String   // [{"i":1,"k":"#..","en":"..","prev"?,"fix"?}]
```
Steps: 실패 테스트(페이로드 JSON 라운드트립, 시스템 프롬프트에 보존 규칙 6종 문자열 존재, 타임스탬프 미포함) → 구현(프롬프트 전문은 부록 B §3.2를 그대로 상수화, few-shot 8개 포함) → 통과 → 커밋 `feat(core): 3층 프롬프트 빌더`.

### Task 5: Gemini Provider

**Files:** Create `llm/gemini.rs`, Test `tests/gemini.rs`(mock 서버 없이 응답 파서 단위 테스트 + `#[ignore]` 실API 스모크)

**Interfaces:**
```rust
pub struct GeminiProvider { pub model: String, /* api_key, client */ }
impl GeminiProvider { pub fn new(model: &str, api_key: &str) -> Self }
// Provider 구현: POST generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
// generationConfig: {temperature:0.2, responseMimeType:"application/json",
//   responseSchema:{type:ARRAY, items:{type:OBJECT, properties:{i:{type:INTEGER}, ko:{type:STRING}}, required:[i,ko]}}}
// usageMetadata에서 promptTokenCount/candidatesTokenCount 추출
// 429/503: 0.5s→2s→8s 백오프 3회 후 에러
pub fn parse_response(json: &str) -> anyhow::Result<(Vec<TranslatedItem>, Usage)>  // 파서 분리(테스트용)
```
Steps: 실패 테스트(parse_response: 정상/candidates 누락/비JSON 텍스트) → 구현 → 통과 → `#[ignore]` 실API 테스트 1회 수동 실행으로 스모크 → 커밋 `feat(core): Gemini Provider (구조화 출력·백오프)`.

### Task 6: 잡 스토어 (중단-재개)

**Files:** Create `jobstore.rs`, Test `tests/jobstore.rs`

**Interfaces:**
```rust
pub struct JobStore { /* dir */ }
pub struct Manifest { pub mod_id: String, pub src_hash: String, pub model: String,
    pub prices: (f64, f64), pub batches: Vec<(String, Vec<String>)> }  // (batch_id, keys)
impl JobStore {
    pub fn create(dir: &Path, m: &Manifest) -> anyhow::Result<JobStore>;
    pub fn open(dir: &Path) -> anyhow::Result<(JobStore, Manifest)>;
    pub fn record(&self, batch_id: &str, items: &[(String, String)], usage: &Usage) -> anyhow::Result<()>; // JSONL append
    pub fn completed(&self) -> anyhow::Result<(BTreeMap<String, String>, Usage, Vec<String>)>; // (key→ko, 누적, 완료 batch_id)
}
```
Steps: 실패 테스트(생성→기록→재오픈 시 완료 배치 제외·비용 재계산, 깨진 마지막 줄 무시, src_hash 불일치 시 open 에러) → 구현 → 통과 → 커밋 `feat(core): 잡 스토어 — manifest+JSONL 재개`.

### Task 7: 파이프라인 (번역→검증→재시도→버킷)

**Files:** Create `pipeline.rs`, Test `tests/pipeline.rs`(FakeProvider로 결정적 테스트)

**Interfaces:**
```rust
pub struct TranslationReport { pub ok: BTreeMap<String, String>,
    pub review: Vec<ReviewItem>, pub failed: BTreeMap<String, String>, pub usage: Usage }
pub struct ReviewItem { pub key: String, pub en: String, pub candidates: Vec<String>, pub violations: Vec<String> }
pub async fn translate_job(
    provider: &dyn Provider, escalation: &dyn Provider,
    entries: &BTreeMap<String, String>,          // en-us 원문
    glossary: &Glossary, mod_name: &str,
    store: &JobStore, concurrency: usize,
) -> anyhow::Result<TranslationReport>
// 시도1: plan_batches 병렬 실행(semaphore) → validate_translation(+2자 DisplayName 규칙)
// 시도2: 실패키 ≤8 재묶음 + prev/fix 주입 → 시도3: 단건 에스컬레이션 → review
// 서킷 브레이커: 시도1 ERROR 비율 >30% && 키 ≥100 → 에러 반환(잡 중단)
// 불변식: entries 모든 키 ∈ ok ∪ review ∪ failed
```
Steps: 실패 테스트(FakeProvider: ①정상 ②특정 키 토큰 누락→재시도 경로→에스컬레이션 성공 ③항상 실패→review ④배치 응답 개수 부족→해당 키 failed 아님 재시도, 불변식 property) → 구현 → 통과 → 커밋 `feat(core): 번역 파이프라인 — 검증 루프·재시도·버킷 불변식`.

### Task 8: 팩 생성기

**Files:** Create `packgen.rs`, Test `tests/packgen.rs`

**Interfaces:**
```rust
pub fn build_pack(out_dir: &Path, unit: &scan::ModUnit, translations: &BTreeMap<String, String>,
    variant_id: &str, model_name: &str) -> anyhow::Result<()>
// Localization/ko.cfg (cfg::serialize 재사용, 키 정렬), pack.json:
//   schema/lang/mod_id/variant_id/src_sha256(unit.source_hash)/keys_translated/keys_target
// 생성 직후 pack::validate_pack(dir, Some(&unit.entries)) 자체 실행 → 오류 있으면 Err
pub fn make_variant_id(method_slug: &str, nick: &str) -> String  // "YYYY-MM-DD-<slug>-<nick>" (UTC)
```
Steps: 실패 테스트(생성 팩이 validate_pack 통과, ko.cfg 라운드트립, variant_id 형식) → 구현 → 통과 → 커밋 `feat(core): 팩 생성기 — 생성 즉시 자체 검증`.

### Task 9: CLI `translate` + 실 E2E

**Files:** Modify `kerbaloc-cli/src/main.rs`, `kerbaloc-cli/Cargo.toml`(tokio)

CLI: `kerbaloc translate <ModId> [--nick <이름>] [--install] [--resume]`
1. scan → ModUnit 찾기(없으면 에러+후보 표시) 2. .env 로드, Provider 2개 생성 3. JobStore create/open(--resume) 4. translate_job 실행(진행 출력: 배치 완료마다 `[12/40] 누적 $0.03`) 5. 리포트 출력(ok/review/failed 수, 비용) 6. packgen → `KerbaLoc-packs/<ModId>/<variantId>/` 7. review 항목은 `review.txt`로 나열 8. `--install` 시 install_pack.

Steps: 빌드 → **실 E2E: `kerbaloc translate CommunityResourcePack --nick spike`** (240키, 예상 $0.05 미만) → 결과 팩 validate 통과·비용·review 수 확인 → 커밋 `feat: translate CLI + CRP 실번역 E2E`.

### Task 10: 마무리

clippy/fmt 클린, `cargo test --workspace` 전체 통과, README 로드맵 갱신(Plan 3 완료), 커밋 `chore: Plan 3 마무리`.

## Self-Review

- 부록 B 대비 의도적 축소(v1): 프롬프트 캐싱(공급자 자동), 배치 API 50%, AIMD 적응 동시성, 용어집 자동 초안(→Plan 4), 복수 공급자 구현(트레이트만). 모두 주석/스펙에 명시.
- 타입 일관성: TranslateItem(1)→batching(3)/prompt(4)/pipeline(7), Provider(1)→gemini(5)/pipeline(7), JobStore(6)→pipeline(7)→CLI(9), ModUnit(Plan1 scan)→packgen(8) 확인.
- 플레이스홀더 없음 확인.
