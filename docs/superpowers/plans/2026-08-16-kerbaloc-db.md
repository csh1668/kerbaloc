# KerbaLoc DB 레포 + CI + 클라이언트 구현 계획 (Plan 2/5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `csh1668/kerbaloc-db` public 레포에 팩·용어집·블랙리스트를 두고, PR 검증·인덱스 생성·manifest 게시 CI를 돌리고, `kerbaloc db list`/`kerbaloc db install <ModId>`로 계정 없이 다운로드-설치되는 전체 루프를 완성한다.

**Architecture:** 부록 E의 v1 축소판 + 부록 H의 A′(개인 레포). 인덱스 생성·검증 로직은 kerbaloc-cli 서브커맨드로 구현해 CI가 같은 바이너리를 사용(로직 단일화). 순환 SHA 문제는 부록 E §3.5 방식 — 레포 내 index는 commit 없이, release 자산 `manifest.json`이 최종 SHA 보유. 다운로드는 jsDelivr@SHA 기본 + raw 폴백.

**Tech Stack:** 기존 kerbaloc-core/cli + GitHub Actions(ubuntu, Swatinem/rust-cache) + gh CLI

**Spec:** 부록 E(스키마·CI)·H(A′) / 시드 데이터: CRP 실번역 팩(Plan 3 산출물), `glossary/core.ko.json`, 블랙리스트 스켈레톤

## Global Constraints

- DB 레포: `csh1668/kerbaloc-db` (public, 개인 계정 — A′). CI는 `csh1668/kerbaloc`을 함께 체크아웃해 `kerbaloc-cli`를 빌드(캐시).
- 인덱스는 **결정적**(타임스탬프 없음) — 같은 내용이면 같은 바이트.
- v1 범위 제외(주석 명시): source.sig.json, mask.json, 팩별 zip 릴리스(개별 파일 fetch로 충분, jsDelivr 파일당 20MB 여유), 태그 충돌 전역 검사, nightly. 업로드 프록시는 Plan 5.
- manifest.json 스키마: `{"schema":"kerbaloc/manifest@1","commit":"<sha>","indexSha256":"<hex>","repo":"csh1668/kerbaloc-db"}`
- index/ko.json 스키마(v1 축소): `{"schema":"kerbaloc/index@1","lang":"ko","packs":[{"modId","variants":[{"variantId","path","srcSha256","keysTranslated","keysTarget","model","files":[{"path","sha256","sizeB"}]}]}]}` — 해시는 전체 64hex(축약 없음, v1 단순화).

## Tasks

### Task 1: `kerbaloc db index` + `db validate` 서브커맨드
- Create `kerbaloc-core/src/dbrepo.rs`: `build_index(repo_dir) -> anyhow::Result<serde_json::Value>` (packs/** 순회, 각 variant의 pack.json 로드 + 파일 sha256/size 계산, modId·variantId 정렬) / `validate_repo(repo_dir) -> ValidationReport`(모든 variant에 `pack::validate_pack(dir, None)` + pack.json의 mod_id가 경로와 일치 + variantId 형식 `^\d{4}-\d{2}-\d{2}-[a-z0-9-]+$`)
- CLI: `kerbaloc db index <repoDir> [--out <파일>]` / `kerbaloc db validate <repoDir>` (오류 시 exit 1)
- Test `tests/dbrepo.rs`: 합성 레포(팩 2변형) → index 구조·정렬·결정성(2회 호출 동일), 깨진 팩 → validate 실패
- 커밋 `feat(core): DB 레포 인덱스 생성·전체 검증`

### Task 2: kerbaloc-db 레포 스캐폴드 (로컬 `C:\Works\kerbaloc-db`)
- `README.md`(용도·기여 방법·구조), `blacklist.json`(스키마+빈 목록+주석 필드), `glossary/core.ko.json`(메인 레포에서 복사 — 이후 DB가 정본), `packs/CommunityResourcePack/ko/variants/2026-08-16-gemini31flashlite-spike/`(Plan 3 산출물 복사)
- `.github/workflows/pr-validate.yml`: PR 시 — checkout db + checkout csh1668/kerbaloc + rust-cache + `cargo run -p kerbaloc-cli -- db validate .`
- `.github/workflows/publish.yml`: main push(`packs/**`,`glossary/**`,`blacklist.json`) 시 — ① db validate ② `db index . --out index/ko.json` ③ 변경 시 `[skip ci]` 커밋·푸시 ④ 그 커밋 SHA로 manifest.json 생성 ⑤ `gh release upload db-latest manifest.json --clobber`(release 없으면 create). permissions `contents: write`, concurrency 직렬.
- 로컬에서 `kerbaloc db validate`·`db index` 통과 확인 후 git init·커밋
- 커밋(kerbaloc-db) `chore: 레포 스캐폴드 — 시드 팩·용어집·블랙리스트·CI`

### Task 3: GitHub 레포 생성·푸시·CI 확인
- `gh repo create csh1668/kerbaloc-db --public --source . --push`
- Actions 실행 확인(`gh run watch/list`), publish가 index 커밋 + db-latest release에 manifest 업로드까지 완주하는지 검증. 실패 시 수정 반복.

### Task 4: 클라이언트 — `kerbaloc db list` / `db install <ModId>`
- Create `kerbaloc-core/src/dbclient.rs`:
  - `fetch_manifest() -> Manifest` (`https://github.com/csh1668/kerbaloc-db/releases/latest/download/manifest.json`)
  - `fetch_index(&manifest) -> Index` (jsDelivr `https://cdn.jsdelivr.net/gh/csh1668/kerbaloc-db@<commit>/index/ko.json`, sha256 대조, 실패 시 raw 폴백)
  - `download_variant(&manifest, &variant, dest_dir)` (files[] 병렬 fetch + sha256 검증 + 임시 디렉터리 조립)
- CLI: `db list`(모드·변형·키수 표), `db install <ModId> [--variant <id>]`(기본 = 첫 변형 → 다운로드 → `pack::validate_pack`(스캔 원문 대조) → `install_pack`)
- Test: 응답 파서·sha 검증 단위 테스트(HTTP 목 없이 순수 함수 분리), 실네트워크는 Step E2E에서
- 커밋 `feat: DB 클라이언트 — manifest→jsDelivr 인덱스→검증 설치`

### Task 5: E2E + 마무리
- 로컬 설치 팩 제거 → `kerbaloc db list` → `kerbaloc db install CommunityResourcePack` → `GameData/KerbaLoc/ko/CommunityResourcePack` 확인
- clippy/fmt/전체 테스트, README 로드맵 갱신(Plan 2 완료), main 머지·푸시

## Self-Review
- 부록 E 대비 의도적 축소를 Global Constraints에 명시. 타입: dbrepo(T1)→CI(T2-3), dbclient(T4)→pack::install_pack(Plan1). CI와 클라이언트가 같은 검증기 사용.
