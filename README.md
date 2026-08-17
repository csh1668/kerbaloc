# KerbaLoc

KSP(Kerbal Space Program) 1.12.x 한국어화 도구 모음.

- 게임에 `ko` 언어를 **추가**한다 (en-US 교체 방식 아님 — 미번역 키는 자동 영어 폴백)
- 번역팩은 `GameData/KerbaLoc/` 아래에만 설치되어 **모드 원본을 수정하지 않는다**
- 언어 전환은 `buildID64.txt`의 한 줄만 변경 (Steam 무결성 검사로 언제든 복구)

실게임 검증 완료 (2026-08-16): `language = ko` + `Localization { ko { … } }` cfg만으로
한글 렌더링·내장 한국어 폰트 매칭·무프리즈 동작 확인. 플러그인 DLL 불필요.

## 사용 (CLI)

    kerbaloc status              # 설치 감지·현재 언어
    kerbaloc scan                # 번역 대상 모드 스캔 (CKAN/AVC 기반 식별 + 소스 해시)
    kerbaloc doctor [--backup x.zip]   # 구방식 오염(en-us 내 한글) 감지/백업
    kerbaloc install <팩 디렉터리>      # GameData/KerbaLoc/<lang>/<ModId>/ 에 설치
    kerbaloc remove <ModId>
    kerbaloc enable | disable    # 언어 ko ↔ en-us
    kerbaloc validate <팩 디렉터리>     # 팩 검증 (CI에서도 동일 사용)
    kerbaloc translate <ModId> [--nick 이름] [--install] [--resume]
                                 # Gemini 번역 → 검증 루프 → 팩 생성 (GEMINI_API_KEY 필요)
    kerbaloc db list             # 원격 번역 DB 팩 목록 (계정 불필요)
    kerbaloc db install <ModId>  # DB에서 다운로드·해시 검증·원문 대조·설치
    kerbaloc db validate|index <레포경로>   # kerbaloc-db CI가 사용하는 서브커맨드

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
