# KSP 로컬라이제이션 시스템 전면 개편 설계

- 날짜: 2026-08-16 (개정 1 — 사용자 피드백 5건 반영)
- 상태: 검토 중
- 대상: KSP 1.12.x (Steam, Windows). 하위 호환성 고려하지 않음. 기존 코드 전면 재작성 허용.
- 범위: **다국어 지원 구조**로 설계하되, 1차 지원 언어는 한국어(ko)만. 도구 UI 언어도 우선 한국어만.

## 배경 및 문제

기존 방식은 모드의 `en-US` 로컬라이제이션 파일을 한국어로 **교체**하는 방식이었다. 이로 인해:

- 중복 번역 (같은 모드를 여러 번 번역)
- 모드 업데이트 시 번역 유실
- "원본 백업"이 실제로는 번역본을 백업하는 문제
- 일부 모드(예: CommunityResourcePack)는 번역 시 게임 로딩이 멈추는 문제 → 기존 구현은 번역 블랙리스트로 회피

해결 방향: 게임에 **언어를 추가**하는 방식으로 전환하고, GitHub 기반 번역 DB로 번역을 공유/다운로드/적용(ckan 유사)할 수 있게 한다.

## 탐색 결과 (ILSpy 디컴파일 + 에셋 분석, 2026-08-16)

대상: `C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program` (KSP 1.12.5, build 03190)

### "ko" 언어가 사실상 게임에 내장되어 있음

1. **언어 결정**: `KSP.Localization.Localizer.GetLanguageIdFromFile()`은 게임 루트의 `buildID64.txt`에서 `language = xx` 값을 **화이트리스트 검증 없이 그대로** 사용한다. `language = ko`로 바꾸면 그대로 현재 언어가 된다.
2. **자동 영어 폴백**: `Localizer.RefreshTagValues()`는 `en-us` 태그를 먼저 전부 로드한 뒤 현재 언어(`ko`)의 태그로 덮어쓴다. 즉 ko 노드에 없는 키는 자동으로 영어가 표시된다. "추가 방식" 번역이 엔진 차원에서 지원되는 구조.
3. **번역 데이터 로드**: `Localizer.AddTagValuesForLanguage()`는 `GameDatabase.GetConfigNodes("Localization")` → `GetNodes(lang)`으로 로드한다. GameData 아무 곳의 cfg에 `Localization { ko { #tag = 값 } }` 노드를 넣으면 된다. `Localizer.Init()`은 GameDatabase 로딩(`GameDatabase.cs:308`) 중 호출되므로 cfg 로드 이후 시점이 보장된다.
4. **한국어 폰트 내장**: 스톡 에셋(`sharedassets0.assets`, `resources.assets`)에 `NotoSansCJK-K01-Regular SDF`, `NotoSansCJK-K02-Regular SDF` 폰트가 이미 존재한다. `level0` 씬의 `FontSettings.LanguageSettings`(게임 폰트/메뉴 폰트 양쪽)에 langCode **"ko"** 항목이 이미 있다 (콘솔판 한국어 지원의 잔재로 추정; zh-tw, pt-pt 등도 함께 존재). 커스텀 폰트 베이킹이나 플러그인 DLL이 불필요하다.
   - `FontLoader.LoadFonts()`가 `MenuFontSettings.ChangeLanguage(GetLanguageIdFromFile())`를 호출하므로 buildID64.txt 값이 정확히 `ko`여야 메뉴 폰트가 매칭된다 (`ko-kr` 아님).
   - `GameFontSettings.ChangeLanguage()`(인자 없음)는 모든 언어의 폴백 폰트를 로드하므로 게임 내 텍스트는 항상 한글 렌더링 가능.
