# 부록 C — 익명 업로드 프록시 상세 설계 (Opus 서브에이전트, 2026-08-16)

## 0. 조사로 확정된 플랫폼 사실 (2026-08 기준)

**Cloudflare Workers 무료**: 요청 100,000/일(00:00 UTC 리셋), **CPU 10 ms/요청 ← 최대 제약**, 메모리 128 MB, 외부 서브리퀘스트 50/요청, 요청 본문 100 MB, Worker 3 MB(압축), 환경변수 5 KB×64.

**Workers KV 무료**: 읽기 100k/일, **쓰기 1,000/일**, 동일 키 쓰기 1회/초, 최종적 일관성 → **레이트리밋에 부적합**.

**Durable Objects 무료**: 사용 가능하나 **SQLite 백엔드 한정**(KV 백엔드는 유료). 요청 100k/일, 13,000 GB-s/일, SQLite 5 GB, 행 읽기 500만/일, **행 쓰기 100,000/일**. 강한 일관성 → 레이트리밋/중복탐지의 정답.

**GitHub**: `POST /pulls`, `/git/refs`, `/git/blobs`, `/git/trees`, `/git/commits`는 전부 `Contents: write`로 충분(PR 라벨만 `Issues: write`). 1차 레이트리밋 5,000 req/시, **2차: 콘텐츠 생성 80/분·500/시**(PR 생성은 2차 대상 명시). fine-grained PAT는 **단일 소유자 리소스만** 접근 가능하고 **"타인 개인 레포의 협업자" 시나리오를 지원하지 않음**. 만료 최대 366일.

**기타**: Workers Logs 무료 200,000 이벤트/일·보존 3일. Turnstile 무료 무제한(localhost 등록 가능).

## 1. 핵심 아키텍처 결정

**D1. 레포는 조직(Org) 소유 + 봇을 조직 멤버로 — 또는 GitHub App.**
fine-grained PAT가 "타인 개인 레포 협업자"를 지원하지 않으므로, 개인 레포 + 봇 협업자 조합은 **작동하지 않는다**:
- (a) 조직 레포 + 봇을 조직 멤버로, PAT resource owner=조직, 레포 1개만 선택 — MVP
- (b) **GitHub App** (권장 최종형): 설치 토큰이 1시간 자동 만료 → 로테이션 절차 소멸, 유출 피해 1시간 한정. Worker에서 WebCrypto RS256 JWT 서명(<1 ms)

토큰 획득 함수 하나만 교체하면 되도록 추상화.

**D2. Worker는 팩 내용을 절대 파싱하지 않는다 (10 ms CPU의 직접 귀결).**
클라이언트가 zip을 만들고 base64 인코딩까지 마쳐서 전송 → Worker는 `TransformStream`으로 스트리밍 연결해 GitHub `create blob`으로 흘려보낸다(CPU ≈ 0). 압축 해제·구조 검증·커버리지 재계산은 **CI 담당**. PR 첫 커밋은 `incoming/<id>.zip` + `incoming/<id>.json` 2개이고, CI가 `packs/…` 정규 경로로 전개해 사람이 읽는 diff를 만든다.

**D3. 상태는 전부 Durable Object(SQLite).** `RateLimiter`(IP/네트워크별), `GlobalGuard`(싱글턴: 전역 상한·킬스위치·GitHub 백오프·base ref 캐시), `Submissions`(세션·콘텐츠해시→PR 인덱스).

## 2. API 설계

Base `https://ksp-loc-upload.<계정>.workers.dev` (커스텀 도메인 불필요 = 0원). CORS는 `127.0.0.1:*`/`localhost:*` 검증 후 에코.

**`GET /v1/health`** — 스튜디오가 공유 버튼 표시 여부 결정. 60초 캐시.
```json
{ "status":"ok", "uploads_enabled":true, "message":null,
  "limits":{"max_zip_bytes":4194304,"max_b64_bytes":5600000,"max_files":200,
            "max_uncompressed_bytes":16777216,"per_ip_per_hour":3,"per_ip_per_day":10},
  "min_client_version":"0.3.0", "repo":"ksp-loc/translation-db" }
```

