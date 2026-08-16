# KerbaLoc 업로드 프록시 + 스튜디오 구현 계획 (Plan 5 + 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** ① `kerbaloc-proxy`(Cloudflare Worker)로 계정 없는 원클릭 공유(자동 PR)를 완성하고 CLI `db share`로 연결. ② `kerbaloc-app`(Tauri 2 + Svelte 5) 스튜디오로 대시보드·번역·검수·설치·공유를 GUI화.

**Architecture:** 프록시는 부록 C의 v1 축소판(A′: 운영자 무기한 FG PAT). 스튜디오는 kerbaloc-core를 Tauri 커맨드로 노출하고 이벤트로 진행률 스트림.

**Spec:** 부록 C(프록시)·H(A′) / 스펙 UX 섹션(스튜디오)

## Plan 5 — 프록시 (kerbaloc-proxy)

### v1 의도적 축소 (부록 C 대비, 코드 주석에도 명시)
- 3단계 API(begin/artifact/finalize) → **단일 `POST /v1/submit`** (JSON, 파일별 base64). 팩이 수 KB~수백 KB라 스트리밍 불필요. 총 디코드 2MB·파일 8개 상한.
- zip 전개 CI 단계 불필요 — Worker가 **최종 경로에 파일별 blob**을 직접 커밋 (서브리퀘스트 ≤ 50 내).
- PoW·Turnstile 미구현(TODO 주석) — 레이트리밋(DO)과 PR 사람 검수가 v1 방어선.
- 봇 계정 없음 — 운영자 본인 FG PAT (secret `GH_PAT`).

### 구조
```
kerbaloc-proxy/
├── wrangler.toml         # DO 바인딩(RATE_LIMITER, SQLite), vars REPO
├── package.json          # wrangler, vitest, @cloudflare/vitest-pool-workers, typescript
├── src/index.ts          # fetch 핸들러: GET /v1/health, POST /v1/submit
├── src/validate.ts       # 순수 검증 함수(스키마·경로 allowlist·크기·닉네임) — 단위 테스트 대상
├── src/github.ts         # blob→tree→commit→ref→PR 시퀀스 (fetch 직접)
├── src/ratelimit.ts      # Durable Object (SQLite events 테이블, IP해시 3/h·10/일 + 전역 30/h)
└── test/                 # vitest: validate 단위 + fetchMock 통합
```

### Tasks
1. 스캐폴드 + validate.ts + 단위 테스트 (경로 allowlist `^(pack\.json|Localization/[A-Za-z0-9._-]+\.cfg|Patches/[A-Za-z0-9._-]+\.cfg)$`, 닉 `^[A-Za-z0-9][A-Za-z0-9 _.-]{0,23}$`, modId `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`, variantId 형식, 크기 상한, PR 본문 새니타이즈: `@`→`@​`, `#\d`→`#​\d`)
2. ratelimit.ts (DO SQLite) + github.ts (시퀀스, PAT는 env) + index.ts 조립
3. vitest 통합 테스트 (fetchMock으로 GitHub API 목킹: submit 성공 → PR 생성 콜 검증 / 레이트리밋 429 / 경로 위반 400)
4. 배포: 사용자 `npx wrangler login` → `wrangler deploy` → 사용자 FG PAT 발급 안내 → `wrangler secret put GH_PAT` → 실제 제출 1회로 kerbaloc-db에 PR 생기는지 E2E
5. CLI 통합: `kerbaloc db share <팩디렉터리> --nick <이름>` — 팩 파일들 base64로 POST, PR URL 출력. env `KERBALOC_PROXY_URL`.

## Plan 4 — 스튜디오 (kerbaloc-app)

### v1 화면 (스펙 UX 축소)
- **대시보드**: scan 유닛 × DB 인덱스 병합 테이블 [모드 | 버전 | 키수 | 상태(미번역/DB N변형/설치됨/낡음) | 액션(설치·제거·번역)] + 상단 언어 토글(enable/disable) + doctor 배지
- **번역 진행**: translate 실행 시 모달 — 배치 진행/비용 실시간(이벤트), 완료 시 결과 요약
- **검수**: review 항목 테이블 [키 | 원문 | 후보 | 위반] — 수정 입력 → 즉시 재검증 → 통과 시 팩에 반영
- **공유**: 팩 선택 → 닉네임 입력 → 프록시 제출 → PR 링크 표시
- **설정**: KSP 경로, GEMINI_API_KEY 저장(로컬 파일)

### Tasks
6. Tauri 2 스캐폴드 (`kerbaloc/kerbaloc-app`, Svelte 5 + Vite + TS). workspace 멤버 추가. `cargo tauri dev` 부팅 확인
7. Tauri 커맨드: `scan_units`, `db_index`, `game_status`, `set_language`, `install_from_db`, `remove_pack`, `translate_mod`(spawn + 이벤트 `translate-progress`), `share_pack`, `save_settings/load_settings`
8. 대시보드 UI (테이블 + 액션 버튼 + 토글)
9. 번역 모달 + 검수 화면 + 공유 다이얼로그 + 설정
10. E2E: 실행 → scan 표시 → DB 설치 → 소형 모드 번역 → 검수 1건 수정 → 공유(프록시). clippy/빌드/README 갱신, 머지

## Self-Review
플랜 5→4 순서(공유 백엔드 먼저). 프록시 배포(Task 4)는 사용자 개입(wrangler login, PAT) 필요 — 해당 시점에 명시적으로 요청. 스튜디오는 core 함수 재사용이라 신규 로직 최소.