5. **Lingoona 문법 엔진**: `Localizer.RefreshRuleSet()`이 언어 앞 2글자를 내부 매핑(en/es/ja/ru/zh/de/fr/it/pt)에서 찾고, 없으면 그대로 `Grammar.setLanguage("ko")`를 호출한다. 네이티브 라이브러리라 미지원 언어는 영어처럼 동작할 뿐 크래시하지 않는 구조(예외는 `SwitchToLanguage`에서 잡힘). 한국어는 성·수 문법이 없으므로 무해. 단, 번역문에는 Lingoona 문법 토큰 대신 단순 치환 토큰(`<<1>>`)을 유지해야 한다.
6. **런타임 언어 전환은 비실용적**: `Localizer.SwitchToLanguage()`는 public이지만 파트 이름 등은 로딩 시점에 `TranslateBranch()`로 번역이 구워지므로 어차피 재시작이 필요하다. 게임 내 언어 선택 UI를 플러그인으로 넣는 것보다 도구가 buildID64.txt를 전환하는 편이 단순·견고 (사용자 결정 사항).
7. **`KSP.UI.Language.Language` 시스템** (`GameData/Localisation/*.xml` + `*.unity3d`): 레거시 시스템으로 현 설치본에 해당 폴더가 없음. 사용하지 않는다.
8. **FontLoader의 모드용 API**: `AddGameSubFont`/`AddMenuSubFont(langCode, ...)`는 게임 내부 호출자가 없는 모드용 통로. `GameData/**/*.fnt` 번들도 자동 로드된다. "ko" 폰트가 내장되어 있으므로 현재 설계에서는 불필요하나, 만약 실게임 검증에서 메뉴 폰트 매칭이 실패하면 이 통로로 폴백 가능(플랜 B). 다른 언어(폰트 미내장 언어) 지원 시에는 이 통로가 필요할 수 있다.

### CRP류 로딩 프리즈 심층 분석 (사용자 피드백 2 + 후속 심화)

조사 결과 (2026-08-16):

- **DisplayName 번역 자체는 안전하다.** CRP가 공식 동봉한 zh-cn/ja 번역이 DisplayName을 번역하고 있고(`#LOC_CRP_Food_DisplayName = 食物`), 게임은 로딩 마지막에 `Localizer.TranslateBranch`(`GameDatabase.cs:3778`)로 모든 cfg 값의 태그를 언어 불문 동일 경로로 치환한다. 중국어/일본어 유저가 정상 플레이 중이므로 엔진 차원 문제가 아니다.
- 단, **공식 번역도 고유명사(Karbonite, Karborundum 등)는 영어로 유지**한다.
- 구 도구의 CRP 백업(`_TranslatorOutputs/_backups/CommunityResourcePack`)은 원본 상태(사고 후 복원됨)라 프리즈를 일으킨 실제 산출물은 소실. 정확한 소비자 코드는 미특정 — 스파이크에서 재현으로 특정한다(아래 검증 계획).
- **구방식 특유의 프리즈 원인 후보와 새 시스템의 해소 방법**:
  1. LLM 산출물에 구조 문자(`{`/`}`, 빈 값, 이스케이프 오류)가 섞여 ConfigNode 파스 트리가 파괴 → 팩 생성기와 CI가 생성물을 **ConfigNode 문법으로 라운드트립 재파싱 검증** (KSP는 값 내 `{}`를 `｢｣`로 이스케이프해야 함 — `Localizer.unescapeStringToLingoona`에서 확인)
  2. en-us 교체로 영어 원문 소실 → 원본 무수정 구조로 해소
  3. 고유명사까지 기계번역 → 아래 "공식 번역 마스크"로 해소

**공식 번역 마스크 (신규 안전 규칙)**: 모드가 공식 번역(zh-cn/ja 등)을 동봉한 경우, **공식 번역이 실제로 번역한 키만** 번역 대상으로 허용하고, 공식 번역이 영어를 유지한 키(고유명사·식별자)는 자동으로 영어 유지한다. 모드 제작자의 판단을 그대로 안전 기준으로 삼는 규칙으로, CRP는 이것만으로 블랙리스트 없이 번역 가능해진다.

