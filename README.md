# KerbaLoc

KSP(Kerbal Space Program) 1.12.x 한국어화 도구 모음.

- 게임에 `ko` 언어를 **추가**한다 (en-US 교체 방식 아님 — 미번역 키는 자동 영어 폴백)
- 번역팩은 `GameData/KerbaLoc/` 아래에만 설치되어 **모드 원본을 수정하지 않는다**
- 언어 전환은 `buildID64.txt`의 한 줄만 변경 (Steam 무결성 검사로 언제든 복구)

실게임 검증 완료 (2026-08-16): `language = ko` + `Localization { ko { … } }` cfg만으로
한글 렌더링·내장 한국어 폰트 매칭·무프리즈 동작 확인. 기존 Localization 키 번역은 플러그인 DLL 불필요.

## 사용 (CLI)

    kerbaloc status              # 설치 감지·현재 언어
    kerbaloc scan                # 번역 대상 모드·부품 필드 수 스캔 (CKAN/AVC 기반 식별 + 소스 해시)
    kerbaloc doctor [--backup x.zip]   # 구방식 오염(en-us 내 한글) 감지/백업
    kerbaloc install <팩 디렉터리>      # GameData/KerbaLoc/<lang>/<ModId>/ 에 설치
    kerbaloc remove <ModId>
    kerbaloc enable | disable    # 언어 ko ↔ en-us
    kerbaloc validate <팩 디렉터리>     # 팩 검증 (CI에서도 동일 사용)
    kerbaloc translate <ModId> [--profile 이름] [--nick 이름] [--install] [--fresh]
                                 # LLM 번역 → 검증 루프 → 팩 생성 (같은 소스·모델 잡은 자동 재개)
    kerbaloc profile list | use <프로필> | models [프로필] | remove <프로필>
    kerbaloc profile set <프로필> provider=gemini api_key=… model=… reasoning=…
                                 # LLM 프로필 — 스튜디오 앱과 같은 settings.json 공유
    kerbaloc db list             # 원격 번역 DB 팩 목록 (계정 불필요)
    kerbaloc db install <ModId>  # DB에서 다운로드·해시 검증·원문 대조·설치
    kerbaloc db validate|index <레포경로>   # kerbaloc-db CI가 사용하는 서브커맨드

## 일반 부품 이름·설명 번역

스캔은 원본 cfg의 최상위 `PART` 노드에 직접 적힌 `title`·`description`도 자동으로 번역 대상에 포함한다.
이미 `#LOC_…` 키를 사용하는 필드는 기존 Localization 경로로 처리한다. CLI와 앱의 전체 키 수에는
추출한 부품 필드가 포함되며, 별도로 표시하는 부품 필드 수는 이름과 설명을 각각 센 값이다.

추출된 필드는 기존 `translate` 명령과 앱의 번역·수동 키 편집에서 함께 다룬다.
부품 필드 번역이 있는 팩은 `Localization/ko.cfg`와 함께, 번역한 필드의 원문 영어 폴백용
`Localization/en-us.cfg`와 부품 필드를 연결하는 `Patches/parts.cfg`를 생성한다.
일부 필드만 번역해도 해당 필드의 패치와 폴백만 생성하며, 기존 설치·제거 기능을 그대로 사용한다.
모드 원본 파일과 내부 식별자인 `PART.name`은 변경하지 않는다.
**부품 텍스트 번역팩을 설치하려면 ModuleManager가 필요하며, 팩 설치·제거 후 게임을 재시작해야 한다.**

대상은 원본 cfg에 직접 적힌 두 필드다. DLL 안의 문자열이나 ModuleManager가 계산·패치·복사해
만드는 부품은 이 스캔 범위에 포함하지 않는다. 중복되거나 패치 대상으로 안전하게 지정할 수 없는
부품 ID는 건너뛰며, 한 필드만 안전한 경우에는 그 필드만 추출한다.
다른 ModuleManager 패치가 나중에 같은 이름·설명을 수정하면 번역을 덮어쓰거나 태그 참조를 바꿀 수 있다.
모드의 변경 사항을 강제로 지우지 않도록 `:FINAL`을 자동 추가하지 않는다.
이 부품 텍스트 적용 경로의 실제 KSP 실행 검증은 아직 필요하다.

## 개발

    cargo test --workspace

- 설계 스펙: `docs/superpowers/specs/2026-08-16-ksp-ko-redesign-design.md` (+ appendix A–F)
- 구현 계획: `docs/superpowers/plans/`
- 골든 데이터: `research/stock-dictionary/` (스톡 en-us/ja/zh-cn + Breaking Ground)
- Rust workspace가 레포 루트 (`kerbaloc-core` / `kerbaloc-cli` / `kerbaloc-app`)

## 구성요소 (로드맵)

| Plan | 내용 | 상태 |
|---|---|---|
| 1 | kerbaloc-core + CLI (파서/해시/검증/스캔/설치/언어 전환) | ✅ 완료 |
| 2 | 번역 DB 레포([kerbaloc-db](https://github.com/csh1668/kerbaloc-db)) + CI + 다운로드 클라이언트 | ✅ 완료 (manifest→jsDelivr@SHA→검증 설치 E2E) |
| 3 | LLM 번역 파이프라인 + 팩 생성 | ✅ 완료 (CRP 240키 E2E: $0.0093, 검수 0) |
| 4 | Tauri 스튜디오 (`kerbaloc-app` — 대시보드/번역/검수/공유 GUI) | ✅ v1 (빌드·타입체크 통과) |
| 5 | 익명 업로드 프록시 ([kerbaloc-proxy](https://github.com/csh1668/kerbaloc-proxy)) | ✅ 배포·실PR E2E (`db share` → PR#1 → CI → 머지 → DB 반영) |
