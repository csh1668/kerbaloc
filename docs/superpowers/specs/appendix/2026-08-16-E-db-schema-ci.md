# 부록 E — DB 스키마 · variantId · CI · 배포 경로 · 명칭 상세 설계 (Opus 서브에이전트, 2026-08-16)

> 주의: 본 보고서의 `mask.json` "allowlist" 정책은 부록 A의 마스크 가설 기각 **이전** 설계다.
> 통합 시 `policy`는 "공식 번역이 번역한 키 = 번역 대상(역방향), 영어 유지 키 = 검토 큐"로 완화 적용한다.

## 0. 확정 제안 요약

| # | 항목 | 제안 결정 | 근거 |
|---|---|---|---|
| D1 | 인덱스 구조 | **2단** — `index/ko.json`(팩당 1행 + 추천 변형 요약) + `index/ko/<ModId>.json`(변형 전체) | 1회 요청으로 대시보드, 드롭다운은 온디맨드 |
| D2 | 영어 원문 | 원문 전체 대신 **`source.sig.json`**(키별 해시+토큰 시그니처+처분) | CI 토큰 검증·키단위 낡음 판정, 원문 재배포(라이선스) 회피 |
| D3 | 공식 번역 참조 데이터 | 팩 레벨 `packs/<ModId>/mask.json` (언어 무관) | 모드 속성. CI가 모드 파일 없이 검증 |
| D4 | variantId | `YYYY-MM-DD-<method>-<nick>[-N]`, `[a-z0-9-]`, ≤64자, **불투명 ID(파싱 금지)** | 사전순=시간순, URL 안전, 충돌 `-2` |
| D5 | 릴리스 자산 | **모드당 zip 1개**(모든 ko 변형), 불변 태그 + `db-latest` 이중화 | 자산 수 = 모드 수 |
| D6 | 클라이언트 기본 경로 | **jsDelivr `@<commitSha>` 핀**, 개별 파일 직접 fetch | 불변·전역 CDN·레이트리밋 없음 |
| D7 | 최신 발견 | `releases/latest/download/manifest.json` 1회 (ETag) | raw 60req/h 예산 중 세션당 1건 |
| D8 | 폴백 | jsDelivr → Release zip → raw@sha | §4 |
| D9 | 명칭 | `kerbaloc` / `kerbaloc-db` / `GameData/KerbaLoc` (차선 `LocKAN`) | §5 |

## 1. JSON 스키마

### 1.0 공통 규약
UTF-8 BOM 없음, LF, 2-space, 키 사전순 — CI가 정규화 포맷 강제. `"schema": "kerbaloc/<종류>@<버전>"`. 시각 UTC ISO-8601, 해시 sha256 소문자 hex, 축약 `h12`. 언어 코드는 buildID64와 동일한 `ko`.
ModId 최소 규칙: CKAN identifier verbatim → 폴더명, `[A-Za-z0-9._-]{1,64}`, Windows 예약어 금지, 대소문자만 다른 공존 금지(CI 전역 검사).

### 1.1 `index/ko.json` — 크기 관리

2단 분할 / 필드 최소화(행당 ~250B) / 해시 h12 / gzip ~5:1 / **원본 1.5MB 초과 시 CI 실패 → 샤딩 전환** / `manifest.json`의 `indexSha256` 대조로 무변경 시 미다운로드. 추정: 300모드×3변형 → ~90KB(gzip ~18KB).

주요 필드: `commit`(클라이언트가 jsDelivr URL을 핀하는 SHA), `dbTag`, `cdn{primary,release,raw}`(하드코딩 대신 인덱스 제공 — 호스팅 이전 여지), `shared{blacklist,coreGlossary}`(해시 동일 시 재다운로드 생략), `packs[]{modId,name,ckan,pinned,kind(tag|mm|mixed),variantCount,hasMask,blacklisted,detail,zip,top{...}}`.
`top` = 추천 1위 요약 — CI는 ②검수됨 ③커버리지 ④최신만 적용하고 `srcH12`를 실어 **①소스 해시 일치는 클라이언트가 최종 적용**. `coverage` 분모는 제외 규칙 적용 후 대상 키 수.

