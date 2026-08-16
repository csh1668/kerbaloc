# 부록 D — ModId 정규화 + 버전 파악 설계 (Opus 서브에이전트, 2026-08-16)

조사 기반: 실제 설치본 (KSP 1.12.5, GameData 최상위 216폴더, CKAN `installed_modules` 240개, `installed_files` 28,934항목, `.version` 162개, cfg 9,818개) + CKAN Spec.md v1.34 / KSP-AVC 스키마·파서 소스.

## 0. 실물 조사 요약 (설계 근거)

### 0.1 CKAN registry.json 구조

```
registry.json (4.8MB)
├── installed_dlls        : { "<DllName>": "GameData/.../X.dll" }
├── installed_modules     : { "<identifier>": { install_time, auto_installed,
│                             installed_files: {...}, source_module: {...} } }
├── installed_files       : { "GameData/a/b/c.cfg": "<identifier>" }   ★ 평면 역인덱스
├── available_modules     : (레거시. 항상 빈 객체로 직렬화 — 신뢰 금지)
└── download_counts       : (동상, 레거시)
```

`source_module` 주요 필드: `identifier`, `name`, `version`, `author`, `license`, **`localizations`(공식 번역 보유 언어의 무료 인덱스)**, `resources.remote-avc`, `install`, `provides`, `kind`(`dlc` 등), `download_hash`.

**핵심 발견 — `installed_files`는 "파일 → identifier" 평면 맵.** 폴더 단위 추측이 필요 없다. 로컬라이제이션 소스 파일 경로를 키로 넣으면 소유 identifier가 정확히 나온다. 이것이 설계 전체의 1차 프리미티브. (CKAN 소스 확인: Windows에서 이 조회는 대소문자 무시.)

주의: `registry.json`은 공개 스펙이 없는 클라이언트 내부 포맷 — 방어적으로 파싱하고 실패 시 조용히 폴백.

### 0.2 실측된 불일치 통계

| 현상 | 실측 |
|---|---|
| 폴더명 ≠ CKAN identifier | **60건 이상** |
| 한 폴더를 2개 이상 CKAN 패키지가 공유 | **19개 폴더** |
| 한 identifier가 여러 최상위 폴더에 설치 | 4건 |
| CKAN 소유가 아닌(수동 설치) 최상위 폴더 | 12개 |
| `provides` 별칭 선언 | 24건 |
| `.version` 엄격 JSON 파싱 실패 | **10 / 162** |
| `.version`의 `VERSION`이 문자열 | **14 / 162** |
| `.version`에 UTF-8 BOM | 4 / 162 |

대표 예: `000_Toolbar→Toolbar`, `000_Harmony→Harmony2`, `Diazo→AGExt`, `Kerbaltek→HyperEdit`, `OPM→OuterPlanetsMod`, `[x]_Science!→xScienceContinued`, `Adiri's TUFX Profiles→C1ustasTUFXProfiles`.
한 폴더 다패키지: `ASET`(3), `TriggerTech`(4), `JSI`(2), `WildBlueIndustries`(4).
한 identifier 다폴더: `SimpleConstruction`(2), `RestockWaterfallExpansion`(3).
provides: `NewTantares provides Tantares`, `FerramAerospaceResearchContinued provides FAR`, `TweakScaleRescaled provides TweakScale` 등.

### 0.3 로컬라이제이션 소스 파일의 실제 배치

cfg 9,818개 중 `Localization/en-us` 노드 보유 **152개**. 파일명/폴더명 규칙 신뢰 불가:
`KAS/Lang/en-us.cfg`, `Tantares/localisation/`(영국식, 12분할), `DockRotate/dictionary-en-us.cfg`, `MPE/en-us.cfg`(폴더 없음), `B9_Aerospace_ProceduralWings/Localization/refactor.cfg`(파일명에 en-us 없음), `KSPCommunityFixes/MMPatches/.../ManufacturerFixes.cfg`(패치 파일 안에 노드), `002_CommunityPartsTitles/Localization/CPT_*.cfg`(43개), `Squad/.../dictionary.cfg`의 `en-us// 인라인주석`.

→ **파일명 기반 탐지 금지. ConfigNode 파싱 후 노드 존재로 판정. 노드명 파싱 시 `//` 제거 필수.**

### 0.4 `.version` (KSP-AVC) 실물의 지저분함

후행 쉼표(ASET 등), 과다 닫는 중괄호, VERSION 문자열형(`"3.3.1.0"`, `"v1.2.0"`), 루트 직속(`999_Scale_Redist.version`), 한 폴더 3개(ASET), `Versioning/`·`Version/`·`Plugins/` 하위 배치.

