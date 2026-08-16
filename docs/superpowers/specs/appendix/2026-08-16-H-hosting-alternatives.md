# 부록 H — 번역 DB 호스팅 대안 조사 (Opus 서브에이전트, 2026-08-16)

> 결론: **현행안(GitHub+Worker) 유지 + 30분짜리 단순화** — 조직·봇 계정·PAT 로테이션 부담은 GitHub의 요구가 아니라 "조직을 쓰기로 한 선택"에서 파생된 것. **운영자 개인 public 레포 + 본인 무기한 fine-grained PAT**로 전부 제거된다 (A′안).
> 부록 C §1 D1 정정: "개인 레포 + 봇 협업자 불가"는 여전히 참이나, **"운영자 본인 레포 + 본인 토큰"은 가능** — 이 사실이 누락되어 조직이 필수로 결론났었다.

## 핵심 결론 3줄

1. 현행안을 교체할 이유 없음. 다만 조직+봇+90일 로테이션 부담은 전부 제거 가능 (A′).
2. "진짜 무가입 업로드" 서비스는 전부 영속성·한국 접근성·ToS 중 하나 이상에서 탈락. 서버 측 자격증명 대행 주체(Worker)는 구조적으로 불가피 — 부록 C 결론 재확인.
3. SpaceDock은 발견성 보조 채널로만 선택적 병행.

## 종합 비교 요지 (전체 표는 조사 원문 기준)

| 후보군 | 판정 | 결정적 사유 |
|---|---|---|
| **A′ 개인 레포 + 본인 무기한 FG PAT** | **★ 권고** | 무기한 FG PAT는 정식 허용("Infinite lifetimes are allowed but may be blocked by an organization policy") — 조직이 없으면 로테이션 개념 자체가 소멸 |
| B1 GitHub App (개인 레포 설치 가능) | 차선 | 봇 아이덴티티 필요 시. 설치 토큰 1시간 자동 갱신, A′에서 무중단 전환 가능 |
| B3 Codeberg | ❌ | ToS가 CDN·파일호스팅 명시 금지 + jsDelivr 미지원 + 한국 259ms |
| B4 GitLab | ❌ | jsDelivr 미지원, CI 400분/월, Free 축소 이력 |
| C1 R2 / C4 Firebase | ❌ | 카드 필수 (무료 제약 위반) |
| C3 Supabase | ❌ | 7일 무활동 시 프로젝트 정지, 백업 보존 0일 |
| C8 Deta/Cyclic/Glitch/Railway/Fly 무료 등 | ❌ | 2024~2026 전부 종료 또는 무료 폐지 |
| D 익명 업로드 호스트 (0x0/transfer.sh/catbox/litterbox) | ❌ | **실측**: 0x0는 한국에서 TCP 연결 실패, transfer.sh 소멸, catbox는 한국 IP 업로드 거부 + 익명 파일 2년 후 삭제, litterbox 72시간 |
| E1 SpaceDock | 보조만 | API 대행 업로드 가능하나 즉시 게시(검수 없음)·"언제든 삭제 가능" ToS·핀란드 306ms. 대표 모드 1개 버전 갱신 형태만, 운영자 사전 합의 권장 |
| E2 Steam Workshop | ❌ | KSP는 craft/mission만 지원, 모드 미지원 |
| E3 CurseForge / E4 Nexus | ❌ | 신규 프로젝트 생성 API 없음 |
| E5 Thunderstore | ❌ | 기술적 최적해였으나 KSP 커뮤니티 0개 (303개 전수 확인) |
| F2 Google Forms | ❌ | 파일 업로드 질문은 Google 로그인 강제, 해제 불가 |
| F4 Telegram | ❌ | 전권 토큰 임베드 → 타인 제출물 열람 가능 |
| F1 Discord 웹훅 | 알림용만 | 첨부 URL 서명 만료로 호스팅 불가. 웹훅 URL을 Worker 서버 측에 두면 PR 알림 채널로 최적 |

## 결정적 발견

1. **로테이션은 조직 정책의 산물**: 무기한 FG PAT 정식 허용. 봇 계정을 없애고 운영자 본인 = 레포 소유자로 만들면 "타인 레포 협업자 미지원" 제약 자체가 소멸.
2. **CI 게이트 안전 확인**: `GITHUB_TOKEN`이 아닌 PAT/App 토큰으로 만든 PR은 워크플로가 승인 대기 없이 자동 실행됨(공식 문서 확인). same-repo 브랜치 PR이라 첫 기여자 승인 문제도 없음.
3. **무료 티어 안정성**: 2년간 무료 서비스 9개가 종료/축소된 반면, Cloudflare Workers 무료 한도 축소 이력 0, GitHub public Actions는 2026 가격 개편에서도 무제한 무료 유지 — 현행안의 두 축이 시장에서 가장 안정적.
4. **한국 접근성 실측**: jsDelivr 21ms, GitHub 40ms, workers.dev 25ms (전부 양호) vs 0x0 연결 실패, Codeberg 259ms, SpaceDock 306ms.
5. **클라이언트 키 임베드 절대 금지의 본질**: 유출보다 "키 교체 = 새 바이너리 배포 = 구버전 전멸"이 문제. 서버 측 토큰은 무중단 교체 가능.

## A′ 아키텍처 (확정 반영)

```
Tauri 앱 ──[PoW + 메타 + base64 zip]──▶ CF Worker (무파싱, Secret: 본인 무기한 FG PAT)
  ──▶ 운영자 개인 public 레포 kerbaloc-db: 브랜치 push → PR
  ──▶ Actions 14단계 검증 (public = 무제한 무료) → 운영자 리뷰 → merge
  ──▶ jsDelivr @commitSha (21ms) / Releases zip 폴백 — 다운로드 무계정
```

- PAT 스코프: Contents RW + Pull requests RW + Metadata R, 레포 1개 한정, 만료 없음
- 자동 안전장치: 1년 미사용 시 GitHub 자동 폐기, public 레포 유출 시 자동 폐기, Administration 권한 없어 레포 삭제 불가
- 부록 C §5.1(로테이션 절차)·2슬롯 로직 삭제 가능. 커밋은 운영자 명의(원 제출자 닉네임은 PR 본문에 기록)

## 신규 리스크 등록

- **jsDelivr GitHub 패키지 상한 50MB**: 팩 누적 시 태그 단위 서빙 제한 가능 → 팩별 개별 릴리스/태그 분할을 publish 설계에 반영 (파일당 20MB는 여유)
- `*.workers.dev` 한국 차단 가능성: 현재 근거 없음(실측 25ms). 발생해도 업로드만 불가, 다운로드 무영향(분리 불변식)

## 선택 항목 (저비용 보강)

- Internet Archive 연 1회 스냅샷 (영속성 최상, 155ms) — 버스 팩터 방어
- Discord 웹훅 PR 알림 (URL은 Worker 서버 측 보관)
- SpaceDock 대표 모드 1개 자동 갱신 (운영자 합의 후)