**`POST /v1/submissions`** — 제출 개시, 메타데이터만, 본문 ≤ 16 KB (Worker가 파싱하는 유일한 JSON).
```jsonc
{ "schema":1, "lang":"ko", "modId":"CommunityResourcePack",
  "modDisplayName":"Community Resource Pack", "ckanIdentifier":"CommunityResourcePack",
  "variantId":"2026-08-16-gemini25-nick", "contributor":"nick",
  "method":"llm:gemini-2.5-pro",          // allowlist: manual | review | llm:<model>
  "targetModVersion":"2.1.0", "sourceHash":"sha256:ab12…",
  "coverage":{"translated":812,"total":1004},   // 참고값. CI가 재계산·덮어씀
  "notes":"고유명사는 영어 유지", "glossaryChanged":true,
  "artifact":{ "sha256":"sha256:cd34…", "zipBytes":183422, "b64Bytes":244564,
    "fileCount":4, "uncompressedBytes":921003,
    "files":[{"path":"pack.json","bytes":812},{"path":"Localization/ko.cfg","bytes":903221}] },
  "pow":{"challenge":"…","nonce":"…"} }
```
→ `201 {submissionId(ULID), uploadToken(HMAC, 10분, id+sha256+크기 바인딩), uploadUrl, expiresAt}` / 중복이면 `200 {duplicate:true, prUrl, prNumber, state}`

**`PUT /v1/submissions/{id}/artifact`** — Bearer uploadToken, `Content-Type: application/base64`, `Content-Length` 필수. 선언 크기와 실제 불일치 시 스트림 abort. → `202 {blobSha}`

**`POST /v1/submissions/{id}/finalize`** — 브랜치·커밋·PR 생성 → `201 {prNumber, prUrl, branch, ciStatusUrl}`

**`GET /v1/submissions/{id}`** — DO 캐시만 반환. **`GET /v1/stats`** — 공개 지표, 300초 캐시.

**오류**: `{error:{code, message_ko, retryAfter}}` — 400 `schema_invalid`/`path_rejected`/`nickname_rejected`, 401 `token_invalid`, 409 `duplicate_pending`, 413 `too_large`, 426 `client_too_old`, 429 `rate_limited_ip`/`rate_limited_global`, 503 `uploads_disabled`/`github_unavailable`.

**팩 zip 구조** (루트 접두 폴더 없음): `pack.json`, `Localization/ko.cfg`, `Patches/*.cfg`(선택), `glossary.json`(선택). 경로 allowlist: `^(pack\.json|glossary\.json|Localization/[A-Za-z0-9._-]+\.cfg|Patches/[A-Za-z0-9._-]+\.cfg)$`

**크기 한도**: zip 4 MiB / base64 5.6 MB / 압축 해제 총량 16 MiB (**압축비 ≤ 4:1 = zip bomb 1차 차단**) / 파일 200개 / 메타 JSON 16 KB / `notes` 500자.

### Worker 검증 vs CI 검증
**Worker** (표면 검사만): 스키마·정규식(allowlist/닉네임) · 경로 allowlist · 크기 한도·선언↔실제 일치 · 압축비 상한 · 레이트리밋/중복/오픈 PR 상한 · 클라이언트 버전 하한 · PoW/Turnstile · PR 본문 인젝션 새니타이즈 · 전역 킬스위치.

**CI (진짜 방어선)**: zip 안전 해제(경로·총량·압축비 재검증) · sha256 실제 대조 · ConfigNode 라운드트립 파싱(`{}`↔`｢｣`) · 포맷 토큰 보존 · 블랙리스트/화이트리스트 위반 · **커버리지 재계산 후 pack.json 덮어쓰기(신고값 신뢰 금지)** · sourceHash 정합 · 팩 간 중복 태그 · BOM 없는 UTF-8·제어문자·널바이트 · 시크릿 스캐닝 · zip 정규 경로 전개.

## 3. GitHub 연동

### 3.1 최소 권한
**경로 (a) fine-grained PAT**: resource owner = 조직, `translation-db` 1개만. Metadata `Read` / Contents `Read and write` / Pull requests `Read and write`. 나머지 전부 No access — Actions/Workflows/Secrets/Administration 절대 금지. 만료 90일.

**경로 (b) GitHub App**: `Contents: RW`, `Pull requests: RW`, `Metadata: R`, 웹훅 불필요, 레포 1개 설치. 설치 토큰 DO에 55분 캐시.

### 3.2 API 시퀀스 (총 ~8회, 콘텐츠 생성 5회)
1. `GET /git/ref/heads/main` → base sha (DO 5분 캐시)
2. `GET /git/commits/{sha}` → base tree
3. `POST /git/blobs` ← artifact 스트리밍 패스스루
4. `POST /git/blobs` — 메타 JSON
5. `POST /git/trees` — `incoming/<id>.zip`, `incoming/<id>.json`
6. `POST /git/commits` — author 봇 고정, 메시지 `feat(ko/<ModId>): <variantId> 업로드 by <nickname>`
7. `POST /git/refs` — `refs/heads/upload/<lang>/<ModId>/<submissionId>` (ULID로 충돌 차단)
8. `POST /pulls`