**블랙리스트 (폴백)**: 공식 번역이 없는 모드에서 문제가 확인될 때 사용. DB에서 공유되는 데이터(`blacklist.json`).

- 수준: 모드 단위 + 키 단위 + 패턴 단위
- 보조 휴리스틱(기존 classifier 계승): 값이 식별자처럼 생긴 경우(공백 없음, CamelCase, 값=키 접두 유사) 자동 SKIP
- CI가 팩 검증 시 블랙리스트 위반과 마스크 위반을 거부

### 현재 설치본 오염 상태

- `GameData/Squad/Localization/dictionary.cfg`: 커뮤니티 패치(Dobie, 24.06.15)로 en-us 노드가 한국어로 덮어써진 상태.
- 다수 모드의 `en-us.cfg`: 기존 도구가 한국어로 교체한 상태. `_TranslatorOutputs/<모드명>/` 폴더가 남아 있으나 백업 신뢰성 불명(번역본이 백업됐을 가능성 — 기존 문제 그 자체).

## 사용자 결정 사항

| 결정 | 선택 |
|---|---|
| 게임 내 언어 선택 | 도구가 buildID64.txt 전환 (플러그인 없음) |
| 번역 스튜디오 형태 | 로컬 웹앱 (Python 백엔드 + 브라우저 UI) |
| 번역 DB | GitHub 공개 레포 + 인덱스 |
| 업로드 | 익명 업로드 프록시 (Cloudflare Worker + 봇 PR) — 사용자 API 키/계정 등록 절대 불필요 |
| 다운로드 | GitHub raw/zip — 키/계정 불필요 |
| 다국어 | 구조는 다국어 지원, 1차 지원·도구 UI는 한국어만 (피드백 1) |

## 전체 구조

```
┌─────────────┐   raw URL (키 불필요)   ┌──────────────────┐
│ 번역 DB      │ ◄──────────────────── │ ksp-loc 도구      │
│ (GitHub 레포)│                        │  CLI + 스튜디오    │
└─────▲───────┘                        │  (로컬 웹앱)       │
      │ 자동 PR                        └────────┬─────────┘
┌─────┴───────┐    zip POST (익명)              │ 설치/전환
│ 업로드 프록시 │ ◄──────────────────────┐      ▼
│ (CF Worker)  │                        ┌──────────────────┐
└─────────────┘                        │ KSP GameData      │
                                       │  /KSP-Loc/<lang>/ │
                                       └──────────────────┘
```

구성요소 4개: ① 번역 DB(GitHub 레포) ② 업로드 프록시(Cloudflare Worker) ③ ksp-loc 도구(CLI + 로컬 웹 스튜디오) ④ 게임 적용 레이어(GameData/KSP-Loc).

명명: 언어 중립적으로 `KSP-Loc`(폴더)/`ksp-loc`(도구)를 사용한다 (피드백 1). 최종 명칭은 구현 시 확정.

## 게임 적용 방식 — 핵심 원칙: 원본 무수정

- **언어 전환**: `ksp-loc enable [--lang ko]` → `buildID64.txt`의 `language = en-us`를 `ko`로 변경. `disable`로 복원. Steam 무결성 검사·게임 업데이트로 파일이 리셋될 수 있으므로 도구가 상태를 감지하고 재적용한다.
- **번역 설치**: 모든 번역팩은 `GameData/KSP-Loc/<lang>/<ModId>/` 아래에만 복사한다. 모드 원본 파일은 절대 수정하지 않는다. → 백업 문제, 모드 업데이트 유실, 중복 번역이 구조적으로 소멸. 제거는 폴더 삭제.
- **번역팩 내용물 두 종류**:
  1. **태그형** (`#autoLOC_...` 태그를 쓰는 스톡/모드): `Localization { ko { #tag = 한국어 } }` cfg. 미번역 키는 자동 영어 폴백.
  2. **비태그형** (part cfg에 영어 하드코딩된 모드): ModuleManager 패치 `@PART[이름]:NEEDS[대상모드] { @title = 한국어 }`. 별도 파일이라 원본 무수정, 파트 이름 기준이라 모드 업데이트에도 재적용됨. MM 패치는 언어와 무관하게 적용되므로, 언어 전환(`disable`) 시 도구가 KSP-Loc 폴더를 비활성화(GameData 밖으로 이동)한다.