샤딩(예비): 1.5MB 초과 시 `index/ko/_shards/<00..0f>.json`(ModId 해시 첫 바이트).

### 1.2 `index/ko/<ModId>.json`
변형별: `variantId`, `path`, `files[]{path,sha256h12,sizeB}`(개별 파일 fetch + 무결성), `method/model/contributor/createdAt`, `targetModVersion`, `srcH12`, `coverage/keysTranslated/keysTarget`, `reviewed/reviewers`, `validation{status,warnings,errors,runAt}`(머지 시점 CI 스냅샷), `prUrl`, `deprecatedBy`, `note`.

### 1.3 `packs/<ModId>/mod.json` (`kerbaloc/mod@1`)
`modId`, `displayName`, `ckanIdentifier`, `folderNames[]`(개명 이력 — 스캐너 매칭), `homepage`, `license`(원문 대신 시그니처만 저장하는 근거), `kind`, `tagPrefixes[]`(태그 충돌 1차 근거), `localizationPaths[]`(해시 입력 정의, 순서 고정), `officialLanguages[]`, `knownVersions[]{version,srcSha256,seenAt,keys}`, `notes`, `maintainers`.

### 1.4 변형 `pack.json` (`kerbaloc/pack@1`) + 레이아웃

```
packs/<ModId>/
├── mod.json
├── mask.json                       # 공식 번역 참조 데이터 (언어 무관)
└── ko/
    ├── glossary.json
    └── variants/<variantId>/
        ├── pack.json
        ├── source.sig.json
        ├── Localization/ko.cfg
        └── Patches/*.cfg           # kind=mm 일 때만
```

`pack.json`: `target{modVersion,versionSource(ckan|avc|folder|unknown),kspVersion,srcSha256,srcInputs[],srcKeyCount}` / `provenance{method,model,promptVersion(재현성),contributor(원본 UTF-8),createdAt,toolVersion,basedOn(파생 계보)}` / `content{files,keysTranslated,keysTarget,coverage}` / `safety{maskApplied,maskSha256h12,blacklistSha256h12,glossarySha256h12}`(참조 규칙 버전 — 규칙 변경 시 CI가 재검증 필요 표시) / `review{reviewed,reviewers,reviewedAt}` / `license` / `note`.

`target.srcSha256` 산출식: `srcInputs` 순서대로 ConfigNode 파싱 → (키,값) 키 사전순 정렬 → `key\x1fvalue\x1e` UTF-8 연결의 SHA-256 (부록 D와 동일).

**`source.sig.json`** (`kerbaloc/source-sig@1`) — 원문 재배포 없이 CI가 (a)토큰 보존 (b)키단위 낡음 (c)커버리지 분모를 계산:
`keys.<태그> = {h: 원문 sha256 앞 8자, t: 보존 필수 토큰 다중집합, len: 원문 길이, st: translate|mask|blacklist:<level>:<rule>|heuristic:identifier}`

### 1.5 용어집 (`kerbaloc/glossary@1`)
코어/모드 동일 스키마, `scope`만 다름. 병합 우선순위 모드별 > 코어.
엔트리: `{en, ko, aliases[], pos(noun|verb|proper|abbr|unit), policy(translate|keep|prefer), domain, note, examples[](few-shot 주입용)}`.
CI: `translate` 미준수·`keep` 위반은 **경고**(차단 아님), 단 자동 영어 유지 대상 키에서의 위반은 오류.

### 1.6 `blacklist.json` (`kerbaloc/blacklist@1`) — 3수준

평가 순서 = 모드 → 키 → 패턴. **`reason`/`evidence`/`addedAt`/`addedBy` 전부 필수** — CI가 빈 reason 거부 ("왜 막았는지 모르는 항목"이 이런 파일이 썩는 주 경로).

- `mods[]{modId, action:deny, reason, evidence,…}` — 팩 자체 금지, 인덱스 `blacklisted:true`
- `keys[]{modId("*"=전역), key, action:keepEnglish,…}` — 번역 존재 시 CI 오류
- `patterns[]{id, modId, match:value|key|keyValueSimilarity, regex|threshold, action, severity(error|warning),…}` — 기본 내장 예: `format-only`(포맷 전용 문자열), `unit-only`(단위 전용), `identifier-like`(식별자형, 경고), `key-echo`(값≈키 접미, 경고), `tag-reference`(값이 타 태그 참조, 오류)

