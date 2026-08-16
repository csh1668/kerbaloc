# 부록 G — 프리즈 재현 실험 결과 (2026-08-16, 실게임)

환경: KSP 1.12.5, GameData = Squad + SquadExpansion(스톡 복원 완료) + CommunityResourcePack + KerbaLoc. `language = ko`.

실험 도구: `kerbaloc-core/examples/gen_test_packs.rs` — 스톡/CRP en-us 전체를 의사번역(pseudo-loc)한 ko 팩 생성.
- **good**: 모든 값에 `한` 접두, 토큰·구조 100% 보존 (Squad 12,030키 + CRP 240키 전량 번역)
- **bad**: good + 구도구 LLM 손상 시뮬레이션 — 3키마다 Lingoona 토큰 절단(`<<1>>`→`<<1>`), 50키마다 값에 원시 `{}` 주입

## 테스트 ① — 우리 방식 (good, `kerbaloc validate` 통과 후 설치)

**결과: 완전 정상.** 로딩 완주, 메뉴·파트·자원 전부 의사번역 표시, 프리즈 없음.

→ **CRP DisplayName·단위·포맷 문자열을 전부 번역해도 안전함이 실증됨.** "위험한 키를 번역해서 프리즈"라는 가설 최종 기각 (부록 A의 마스크 기각과 정합).

## 테스트 ② — 구방식 재현 (bad, 검증 우회 강제 설치)

`kerbaloc validate`는 bad 팩을 거부(원시 중괄호·토큰 손상 검출)했고, 구도구처럼 검증 없이 수동 복사로 설치.

**결과: 로딩 프리즈 재현** — "Loading Resources" 단계에서 영구 정지.

### 원인 완전 특정 (Player.log 스택트레이스)

```
Resource Hydrogen added to database        ← 다음 자원 = Karbonite
ArgumentOutOfRangeException: Index and length must refer to a location within the string.
  at System.String.Substring
  at PartResourceDefinition.GetShortName (length=2)     ← displayName.Substring(0, 2)
  at PartResourceDefinition.Load
  at PartResourceLibrary.LoadDefinitions
  at GameDatabase.<CreateDatabase>.MoveNext             ← 코루틴 사망 = 로딩 영구 정지
```

인과 사슬:

1. bad 팩의 `#LOC_CRP_Karbonite_DisplayName = {한Karbonite}` — 게임의 ConfigNode 리더가 값의 `{`를 구조 문자로 소비 → **태그 값이 빈 문자열**로 파싱됨
2. `Localizer.TranslateBranch`가 CRP RESOURCE_DEFINITION의 `displayName`을 그 빈 문자열로 치환
3. Karbonite는 `abbreviation` 필드가 없어 `PartResourceDefinition.GetShortName()`이 `displayName.Substring(0, 2)` 실행 → `ArgumentOutOfRangeException`
4. 예외가 `GameDatabase.CreateDatabase` 코루틴을 죽임 → 로딩 화면 영구 멈춤
5. **예외는 KSP.log에 기록되지 않고 Unity Player.log에만 남음** → 구방식 시절 원인 규명이 어려웠던 이유

### 파생 발견 — 2자 미만 DisplayName도 동일 프리즈

`Substring(0, 2)`는 displayName이 **2자 미만이기만 해도** 던진다. 즉 구조 손상 없이도 LLM이 `Ore → 광` 같은 1자 번역을 내면 그대로 프리즈다. 구방식 프리즈의 또 다른 유력 경로.

→ 검증기에 규칙 추가 완료: `*_DisplayName` 키의 번역이 2자 미만이면 **오류** (`displayname-too-short`).

### 부수 확인

- Squad bad 팩의 토큰 절단(`<<1>`) 수천 건은 **로딩 자체는 통과**시킴 — Lingoona 토큰은 로딩이 아니라 런타임 Format 시점에 소비되므로 프리즈가 아닌 표시 오류로 나타남. 프리즈의 주범은 구조 손상(→ 빈/짧은 값)이다.
- 이 실험 과정에서 검증기 구멍 2개 발견·수정: ① 값의 원시 중괄호는 파싱 후엔 흔적이 사라지므로 **파싱 전 원문 줄 검사** 필요 ② `validate`/`install`이 설치본 스캔으로 en-us 원문을 자동 확보해 토큰 대조 수행.

## 결론

| 가설 | 판정 |
|---|---|
| 위험한 키(DisplayName/단위)를 번역해서 프리즈 | **기각** (테스트 ①: 전량 번역 정상) |
| 번역 내용 손상(구조 문자 → 빈 값, 2자 미만 값)이 프리즈 | **확정** (테스트 ②: 재현 + 스택트레이스) |
| 우리 방식(검증 게이트 + 추가형 팩)이 프리즈를 방지 | **확정** (validate가 bad 팩 거부, good 팩 정상) |

스펙 "리스크 및 미지수"의 "구방식 CRP 프리즈의 정확한 소비자 코드" 항목 **해소**: `PartResourceDefinition.GetShortName`.