파서 사실(KSP-AVC 소스 확인): 필수 필드는 `NAME`,`VERSION`뿐 / VERSION 문자열형은 스키마 공식 허용(`anyOf[string,object]`), 4자리도 양쪽 파서 수용 / 주석 미지원 / 후행 쉼표는 MiniJSON이 관용 / `{` 앞 잡문자 제거 / 키 대소문자 무시 / `SetVersion`이 `[^\d.\-]`를 조용히 제거하므로 원본 문자열 보존 필요 / 탐색은 `*.version` 재귀, stem 자유 / CKAN 소스 주석: ".version 버전은 unreliable".

### 0.5 CKAN identifier 규칙 (Spec.md v1.34)

- 문자셋: ASCII 영문자+숫자+`-`만. **`.` 없음** → `local.` 접두는 영구 무충돌.
- 전역 유일(대소문자 무시 기준 포함).
- `install` 기본값 `{"find": "<identifier>"}`가 "폴더명=identifier" 관습의 근원(강제 아님, 실측 60건+ 위반).
- `-Core`/`-Config` 분할은 스펙에 없는 커뮤니티 관행 → 하드코딩 금지.

## 1. ModId 정규화 알고리즘

### 1.1 원칙
1. **번역 단위 = "로컬라이제이션 소스를 소유하는 배포 패키지"**, 폴더가 아니다.
2. CKAN identifier를 **verbatim** 사용 (슬러그화·소문자화 금지).
3. CKAN 밖 ModId는 **`local.` 접두**.
4. 스톡/DLC 예약 ID: `Squad`, `MakingHistory-DLC`, `BreakingGround-DLC`.
5. 비교는 대소문자 무시, 저장은 최초 등록 형태.

### 1.2 의사코드 (요약)

```python
def resolve_owner(path, gamedata, ckan) -> OwnerKey:
    rel = "GameData/" + relpath(path)
    if ckan and (ident := ckan.installed_files_ci.get(rel.lower())):
        return OwnerKey("ckan", ident)                       # 1순위: 파일 소유권
    top = first_dir(path)
    if top == "Squad": return OwnerKey("stock", "Squad")     # 2순위: 예약 경로
    if top == "SquadExpansion": return OwnerKey("stock", DLC_MAP[second(path)])
    avc = deepest_scoping_avc(path, root=gamedata/top)       # 3순위: 최근접 .version
    if avc:
        if avc.github: return OwnerKey("avc-gh", avc.github.repo)
        return OwnerKey("avc", avc.file_stem)
    return OwnerKey("folder", top)                           # 4순위: 폴더명

def slug(s):   # local.* 생성용
    s = NFC(s); s = re.sub(r"^\d{3}_", "", s); s = re.sub(r"^z{1,3}_", "", s, re.I)
    s = s.replace("'", ""); s = re.sub(r"[^A-Za-z0-9]+", "-", s)
    return re.sub(r"-{2,}", "-", s).strip("-") or "unnamed"
```

| 순위 | 근거 | 산출 ModId | 정확도 |
|---|---|---|---|
| 1 | CKAN `installed_files[경로]` | identifier verbatim | 정확 |
| 2 | 예약 스톡 경로 | `Squad`/`MakingHistory-DLC`/`BreakingGround-DLC` | 정확 |
| 3 | 최근접 `.version`의 GITHUB repo | `local.<repo>` | 높음 |
| 4 | 최근접 `.version` stem | `local.<stem>` | 중간 |
| 5 | 최상위 폴더명 slug | `local.<slug>` | 낮음 |

### 1.4 Core/Config 분할
실측상 로컬라이제이션 파일은 항상 한쪽만 소유 → 파일 소유권 기반이 자동으로 올바른 쪽 선택. **폴더→identifier 매핑을 만들지 말 것. 파일→identifier만 쓴다.**

### 1.5 별칭(alias)
`provides`(24건)와 과거 폴더명은 `mod.json.match.aliases`에 누적. 매칭은 후보 제시일 뿐, 설치 가부는 소스 해시가 결정. `provides`는 자동 설치 근거로 삼지 말 것.

### 1.6 CKAN 미설치 사용자 연결
index에 팩별 `match` 블록(`folders`/`avc_stems`/`avc_github`/`source_hashes`) — 우선순위 `source_hashes` > `avc_github` > `avc_stems` > `folders`. 해시가 맞으면 CKAN 없이 버전까지 확정.

## 2. 버전 파악 알고리즘

- **낡음 판정은 절대 버전 번호로 하지 않는다. 오직 소스 해시.** 버전은 표시·메타·정렬 힌트.
- 우선순위: **CKAN(high) → .version(medium) → 경로 휴리스틱(low) → unknown**. 출처·신뢰도를 UI 배지로 병기.
- `.version` **관대 파서는 필수**(실측 6% 엄격 실패): BOM/잡문자 제거, 후행 쉼표, 주석 허용, 과다 `}` 관용, 키 대소문자 무시, VERSION 문자열/객체 양형 + `v` 접두 + 1~4자리, 실패 시 조용히 폴백.
- 표시: 🟢 CKAN / 🟡 .version / 🟠 경로 추정 / ⚪ 미상 + 모든 행에 `소스 #<hash8>` 병기.