**보상 트랜잭션**: 브랜치 성공 후 PR 실패 시 DO에 `pending_pr` 기록 → finalize 재호출이 멱등하게 PR만 재시도. 24h 고아 브랜치는 nightly 워크플로가 삭제.

### 3.3 PR 본문 템플릿

표 형태로 모드/identifier/언어/변형ID/기여자/방식/대상버전/소스해시/커버리지(신고값)/용어집 변경/크기/제출ID 표기 + 기여자 메모(인용) + 자동 검증 체크리스트 + 검수자 안내(익명 제출·닉네임 미검증·브랜치 삭제 방법) + 기계 판독 마커 주석(`ksp-loc-proxy: submissionId=… contentHash=…` — DO 유실 시 PR 검색으로 중복 탐지 복구).

### 3.4 PR 본문 인젝션 방지 (필수)
닉네임·메모·표시명 삽입 전: 제어문자 제거 → `@` 멘션 무력화(U+200B 삽입) → 마크다운 특수문자 이스케이프 → **`#123`/`fixes #`/`closes #` 패턴 무력화(타 이슈 자동 종료가 실제 공격 벡터)** → URL 코드 스팬 감싸기 → 길이 하드 클립.

### 3.5 레포 방어 설정
`main` 보호(직접 push 금지, PR+CI 필수, 1인 승인) / **`pull_request_target` 절대 금지** / 팩 내용을 워크플로에서 실행하지 않고 파싱만 / `GITHUB_TOKEN` 기본 `read` / 머지·닫힘 시 브랜치 자동 삭제.

## 4. 남용 방어

**식별자**: `CF-Connecting-IP` + `cf.asn`/`cf.country`. 원시 IP 미저장 — `HMAC-SHA256(ip, IP_PEPPER)` 상위 16바이트(pepper 90일 교체). 네트워크 버킷 IPv4 `/24`, IPv6 `/48`.

**레이트리밋(DO 슬라이딩 윈도우)**: IP 3/시간·10/일 · 네트워크 20/일 · ASN 60/일(클라우드 ASN 10) · **전역 30/시간·200/일** · 동일 (modId,lang,기여자) 2/일 · 닉네임당 오픈 PR 5개. 전역 30/시간은 **GitHub 2차 제한 500/시에서 역산**(제출당 콘텐츠 생성 5회 × 30 = 150/시 ≪ 500).

**DO SQLite**: `events`(자기 정리) / `submissions`(30일 TTL) / `global_state`. 제출당 ~6행 쓰기 → 무료 100k 행/일 대비 하루 1만 건 여유.

**봇 억제 (계정 없이)**: 1순위 **Proof-of-Work(hashcash)** — SHA-256 선행 0비트 ≥ 20, 검증 SHA-256 1회 = 0.01 ms, 데스크톱 체감 0.5~2초, 난이도 18~24비트 동적. 2순위 Turnstile — 남용 급증 시 켜는 스위치. 3순위 클라이언트 버전 하한 + 서명 uploadToken. 목표는 완전 차단이 아니라 "검수자 1명이 감당 가능한 유입량", 최종 게이트는 사람 검수.

**중복 제출**: 키 = `sha256(zip)`. 열린 PR → `200 duplicate` / 머지됨 → `200 state:"merged"` / 거절됨 → `409` + 수정 재제출 안내. variantId에 콘텐츠 해시 6자 접미 권장.

**악성 내용 1차 필터(Worker)**: 닉네임 `^[\p{L}\p{N}][\p{L}\p{N} _.-]{1,23}$` + NFKC + RTL override 거부 + 예약어 blocklist / 경로 `..`·절대경로·백슬래시 차단 / `method` allowlist / 압축비>4 거부 / 확장자 allowlist `.cfg`,`.json` / `notes` 내 URL ≥ 3 거부.
**"cfg 위장" 본체 방어는 CI**: ConfigNode 실제 파싱, 최상위 노드 `Localization { <lang> { … } }`만 허용. **MM 패치는 화이트리스트 연산자만**(`@PART[...]`/`@title`/`@description` 등) 허용, `!`(삭제)·`:FINAL`·`:BEFORE[...]` 등 파괴적 지시자 차단.

## 5. 운영