**`mask.json`** (`kerbaloc/mask@1`): `derivedFrom{languages,modVersion,srcSha256,generatedAt,tool}`, `translatable[]`(공식 번역이 번역 → 안심 번역 대상), `keepEnglish[]`(공식 영어 유지 → 검토 큐/자동 유지), `unknown[]`(공식 미커버 신규 키 → 경고만; 차단하면 모드 업데이트마다 팩이 막힘). `derivedFrom.srcSha256`이 팩과 다르면 "마스크 갱신 필요" 경고.

**규칙 우선순위(확정)**: `blacklist.mods.deny` > `keepEnglish류` = `blacklist.keys` > `patterns(error)` > `translatable 허용` > `patterns(warning)` > 용어집 경고.

## 2. variantId 명명 규칙

**형식** `<YYYY-MM-DD>-<method>-<nick>[-<n>]` — 예 `2026-08-16-gemini25pro-nick`, `2026-08-16-manual-dobie`.

- 문자 `[a-z0-9-]` 소문자 강제, 8~64자. 날짜는 UTC.
- `method`: 통제 어휘 — 모델 슬러그(`gemini-2.5-pro`→`gemini25pro`) 또는 `manual`/`mixed`/`import`, ≤12자.
- `nick`: ASCII 소문자+숫자 ≤16자(비ASCII 제거, 없으면 `anon`). **표시용 닉네임은 `provenance.contributor`에 원본 보존** — ID는 경로 안전성만 책임.
- `n`: 충돌 시 CI가 부여하는 2 이상 정수. 예약어 단독 금지.
- **불투명 ID 원칙**: 사람이 읽기 좋게 만들되 도구는 절대 파싱하지 않는다(정본은 pack.json). `-2` 접미와 닉네임 하이픈의 모호성이 전부 무해해짐.
- 충돌: Worker가 빈 자리 선점 → 경쟁 시 CI가 자동 리네임 커밋. 동일 내용 재업로드는 CI 거부 + 기존 ID 안내. **머지된 variantId는 영구 불변** — 수정은 새 변형 + `deprecatedBy` (클라이언트 설치 기록·jsDelivr SHA 캐시 보호).

## 3. CI 파이프라인 (GitHub Actions)

### 3.0 레이아웃·보안 전제
`.github/workflows/{pr-validate,publish,nightly}.yml`, `schemas/`, `tools/`, `allowed-tag-overlaps.json`.
프록시는 **same-repo 브랜치 PR**(포크 아님) → `GITHUB_TOKEN` 코멘트 가능, `pull_request_target` 불필요. **검증 코드는 반드시 base ref 것을 사용**(PR 데이터는 PR ref). 봇 PR이 `tools/`·`.github/`·`schemas/`·`blacklist.json`을 건드리면 즉시 실패(데이터 전용 PR 규칙).

### 3.1 `pr-validate.yml` 14단계
permissions `{contents: read, pull-requests: write}`, PR별 concurrency.
① 체크아웃 2종(base 도구로만 검증) ② 변경 범위(packs/glossary 밖 거부, PR당 ≤5팩) ③ 스키마+정규화 포맷 ④ **cfg 라운드트립** ⑤ **토큰 보존** ⑥ 참조 규칙/블랙리스트 ⑦ **커버리지 재계산 → 다르면 자동 수정 커밋** ⑧ 팩 간 태그 충돌 ⑨ 용어집(warn) ⑩ 길이/빈 값(warn) ⑪ 크기 한도(변형 ≤2MB, zip ≤5MB — jsDelivr 20MB 대비 여유) ⑫ 중복 변형 ⑬ variantId 규칙(충돌 자동 리네임) ⑭ 리포트 게시.

### 3.2 cfg 검증 상세

