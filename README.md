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

## 개발

    cd kerbaloc && cargo test --workspace

- 설계 스펙: `docs/superpowers/specs/2026-08-16-ksp-ko-redesign-design.md` (+ appendix A–F)
- 구현 계획: `docs/superpowers/plans/` (Plan 1/5 완료: core+CLI)
- 골든 데이터: `research/stock-dictionary/` (스톡 en-us/ja/zh-cn + Breaking Ground)
- 구 Python 구현(`src/ksp_translator/`)은 참조용 — 신규 개발은 `kerbaloc/` Rust workspace

## 구성요소 (로드맵)

| Plan | 내용 | 상태 |
|---|---|---|
| 1 | kerbaloc-core + CLI (파서/해시/검증/스캔/설치/언어 전환) | ✅ 완료 |
| 2 | 번역 DB 레포(kerbaloc-db) + CI | 예정 (GitHub 조직 필요) |
| 3 | LLM 번역 파이프라인 + 팩 생성 | ✅ 완료 (CRP 240키 E2E: $0.0093, 검수 0) |
| 4 | Tauri 스튜디오 (대시보드/에디터/공유) | 예정 |
| 5 | 익명 업로드 프록시 (Cloudflare Worker) | 예정 |