### 2.4 낡음 판정
```
local_hash == pack.hash        → fresh          (버전 번호 달라도 무시)
local_keys_hash == pack.keys   → values-changed (설치 허용 + 변경 키 재번역 제안)
else                           → stale          (다이얼로그 + 키 diff)
```
버전 번호가 같아도 해시가 다르면 stale (버전 없는 핫픽스 재배포가 흔함).

## 3. 소스 해시 정규화

- **바이트가 아니라 파싱된 (키,값) 집합**을 해시. 주석/CRLF/BOM/공백/순서 불감.
- 태그형: 소유 cfg 전체의 `Localization/en-us` 합집합(하위 노드는 `sub/#tag` 경로 반영). 비태그형: `PART`/`AGENT`/`RESOURCE_DEFINITION`/`EXPERIMENT_DEFINITION`의 번역 대상 필드만, 키 = `PART:<name>:title`(파일 경로 무관).
- 정규화: 개행 LF 통일, NFC, 줄끝 공백 제거, strip. 보존: 리터럴 `\n`, `<<1>>`, `^`, `｢｣`, 내부 연속 공백.
- 인코딩: BOM 제거 후 UTF-8 strict, 실패 시 replace + 경고 (cp949 추측 금지).
- canonical blob: 키 바이트순 정렬, `key\x1fvalue\x1e` 연결 → sha256.
- 산출 3종: `source.hash`(낡음 판정 진실) / `keys_hash`(키 변화 vs 값 변화) / `mask_hash`(공식 번역 커버 변화) + 키별 8-hex 지문(`keys`, 1,000키 초과 시 사이드카).
- **알고리즘 버전 접두 `v1:sha256:…`** — 규칙 변경 시 전면 오탐 방지.

## 4. `mod.json` 스키마 초안

(부록 E의 `kerbaloc/mod@1`과 통합 예정 — 본 부록 안의 필드: `mod_id`, `id_source`, `ckan_identifier`, `display_name(_ko)`, `match{folders,avc_stems,avc_github,aliases}`, `authors`, `license`, `homepage`, `repository`, `content_kinds[tag|patch]`, `official_localizations`, `mask_policy`, `known_versions[{version, version_source, source{hash,keys_hash,mask_hash,key_count}, first_seen, observed_by}]`, `deprecated_by`)

CI 불변식: mod_id == 디렉터리명 / ckan 유래면 identifier == mod_id / known_versions 해시 중복 금지 / 전 레포 대소문자 무시 유일 / pack의 source.hash는 known_versions에 존재(없으면 CI 자동 추가) / 태그 중복 검출은 전역.

## 5. 엣지 케이스 표 (30건 요약)

CKAN 미설치(12폴더) / 접두 폴더(`000_`,`zzz_`,`999_`) / 특수문자 폴더명 / 한 폴더 다패키지(파일 소유권으로 분리) / 한 패키지 다폴더(합집합 해시) / Core 분할(자동) / 폴더명≠identifier(60건+) / 포크 개명(provides→aliases) / 루트 직속 파일(top이 파일인 경우 처리) / 중첩 Localization(재귀+소유권) / 다중 파일(합집합) / 파일명 비표준(내용 판정) / 인라인 주석(`en-us//`) / .version 파싱 실패(관대 파서) / VERSION 문자열형 / 한 폴더 다.version(최심 스코프) / .version 위치 자유(전체 재귀) / DLC(예약 매핑) / 스톡(예약 ID) / 오염 설치본(en-us에 한글 → 해시 신뢰 불가 표시, doctor 복원 유도) / 폴더명 변경(해시로 매칭) / 태그 중복(전역 검사) / 인코딩 이상(경고 배지) / CRLF 혼재(무영향) / 버전 같은데 내용 다름(stale) / 버전 없음(기능 저하 없음) / `KSP-Loc(KerbaLoc)/**` 자기 제외 / MM 캐시 제외.

## 6. 스펙 반영 제안 (요지)

> **ModId = CKAN `installed_files` 역인덱스 identifier(verbatim) → 예약 스톡 ID → `local.<AVC GitHub repo>` → `local.<AVC stem>` → `local.<slug(폴더명)>`. 번역 단위는 폴더가 아니라 "로컬라이제이션 소스 파일의 소유 패키지".**

- 스펙의 "해시 → .version → CKAN" 순서는 **낡음 판정** 축이고, **식별(ModId)** 축은 정반대(CKAN → .version → 폴더) — 두 축 분리 기술.
- CKAN `source_module.localizations`가 공식 번역 보유 언어의 무료 인덱스.
- `.version` 관대 파싱은 필수(실측 6% 실패, 9% 문자열형).
- 로컬라이제이션 탐지는 파일명 기반이면 실패(152개 중 60개+ 비표준) — ConfigNode 파싱 기반.
- `registry.json`의 `available_modules`/`download_counts`는 레거시(빈 객체) — 의존 금지.