**(A) ConfigNode 라운드트립**: BOM 오류 / KSP 규칙 모사 파서 — `//` 주석은 **값 안에서도 잘림** → 번역문 내 `//`(URL 포함) 오류 / 원시 `{`/`}` 오류(`｢｣` 필수) / `\uXXXX` 형식 검증 / `=` 첫 등장 분리·값 트림 / **파싱→직렬화→재파싱 AST 완전 일치**(프리즈 원인 후보 1 방어선) / 구조 `Localization{ko{…}}`만, **ko 외 언어 노드 금지** / MM 패치: `:NEEDS[<대상모드>]` 필수, `:FOR[]` 금지, 대상 필드 화이트리스트(`title`/`description`/`manufacturer`/`tags`), (§5 발견) `:FINAL` 대신 `:AFTER[<대상모드>]` 권장 검사.

**(B) 토큰 보존** — `source.sig.json`의 `t` 다중집합과 정확 일치:
`<<\d+>>`(error) / `\^[a-zA-Z]`·단독 `^`(error — 한국어는 성이 없지만 네이티브 엔진이 파싱하므로 보존) / `\n`·`\t`(error) / `｢｣` 짝+개수(error) / `\uXXXX` 형식(error) / TMP 리치텍스트 태그 짝 균형(error) / 값 내 태그 참조 `#autoLOC_…`(error) / `{n}`·`%s`(warning).
번역문이 원문과 완전 동일하면 미번역 간주 → `keysTranslated` 제외(커버리지 부풀리기 방지).

### 3.3 태그 충돌
`tag → [modId…]` 맵, 서로 다른 모드 중복 정의 시 오류, 정당 사유는 `allowed-tag-overlaps.json` 등록, `tagPrefixes` 침범은 경고. PR은 변경분만, nightly 전수.

### 3.4 PR 코멘트 피드백
스티키 단일 코멘트(`<!-- kerbaloc-ci-report -->` 마커, edit-last) / 한 줄 결론 + 14단계 표 + 오류 상세(경로:줄 + 키 + 원문/번역 + 규칙 ID + **수정 방법 한 문장**) + 경고는 `<details>` / `::error file,line::` 인라인 어노테이션 / **`validation-report.json` 아티팩트** → 스튜디오가 받아 해당 키로 점프(UX "실패 클릭→에디터"를 원격 PR에도) / 라벨 자동(`ci:pass`/`ci:fail`/`pack:<ModId>`).

### 3.5 `publish.yml` — 머지 후
`paths`에서 `index/**` 제외(봇 커밋 재트리거 무한 루프 차단), concurrency 직렬화.
① 전수 재검증(실패 시 게시 중단+이슈) ② 인덱스 재생성 ③ 인덱스 커밋 — **순환 해결**: 레포 내 `index/ko.json`의 `commit`/`cdn`은 null, 릴리스 자산 `index-ko.json`에만 최종 SHA 주입, 클라이언트는 manifest에서 SHA 획득 ④ 불변 태그 `db-<YYYY.MM.DD>-<n>` ⑤ zip 빌드(변경 팩만) — **재현 가능 zip**(경로 정렬·타임스탬프 1980 고정·압축 레벨 고정 → 내용 무변경 시 해시 불변), 5MB 초과 시 변형별 분할 자동 전환 ⑥ 롤링 릴리스 `db-latest` 갱신 + **불변 태그 릴리스에 동일 자산**(불변 태그가 정본, latest는 발견용) ⑦ jsDelivr 새 SHA는 퍼지 불필요, 엣지 워밍 1회 ⑧ 게시 후 공개 URL 3종 해시 대조, 불일치 시 이슈+이전 태그 유지.

### 3.6 `nightly.yml`
전수 재검증(규칙 변경의 소급 위반 → `stale-fail` 표시+이슈, 즉시 삭제 안 함) / 태그 충돌 전수 / URL 가용성·해시 / 오래된 `unknown` 마스크 키 리포트.

## 4. 다운로드 경로 (웹 확인 2026-08-16)

| 특성 | raw.githubusercontent | jsDelivr `/gh/` | Release 자산 |
|---|---|---|---|
| 레이트리밋 | **비인증 60 요청/시/IP** (2025-05 정책 개편으로 raw 다운로드 명시 포함) | 사실상 없음 | 리다이렉트 단계가 비인증 한도 포함, 429 사례 있음 |
| 캐시 | 오리진 직결 | 전역 엣지, **SHA 핀은 영구 불변** | CDN 백엔드 |
| 캐시 함정 | 없음 | `@main` 최대 12h, 버전 별칭 7~14일 — **SHA 핀은 함정 없음** | `db-latest` 자산 URL 내용 가변 → 캐시 오염 가능 |
| 파일 한도 | 레포 한도 | 20MB/파일 | 2GB |
| 폴더 zip | ❌ | ❌ | ✅ CI 생성 |

