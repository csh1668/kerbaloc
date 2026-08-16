# KSP 한국어화 시스템 전면 개편 설계

- 날짜: 2026-08-16
- 상태: 초안 (사용자 검토 대기 — 수정 사항 반영 예정)
- 대상: KSP 1.12.x (Steam, Windows). 하위 호환성 고려하지 않음. 기존 코드 전면 재작성 허용.

## 배경 및 문제

기존 방식은 모드의 `en-US` 로컬라이제이션 파일을 한국어로 **교체**하는 방식이었다. 이로 인해:

- 중복 번역 (같은 모드를 여러 번 번역)
- 모드 업데이트 시 번역 유실
- "원본 백업"이 실제로는 번역본을 백업하는 문제

해결 방향: 게임에 **ko 언어를 추가**하는 방식으로 전환하고, GitHub 기반 번역 DB로 번역을 공유/다운로드/적용(ckan 유사)할 수 있게 한다.

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
8. **FontLoader의 모드용 API**: `AddGameSubFont`/`AddMenuSubFont(langCode, ...)`는 게임 내부 호출자가 없는 모드용 통로. `GameData/**/*.fnt` 번들도 자동 로드된다. "ko" 폰트가 내장되어 있으므로 현재 설계에서는 불필요하나, 만약 실게임 검증에서 메뉴 폰트 매칭이 실패하면 이 통로로 폴백 가능(플랜 B).

### 현재 설치본 오염 상태

`GameData/Squad/Localization/dictionary.cfg`가 커뮤니티 패치(Dobie, 24.06.15, KSP 1.12.5용)로 **en-us 노드가 한국어로 덮어써진 상태**다. 즉 이 설치본의 "원본"은 이미 원본이 아니다. 새 도구는 스톡 파일 오염을 감지하고 Steam 무결성 검사로 복원하도록 안내해야 한다.

## 사용자 결정 사항

| 결정 | 선택 |
|---|---|
| 게임 내 언어 선택 | 도구가 buildID64.txt 전환 (플러그인 없음) |
| 번역 스튜디오 형태 | 로컬 웹앱 (Python 백엔드 + 브라우저 UI) |
| 번역 DB | GitHub 공개 레포 + 인덱스 |
| 업로드 | 익명 업로드 프록시 (Cloudflare Worker + 봇 PR) — 사용자 API 키/계정 등록 절대 불필요 |
| 다운로드 | GitHub raw URL/zip — 키/계정 불필요 |

## 전체 구조

```
┌─────────────┐   raw URL (키 불필요)   ┌──────────────────┐
│ 번역 DB      │ ◄──────────────────── │ ksp-ko 도구       │
│ (GitHub 레포)│                        │  CLI + 스튜디오    │
└─────▲───────┘                        │  (로컬 웹앱)       │
      │ 자동 PR                        └────────┬─────────┘
┌─────┴───────┐    zip POST (익명)              │ 설치/전환
│ 업로드 프록시 │ ◄──────────────────────┐      ▼
│ (CF Worker)  │                        ┌──────────────────┐
└─────────────┘                        │ KSP GameData      │
                                       │  /KSP-KO/<팩들>   │
                                       └──────────────────┘
```

구성요소 4개: ① 번역 DB(GitHub 레포) ② 업로드 프록시(Cloudflare Worker) ③ ksp-ko 도구(CLI + 로컬 웹 스튜디오) ④ 게임 적용 레이어(GameData/KSP-KO).

## 게임 적용 방식 — 핵심 원칙: 원본 무수정

- **언어 전환**: `ksp-ko enable` → `buildID64.txt`의 `language = en-us`를 `ko`로 변경. `ksp-ko disable`로 복원. Steam 무결성 검사·게임 업데이트로 파일이 리셋될 수 있으므로 도구가 상태를 감지하고 재적용한다.
- **번역 설치**: 모든 번역팩은 `GameData/KSP-KO/<ModId>/` 아래에만 복사한다. 모드 원본 파일은 절대 수정하지 않는다. → 백업 문제, 모드 업데이트 유실, 중복 번역이 구조적으로 소멸. 제거는 폴더 삭제.
- **번역팩 내용물 두 종류**:
  1. **태그형** (`#autoLOC_...` 태그를 쓰는 스톡/모드): `Localization { ko { #tag = 한국어 } }` cfg. 미번역 키는 자동 영어 폴백.
  2. **비태그형** (part cfg에 영어 하드코딩된 모드): ModuleManager 패치 `@PART[이름]:NEEDS[대상모드] { @title = 한국어 }`. 별도 파일이라 원본 무수정, 파트 이름 기준이라 모드 업데이트에도 재적용됨. MM 패치는 언어와 무관하게 적용되므로, 언어 전환(`disable`) 시 도구가 KSP-KO 폴더를 비활성화(이동)한다.
