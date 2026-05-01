# Resolve Shortcut Branch Merge With Dev

## Business Goal
`feat/alert-shortcut` 브랜치에 `origin/dev`의 최신 검색 및 알림 변경을 안전하게 흡수해, 새로 추가한 보드 단축키 기능과 `dev`의 신규 동작이 동시에 유지되도록 한다.

## Scope
- **In Scope**: `origin/dev` 머지, 충돌 해소, 단축키/알림/검색 관련 UI와 테스트 조정, 관련 검증 실행
- **Out of Scope**: 새 기능 추가, 단축키 사양 변경, 머지와 무관한 리팩터링

## Codebase Analysis Summary
현재 브랜치는 보드 단축키와 드롭다운 키보드 제어를 추가했고, `origin/dev`는 빠른 검색 개선과 백그라운드 동기화 알림 흐름을 변경했다. 겹치는 파일은 `Board`, `NotificationCenterButton`, `TaskQuickSearchDialog`, 메시지 번역, 그리고 해당 테스트들이다. 충돌 해소 시 우리 브랜치의 `BoardCommandProvider` 기반 단축키 구조와 `origin/dev`의 검색/알림 로직을 함께 보존해야 한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Board.tsx` | 보드 단축키와 알림 버튼 렌더링 | Modify |
| `src/components/NotificationCenterButton.tsx` | 알림 드롭다운 UI/동작 | Modify |
| `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | 전역 태스크 검색과 branch TODO 생성 | Modify |
| `messages/en.json` | 영문 UI 문구 | Modify |
| `messages/ko.json` | 한국어 UI 문구 | Modify |
| `messages/zh.json` | 중국어 UI 문구 | Modify |
| `src/components/__tests__/Board.test.tsx` | 보드 UI 회귀 테스트 | Modify |
| `src/components/__tests__/NotificationCenterButton.test.tsx` | 알림 드롭다운 테스트 | Modify |
| `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` | 빠른 검색/단축키 테스트 | Modify |
| `src/desktop/renderer/components/BoardCommandProvider.tsx` | 보드 명령 허브 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 단축키 처리 일관성 | `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | 입력 포커스 상태를 존중하고 필요한 경우에만 `preventDefault`를 사용한다 |
| 드롭다운 제어 패턴 | `src/components/ProjectSelector.tsx`, `src/components/NotificationCenterButton.tsx` | imperative ref와 내부 highlighted index를 일관된 방식으로 유지한다 |
| 테스트 스타일 | 기존 `__tests__` 파일들 | Testing Library 기반 사용자 상호작용 중심 검증을 유지한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 머지 기준 브랜치 | `origin/dev` | 로컬 `dev`가 stale 상태라 원격 최신 기준으로 맞춰야 한다 | 로컬 `dev` |
| 충돌 해소 방식 | 기존 기능 보존 후 최소 수정 | 회귀 위험이 높은 영역이라 구조 변경보다 의도 결합이 우선이다 | 리팩터링 동반 통합 |
| 검증 범위 | 관련 타깃 테스트 + `pnpm check` | 충돌 영역이 프론트엔드/데스크톱 경계까지 걸쳐 있어 타입/정적 검증이 필요하다 | 단일 테스트만 실행 |

## Implementation Todos

### Todo 1: 원격 `dev` 머지와 충돌 범위 고정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 실제 충돌 파일과 변경 의도를 확정한다
- **Work**:
  - `origin/dev`를 현재 브랜치에 머지한다
  - `git status`, 충돌 마커 검색, 양 브랜치 diff로 실제 충돌 파일을 정리한다
  - 충돌 파일별로 `ours`와 `theirs`의 신규 변경사항을 구분한다
- **Convention Notes**: 충돌 전까지 작업 트리를 깨끗하게 유지하고, 분석 결과는 플랜에 반영한다
- **Verification**: `git status --short`, 충돌 마커 검사
- **Exit Criteria**: 충돌 파일 목록과 해소 대상 기능이 명확해진다
- **Status**: completed

### Todo 2: 충돌 해소와 기능 통합
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 단축키/알림/검색 기능이 모두 유지되도록 충돌 파일을 정리한다
- **Work**:
  - `Board`, `NotificationCenterButton`, `TaskQuickSearchDialog`, 메시지/테스트 파일 충돌을 해소한다
  - `BoardCommandProvider` 경로를 유지하면서 `origin/dev`의 알림/검색 흐름을 반영한다
  - 필요한 경우 테스트 기대값과 fixture를 현재 동작에 맞춘다
- **Convention Notes**: 기존 public prop/handler 계약을 불필요하게 바꾸지 않고 최소한의 통합만 수행한다
- **Verification**: 충돌 마커 제거, 관련 테스트 파일 정적 검토
- **Exit Criteria**: 머지 충돌이 모두 제거되고 코드가 일관된 상태가 된다
- **Status**: completed

### Todo 3: 검증과 머지 마감
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 머지 결과가 회귀 없이 동작함을 검증한다
- **Work**:
  - 관련 Vitest 스위트 실행
  - `pnpm check` 실행
  - 플랜 진행 상태와 변경 로그를 갱신한다
- **Convention Notes**: 실패 시 원인 지점을 좁혀 최소 수정으로 재검증한다
- **Verification**: 테스트 명령과 `pnpm check`
- **Exit Criteria**: 필요한 검증 명령이 모두 통과한다
- **Status**: completed

## Verification Strategy
충돌 파일과 인접 영역의 회귀를 먼저 타깃 테스트로 확인한 뒤, 최종적으로 정적 검증까지 실행한다.
- `pnpm exec vitest run src/components/__tests__/Board.test.tsx src/components/__tests__/NotificationCenterButton.test.tsx src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx`
- 필요 시 `src/desktop/renderer/components/__tests__/BoardCommandProvider.test.tsx`와 `src/components/__tests__/ProjectSelector.test.tsx` 추가 실행
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-05-01: Plan created
- 2026-05-01: Todo 1 completed — `origin/dev` 머지 후 실제 충돌 파일과 겹치는 신규 변경사항을 확정
- 2026-05-01: Todo 2 completed — `Board`, 알림 테스트, 빠른 검색 테스트 충돌을 두 브랜치 기능이 모두 유지되도록 통합
- 2026-05-01: Todo 3 completed — 관련 Vitest 스위트 29개와 `pnpm check`를 통과시켜 머지 결과를 검증