### 5.1 PAT 로테이션 (경로 a, 90일)
시크릿 2슬롯 PRIMARY/SECONDARY, 401/403 시 자동 전환. D-7 알림 → 새 PAT 발급 → 비활성 슬롯 주입 → 스모크 → 전환 → 24h 후 구 토큰 즉시 Revoke.
**유출 대응**: Revoke → 킬스위치 ON → Audit log 감사 → 이상 브랜치·PR 삭제(**봇은 main push 권한이 없어 피해가 브랜치·PR 생성에 국한**) → 재발급.
경로 (b)로 가면 절차 전체가 사라짐 → **장기적으로 (b) 강력 권장.**

### 5.2 열화 모드 — 원칙: 업로드만 불가, 다운로드 무영향
다운로드는 GitHub raw/Release/jsDelivr 직결이라 Worker와 완전 독립. **이 분리를 설계 불변식으로 고정.**

| 상태 | 트리거 | Worker | 스튜디오 |
|---|---|---|---|
| ok | — | 정상 | 공유 활성 |
| degraded | 한도 근접, GitHub 5xx/2차 제한 | 429/503 + Retry-After | "혼잡, N분 뒤 재시도" |
| disabled | 킬스위치, 토큰 무효, 일일 소진 | 503 | 공유 비활성 + 수동 경로 안내 |
| unreachable | Worker/CF 장애 | 무응답 | health 3초 타임아웃 → disabled 취급 |

**수동 폴백(항상 제공)**: 팩 zip을 `exports/<variantId>.zip`로 내보내고 프리필 이슈 URL을 연다. 공유 실패가 번역 파이프라인을 절대 막지 않는다 — 공유만 로컬 재시도 대기열로.

### 5.3 로깅·모니터링 (전부 무료)
Workers Logs(3일) — 제출당 구조화 로그 3줄, PII 금지(ipHash 앞 8바이트·submissionId·modId·결과 코드만). CPU p99 > 8 ms = 설계 위반 신호. `GET /v1/stats`로 모니터링 SaaS 불필요. Actions schedule이 매일 stats를 이슈 코멘트로 적재(3일 보존 보완).

### 5.4 비용 0원 유지 조건
요청 <100k/일(health 캐시, 주기 폴링 금지) / CPU <10 ms(파싱·해시·base64를 Worker에서 안 함) / DO 요청 <100k / DO 행 쓰기 <100k / KV 런타임 쓰기 금지 / R2·D1·Queues 미사용 / `*.workers.dev` / DB 레포 public 유지 / 팩 zip은 Release 자산.
**무료 플랜 한도 초과는 과금이 아니라 429 거절** → 비용 리스크가 아니라 가용성 리스크이며 §5.2가 답. 결제 수단 미등록으로 실수 과금 원천 차단.

## 6. 대안 비교 (Worker 없이 가능한가)

`repository_dispatch`는 성립하지 않는다 — `contents: write` 토큰을 데스크톱 도구에 넣는 순간 공개된 것과 같다. issue-ops는 토큰 문제가 없지만 **GitHub 계정·로그인을 요구**해 "계정/키 절대 불필요" 요구를 정면 위반. Device flow/OAuth도 동일. 결국 **"익명 사용자를 대신해 쓰기 자격증명을 보관·행사하는 서버 측 주체"가 구조적으로 불가피**하며 Worker가 그 최소 형태다. 단, **issue-ops를 열화 모드의 수동 폴백으로 병행 유지** — Worker 장애 시에도 GitHub 계정 보유자는 기여 가능. **이 병행 구성이 최종 권고안.**

## 7. 잔여 확정 항목
조직 PAT(a) vs 처음부터 GitHub App(b) — **b 권장** / PoW 기본 난이도(20비트 제안) / `min_client_version` 강제 시점 / CI zip 전개 force-push vs 추가 커밋 / variantId 명명(콘텐츠 해시 6자 접미 권장).

## 핵심 결정 5줄 요약
1. 개인 레포 + 봇 협업자 조합은 불가 — 조직 레포 또는 **GitHub App(권장)**.
2. Worker CPU 10 ms 때문에 Worker는 팩을 절대 파싱하지 않는다 — 모든 내용 검증은 CI.
3. KV는 레이트리밋에 못 쓴다 — **Durable Objects(SQLite, 무료)** 가 정답.
4. 전역 레이트리밋 30/시간은 GitHub 2차 제한 500/시 역산. 봇 억제는 PoW 기본, Turnstile 비상 스위치.
5. PR 본문 인젝션 새니타이즈와 MM 패치 연산자 화이트리스트가 가장 놓치기 쉬운 실질 보안 항목. 장애 시 다운로드 무영향 + issue-ops 수동 폴백.

출처: Cloudflare Workers/KV/DO/Logs/Turnstile 공식 docs, GitHub REST(pulls, fine-grained PAT permissions, rate limits), GitHub PAT rotation changelog.