- **스톡 게임**(Squad, SquadExpansion)도 하나의 팩으로 취급하여 전체 ko 사전을 제공한다.
- **오염 감지**: 스톡 dictionary.cfg 등의 변조를 감지(해시/휴리스틱)하면 Steam 무결성 검사를 안내한다.

## 번역 DB (GitHub 공개 레포)

```
translation-db/
├── index.json          # CI가 자동 생성: 팩 목록/버전/커버리지/다운로드 URL
├── glossary.json       # 공용 용어집 (Kerbal→커벌 등, LLM 프롬프트에 주입)
└── packs/
    └── <ModId>/
        ├── pack.json   # 모드명, 대상 모드 버전, 기여자, en-us 소스 해시
        ├── Localization/ko.cfg
        └── Patches/*.cfg        # 비태그형 모드용 MM 패치 (해당 시)
```

- **pack.json의 en-us 소스 해시**: 모드가 업데이트되어 번역이 낡았는지(키 추가/변경) 도구가 감지하는 근거. 스튜디오에서 변경된 키만 diff로 표시.
- **CI (GitHub Actions)**: PR 검증(cfg 파싱 가능, 포맷 토큰 보존, 스키마 검사, 커버리지 계산) + 머지 시 index.json 재생성.
- **다운로드 흐름**: 도구가 index.json을 raw URL로 조회 → 설치된 모드와 매칭 → 선택한 팩 zip 다운로드 → `GameData/KSP-KO/`에 설치. 계정/키 완전 불필요.
- **업로드 흐름**: 스튜디오 "공유" 버튼 → Cloudflare Worker(무료 티어)에 팩 zip POST → Worker가 봇 계정 토큰으로 브랜치 생성 + 자동 PR. 사용자는 닉네임만 입력. 스팸 방어: 레이트리밋 + 크기 제한 + PR 사람 검수 + CI 검증.

## ksp-ko 도구 (Python, 전면 재작성)

- **CLI 명령**: `scan`(설치 모드 분석) / `install`·`remove`(팩 관리) / `enable`·`disable`(언어 전환) / `studio`(웹앱 실행)
- **스튜디오** (FastAPI + 브라우저 UI, localhost):
  - 대시보드: 설치된 모드 × DB 보유 팩 매칭, 커버리지, 낡음(stale) 표시
  - 에디터: 키별 [영어 원문 | 한국어 | 상태(기계번역/검수됨)] 테이블, 검색/필터, 용어집 참조
  - LLM 초벌 번역: 미번역 키 일괄 번역 (기존 translator/cost 로직 계승, 용어집 주입, `<<1>>`·`\n` 등 포맷 토큰 보존 검증)
  - 공유: 팩을 묶어 원클릭 업로드
- 기존 `_TranslatorOutputs`/백업/en-US 교체 방식 전부 폐기. 하위 호환 없음.

## 검증 계획

1. **게임 검증 스파이크** (최우선, 설계의 유일한 실증 미완 가정): buildID64.txt를 `ko`로 전환하고 테스트 cfg 1개(`Localization { ko { ... } }`)를 넣어 실게임에서 (a) 한글 렌더링 (b) 메뉴 폰트 매칭 (c) 미번역 키 영어 폴백 (d) KSP.log 오류 없음을 확인한다. 실패 시 플랜 B: FontLoader `AddMenuSubFont` 경유 플러그인.
2. 파서/제너레이터/해시 감지/팩 빌더는 pytest 단위 테스트.
3. 프록시는 wrangler 로컬 테스트 + 테스트 레포 대상 E2E.
4. 실모드 3~4개(태그형/비태그형 혼합)로 E2E: 팩 생성 → 설치 → 게임 확인 → 제거.

## 미해결/보류 항목

- ModId 정규화 규칙 (폴더명 vs CKAN identifier 매칭) — 구현 계획에서 확정
- 업로드 프록시의 레이트리밋 세부 정책
- LLM 공급자/모델 선택 및 스튜디오에서의 키 관리(로컬 사용자 본인 키; 공유와는 무관)
- 사용자 추가 수정 사항 반영 예정 (검토 대기 중)
