# 부록 F — 실게임 스파이크 결과 (2026-08-16)

환경: KSP 1.12.5 (Steam, build 03190). 사용자가 GameData를 백업 후 서드파티 모드를 모두 제거하고 Squad / SquadExpansion / KerbaLoc만 남긴 상태에서 검증.

절차: `kerbaloc install tests-fixtures/spike-pack` (메인 메뉴 4키 ko 팩 → `GameData/KerbaLoc/ko/Squad`) → `kerbaloc enable` (`language = ko`) → 게임 실행.

## 결과

| # | 검증 항목 | 결과 |
|---|---|---|
| a | 한글 렌더링 | **성공** — 메인 메뉴에 "게임 시작 (스파이크)" 등 한글 정상 표시, 폰트 박스(□) 없음 |
| b | 메뉴 폰트 매칭 | **성공** — 메뉴 폰트가 한국어 폰트로 전환됨 (일반 텍스트는 불변). 이는 `FontLoader.LoadFonts()` → `MenuFontSettings.ChangeLanguage("ko")`가 level0의 "ko" 폰트 항목(NotoSansCJK-K)에 매칭된 결과로, **엔진의 의도된 동작이자 ko 폰트 내장 가설의 실증**. 시각적 위화감(라틴 글리프가 CJK 폰트로 렌더링)은 인지된 특성 — 필요 시 후속 과제 |
| c | 미번역 키 영어 폴백 | **1차 판정 불가** (Dobie 잔재로 전체가 한국어 표시) → 스파이크 후 Squad·Serenity dictionary.cfg를 스톡 원본(미러)으로 복원 완료, **재실행 검증 대기** |
| d | 로딩 정상(프리즈 없음) | **성공** |

## 스펙 "검증으로 해소 예정" 항목 판정

- [x] `language = ko`에서 메뉴 폰트 매칭 → **확인됨** (b)
- [x] Lingoona `setLanguage("ko")` 조용히 통과 → **확인됨** (d — 프리즈/크래시 없음)
- [ ] 런처/Steam의 buildID64.txt 덮어쓰기 여부 → 미확인 (장기 관찰 항목)
- [ ] TranslateBranch 시점 폴백 일관성 → (c) 재검과 함께 파트 로딩 확인 필요
- [ ] 구방식 CRP 프리즈 재현 → 모드 제거 상태라 보류 (Plan 3 팩 생성 후 E2E에서)

## 부수 작업 (사용자 지시)

- Squad `dictionary.cfg`: Dobie 패치(1.0MB, en-us 단일 노드 한국어) → 스톡 미러 en-us+ja+zh-cn 병합본(2.97MB)으로 교체
- Serenity `dictionary.cfg`: 동일하게 스톡 미러 3개 언어 병합본(0.2MB)으로 교체 — Breaking Ground 원본은 [Apofoo/KSP-Localization](https://github.com/Apofoo/KSP-Localization)의 "Ground Expansion DLC" 폴더에서 추가 확보(en-us/ja/zh-cn 각 724키, `research/stock-dictionary/bg-*.cfg`로 보존)
- 교체 후 `kerbaloc doctor` = **오염 없음**

## 결론

**플랜 B(FontLoader 플러그인) 불필요.** buildID64 `ko` 전환 + `Localization { ko }` cfg 추가만으로 한국어화가 성립한다는 설계 전제가 실게임에서 입증됨.