**핵심**: raw는 60/시/IP라 "인덱스+팩 파일 수십 개" 설치를 감당 못 하고 공유 IP(회사/학교 NAT)에선 이미 소진됐을 수 있다 → 기본 경로 불가. jsDelivr의 유일한 위험(캐시 지연)은 SHA 핀으로 개념적으로 소멸.

**확정 흐름**: ① `releases/latest/download/manifest.json` 1회(ETag, 304면 이후 전부 생략) ② jsDelivr `@<commit>/index/ko.json`(indexSha256 대조) ③ 온디맨드 파일 전부 jsDelivr @SHA ④ 설치는 개별 파일 병렬 fetch + sha256 검증 (files > 8개 또는 512KB 초과 시에만 zip 경로).

**폴백**: jsDelivr → Release zip(불변 태그) → raw@sha(최후, 429는 재시도 없이 즉시 다음/안내).
공통: 모든 URL SHA/불변 태그 핀 / 모든 응답 해시 검증 / UA 고정 / 로컬 캐시 내용 주소 저장(`%LOCALAPPDATA%\kerbaloc\cache\<sha앞2>\<sha>`, LRU 200MB) / 오프라인 모드 / 타임아웃 8s·재시도 2회.

> **스펙 변경**: "raw 기본, jsDelivr 폴백" → **순서 반전** (raw 60/시 확인).

## 5. 명칭 후보

| # | 후보 | CLI/폴더/DB | 평가 |
|---|---|---|---|
| **1 ★** | **KerbaLoc** | `kerbaloc` / `KerbaLoc` / `kerbaloc-db` | 검색성 최상, Kerbal+Localization 즉독, KSP 관례 톤 일치. 기존 모드와 충돌 없음 확인 |
| 2 | LocKAN | `lockan` / `LocKAN` / `LocKAN-meta` | CKAN 사용자가 구조 즉시 이해. CKAN 공식 혼동 리스크, "Lock" 오독 |
| 3 | KSPolyglot | — | 다국어 명시, 11자로 김 |
| 4 | KSP-Loc(가칭) | — | 자명하나 검색성 최악, KSP 상표 접두 |
| 5 | Kerbalingua | — | 명확하나 12자 |

**권고: KerbaLoc** — 도구 `kerbaloc`, 폴더 `GameData/KerbaLoc`, DB `kerbaloc-db`, 프록시 `kerbaloc-proxy`.
부수 발견: `KerbaLoc`은 알파벳순으로 Squad보다 앞 → 먼저 로드. Localization 노드는 순서 무관하지만 **MM 패치는 순서 영향** → `:FINAL` 대신 `:AFTER[<대상모드>]` 명시를 CI 검사에 추가.

## 6. 스펙 대비 변경/추가 제안 (승인 필요)
1. 다운로드 기본 경로 raw → jsDelivr(@commit SHA) 반전
2. 인덱스 2단 분할
3. `packs/<ModId>/mask.json` 정식화 (역방향 규칙으로 완화 적용)
4. `source.sig.json` 신설
5. 릴리스 자산 = 모드당 zip 1개, 불변 태그 + `db-latest` 이중화
6. 명칭 KerbaLoc 권고

## 7. 미해결/후속
MM `:AFTER[]` vs `:FINAL` 실게임 검증 / `db-latest` CDN TTL 실측 → manifest 폴링 주기 / 팩 1000개 초과 시 샤딩 재검토 / Dobie 시드 license 값 — 원작자 협의.

출처: GitHub 비인증 레이트리밋 변경(2025-05) changelog · REST rate limits docs · raw 429 사례(bazarr#3057, opentofu#2802) · jsDelivr 한도/캐시 이슈(#18268, #18502, #18532) · jsDelivr /gh/ docs · 릴리스 자산 다운로드 community#8535.