- **블랙리스트 적용**: 팩 생성(스튜디오)과 설치(CLI) 양쪽에서 공유 블랙리스트를 참조하여 위험 키를 번역/설치 대상에서 제외한다.
- **스톡 게임**(Squad, SquadExpansion)도 하나의 팩으로 취급하여 전체 ko 사전을 제공한다.
- **오염 감지**: 스톡 dictionary.cfg 등의 변조를 감지(해시/휴리스틱)하면 Steam 무결성 검사를 안내한다.

## 번역 DB (GitHub 공개 레포)

```
translation-db/
├── index/
│   └── ko.json         # CI가 자동 생성: 언어별 팩 목록/버전/커버리지/다운로드 URL
├── glossary/
│   └── ko.json         # 언어별 공용 용어집 (Kerbal→커벌 등, LLM 프롬프트에 주입)
├── blacklist.json      # 언어 무관 공유 블랙리스트 (모드/키/패턴 단위)
└── packs/
    └── <ModId>/
        ├── pack.json   # 모드명, 대상 모드 버전, en-us 소스 해시, 언어별 기여자/커버리지
        └── ko/
            ├── Localization/ko.cfg
            └── Patches/*.cfg        # 비태그형 모드용 MM 패치 (해당 시)
```

- **pack.json의 en-us 소스 해시**: 모드가 업데이트되어 번역이 낡았는지(키 추가/변경) 도구가 감지하는 근거. 스튜디오에서 변경된 키만 diff로 표시.
- **CI (GitHub Actions)**: PR 검증(cfg 파싱 가능, 포맷 토큰 보존, 블랙리스트 위반, 스키마 검사, 커버리지 계산) + 머지 시 index 재생성 + **팩별 zip을 롤링 GitHub Release에 게시** (GitHub는 레포 하위 폴더만 zip으로 받는 공식 방법이 없으므로 CI가 빌드).
- **다운로드 흐름**: 도구가 index를 raw URL로 조회 → 설치된 모드와 매칭 → 팩 zip(Release 자산) 다운로드 → `GameData/KSP-Loc/<lang>/`에 설치. 계정/키 완전 불필요. raw 레이트리밋 대비 jsDelivr CDN 미러를 폴백으로 사용.
- **업로드 흐름**: 스튜디오 "공유" 버튼 → Cloudflare Worker(무료 티어)에 팩 zip POST → Worker가 봇 계정 토큰(대상 레포 한정 fine-grained PAT)으로 브랜치 생성 + 자동 PR. 사용자는 닉네임만 입력. 스팸 방어: 레이트리밋 + 크기 제한 + PR 사람 검수 + CI 검증.

## 모드 식별과 버전 파악 (피드백 3)

세 겹으로 접근한다. **낡음 판정의 근거는 항상 소스 해시**이고, 버전 번호는 표시/메타데이터 용도다.

1. **소스 해시 (1차, 진실의 원천)**: 팩 생성 시 대상 모드의 로컬라이제이션 입력(en-us cfg들, 비태그형이면 대상 필드들)의 정규화 해시를 pack.json에 기록. 설치본의 현재 해시와 다르면 "낡음". 버전 번호가 없거나 부정확한 모드에도 동작하는 유일하게 견고한 방법.
2. **KSP-AVC `.version` 파일 (2차)**: 다수 모드가 표준 JSON(`CRP.version` 등)을 동봉 — `NAME`/`VERSION`/`KSP_VERSION` 필드를 표시용으로 파싱.
3. **CKAN `registry.json` (3차)**: CKAN 설치본이면 게임 루트 `CKAN/registry.json`에 설치된 모드의 identifier·정확한 버전·설치 파일 목록이 있음. 존재 시 ModId↔CKAN identifier 매핑과 버전 표시에 활용. **참고 — CKAN의 방식**: 중앙 메타데이터 레포(CKAN-meta)에 모드별 버전 메타를 쌓고 클라이언트가 인덱스를 받아 매칭한다. 우리 index.json 방식과 동형이며, ModId는 가능하면 CKAN identifier와 일치시켜 상호운용을 남겨둔다.
- ModId 정규화 규칙(폴더명이 기본, CKAN identifier 존재 시 우선)은 구현 계획에서 확정.

## 기존 설치 마이그레이션/복원 (피드백 4)

현 설치본은 구방식 번역이 적용된 상태이므로, 실게임 검증 전에 원상 복구가 선행되어야 한다.

`ksp-loc doctor` (또는 마이그레이션 스크립트)가 수행:

1. **감지**: 전체 GameData를 스캔하여 `en-us` 노드에 한글이 포함된 cfg를 목록화 (= 구방식으로 교체된 파일).
2. **백업**: 현재 상태(번역된 파일들)를 타임스탬프 zip으로 보관 — 번역 자산 유실 방지. 이 번역문들은 이후 새 팩 생성의 시드로 재활용 가능(키·번역 추출).
3. **복원**:
   - Squad/SquadExpansion → Steam 무결성 검사 안내 (dictionary.cfg 등 원복, buildID64.txt도 리셋됨에 유의)
   - 모드 → CKAN 설치 모드는 CKAN 재설치 안내(또는 자동화), 수동 설치 모드는 재다운로드 안내. `_TranslatorOutputs` 백업은 신뢰성 검증(en-us 노드에 한글 유무) 후에만 사용.
4. **검증**: 복원 후 재스캔하여 잔여 오염 없음을 확인.

## ksp-loc 도구 (Python, 전면 재작성)

- **CLI 명령**: `scan`(설치 모드 분석) / `install`·`remove`(팩 관리) / `enable`·`disable`(언어 전환) / `doctor`(오염 감지·복원 안내) / `studio`(웹앱 실행)
- **스튜디오** (FastAPI + 브라우저 UI, localhost):
  - 대시보드: 설치된 모드 × DB 보유 팩 매칭, 커버리지, 낡음(stale) 표시
  - 에디터: 키별 [영어 원문 | 번역 | 상태(기계번역/검수됨)] 테이블, 검색/필터, 용어집 참조, 블랙리스트 키 표시
  - LLM 초벌 번역: 미번역 키 일괄 번역 (기존 translator/cost 로직 계승, 용어집 주입, `<<1>>`·`\n` 등 포맷 토큰 보존 검증). LLM 키는 로컬 사용자 본인 키(공유 기능과 무관).
  - 공유: 팩을 묶어 원클릭 업로드
- 도구 UI 언어: 한국어 (다국어 UI는 후순위)
- 기존 `_TranslatorOutputs`/백업/en-US 교체 방식 전부 폐기. 하위 호환 없음.

## 검증 계획

1. **선행: 기존 번역 백업 + 원본 복원** (위 마이그레이션 절차) — 오염된 상태로는 검증 결과가 무의미.
2. **게임 검증 스파이크** (설계의 유일한 실증 미완 가정): buildID64.txt를 `ko`로 전환하고 테스트 cfg 1개(`Localization { ko { ... } }`)를 넣어 실게임에서 (a) 한글 렌더링 (b) 메뉴 폰트 매칭 (c) 미번역 키 영어 폴백 (d) KSP.log 오류 없음을 확인. 실패 시 플랜 B: FontLoader `AddMenuSubFont` 경유 플러그인.
3. 파서/제너레이터/해시 감지/팩 빌더는 pytest 단위 테스트.
4. 프록시는 wrangler 로컬 테스트 + 테스트 레포 대상 E2E.
5. **CRP 프리즈 재현 테스트**: zh-cn 구조를 미러링한 CRP ko 팩(공식 번역 마스크 적용)을 설치하고 실게임 로딩 확인. 프리즈 재현 시 KSP.log로 소비자 코드를 특정하여 마스크/블랙리스트 규칙을 갱신.
6. 실모드 3~4개(태그형/비태그형/공식 번역 보유 모드 혼합 — CRP 포함)로 E2E: 팩 생성 → 설치 → 게임 확인 → 제거.

## 리스크 및 미지수 (unknown unknowns 추적, 피드백 5 — 살아있는 목록)

발견 즉시 이 목록을 갱신하고 사용자에게 보고한다.

### 검증으로 해소 예정 (스파이크 대상)
- [ ] `language = ko`에서 메뉴 폰트가 실제로 매칭되는가 (level0의 "ko" 항목이 유효한 폴백 폰트를 가리키는가)
- [ ] Lingoona `setLanguage("ko")`가 실게임에서 조용히 통과하는가
- [ ] 스팀 클라우드/런처(`Launcher.exe`)가 buildID64.txt를 덮어쓰는 경우가 있는가
- [ ] `#autoLOC` 폴백이 파트 로딩 시점(TranslateBranch)에도 일관되게 동작하는가
- [ ] 구방식 CRP 프리즈의 정확한 소비자 코드 (산출물 소실로 미특정 — 재현 테스트로 확인, 재현 안 되면 구방식 특유 문제로 종결)

### 설계상 인지된 제약 (해소 불가 또는 후순위)
- **플러그인 DLL에 하드코딩된 영어 문자열**은 어느 방식으로도 번역 불가 (Harmony 패치는 범위 외). 팩 커버리지 표시에서 "코드 문자열 제외"임을 명시.
- **세이브에 구워진 문자열** (기존 계약 텍스트, 함선 이름 등): 언어 전환 후에도 과거 텍스트는 영어/한국어 혼재 — 무해하나 사용자 안내 필요.
- **에디터 파트 검색**: 한국어 제목으로 바뀌면 영어 검색이 안 될 수 있음 (스톡 검색은 title 기준). 검증 항목에 추가.
- **MM ConfigCache**: 팩 설치/제거 시 ModuleManager가 변경을 감지해 재패치하는지 확인 (일반적으로 자동이나 검증 필요).
- **태그 충돌**: 두 팩이 같은 태그를 정의하면 GameDatabase 로드 순서(사실상 폴더 알파벳순)에 따라 나중 것이 승리. CI에서 팩 간 중복 태그 검출.

### 운영 리스크
- **Program Files 쓰기 권한**: 도구가 GameData에 쓰려면 관리자 권한이 필요할 수 있음 (이 설치본은 모드가 이미 설치돼 있어 쓰기 가능으로 보이나, 일반화 필요 — 권한 오류 시 안내).
- **봇 PAT 유출 리스크**: Worker 환경변수로만 보관, 대상 레포 한정 fine-grained, PR 생성 권한만.
- **Worker 남용**: 무료 티어 한도 초과 시 업로드만 일시 불가(다운로드는 무관) — 열화 모드 설계.
- **인코딩**: cfg는 UTF-8(BOM 유무) 처리 일관성 필요. 게임의 cfg 리더는 UTF-8 BOM을 허용하는 것으로 보이나(기존 패치 동작 중) 팩 생성 시 BOM 없는 UTF-8로 통일.
- **용어집 표류**: 팩마다 용어가 달라지는 문제 — CI가 용어집 위반을 경고(차단은 안 함).

## 미확정 항목 (구현 계획에서 확정)
- ModId 정규화 규칙 상세
- 업로드 프록시의 레이트리밋 세부 정책
- LLM 공급자/모델 선택
- 도구/DB 레포 최종 명칭
