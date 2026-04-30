# 칸반 보드용 백그라운드 worktree/PR 동기화

## Business Goal
등록된 프로젝트의 기본 브랜치 기준 worktree와 PR 변화를 백그라운드에서 자동 반영해, 사용자가 수동 스캔이나 상세 페이지 진입 없이도 새 worktree task와 PR 상태를 즉시 칸반 보드에서 관리할 수 있게 한다.

## Scope
- **In Scope**: 등록 프로젝트 worktree 주기 동기화, 새 worktree task 자동 등록/재연결, PR URL 자동 저장, merge 감지 시 Done 이동 확인 알럿, main process 비동기 스케줄러 연결, 관련 단위 테스트 추가
- **Out of Scope**: GitHub webhook 연동, GitHub 외 PR provider 지원, PR 리뷰 상태/체크 상태 시각화, 사용자 설정 UI 추가

## Codebase Analysis Summary
현재 `projectService.scanAndRegisterProjects`에는 worktree를 태스크로 등록하는 로직이 이미 있지만 수동 스캔 경로에만 묶여 있다. `kanbanService.fetchAndSavePrUrl`는 단일 task의 PR URL만 조회하며 merge 상태는 다루지 않는다. Electron 렌더러는 `boardNotifier` 이벤트를 통해 `board-updated`와 hooks 실패 알림을 받고, 보드 컴포넌트는 Done 이동 확인 모달과 전역 refresh 흐름을 이미 갖추고 있다. 따라서 기존 worktree 등록/PR 조회 로직을 재사용 가능한 helper로 추출하고, main process에서 주기적으로 호출한 뒤 board event로 렌더러에 결과를 전달하는 구성이 가장 자연스럽다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/desktop/main/services/projectService.ts` | 프로젝트 root task 복구 및 worktree task 등록 | Modify |
| `src/desktop/main/services/kanbanService.ts` | task PR URL 조회 및 task 상태 변경 | Modify |
| `src/desktop/main/services/backgroundTaskSyncService.ts` | 백그라운드 주기 동기화 스케줄러 | Create |
| `src/lib/boardNotifier.ts` | 렌더러로 전달하는 board event 타입 | Modify |
| `electron/main.js` | 앱 시작 시 background service 연결 | Modify |
| `src/components/Board.tsx` | merge 감지 알럿/확인 후 Done 이동 UX | Modify |
| `messages/ko.json` | 보드 merge 확인 문구 | Modify |
| `messages/en.json` | 보드 merge 확인 문구 | Modify |
| `messages/zh.json` | 보드 merge 확인 문구 | Modify |
| `src/desktop/main/services/__tests__/projectService.test.ts` | worktree sync 회귀 테스트 | Modify |
| `src/desktop/main/services/__tests__/kanbanService.test.ts` | PR sync/merge 감지 테스트 | Modify |
| `src/components/__tests__/Board.test.tsx` | merge 알럿 렌더링/확인 동작 테스트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Board event 브로드캐스트 | `src/lib/boardNotifier.ts` | 메인 프로세스에서 payload type을 명시하고 공용 board event 채널로 전달한다 |
| 백그라운드 후속 작업 | `src/desktop/main/services/projectService.ts`, `src/desktop/main/services/kanbanService.ts` | 렌더러 응답을 막지 않도록 `setTimeout` 기반 비동기 스케줄 또는 별도 async loop를 사용한다 |
| 보드 상태 갱신 | `src/desktop/renderer/App.tsx`, `src/components/Board.tsx` | DB/메타데이터 변경은 `board-updated`를 통해 refresh 시그널로 반영한다 |
| 테스트 스타일 | `src/desktop/main/services/__tests__/*`, `src/components/__tests__/Board.test.tsx` | `vi` mock과 Given/When/Then 흐름을 유지하고, 비동기 타이머는 fake timer로 검증한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 백그라운드 실행 위치 | Electron main process 전용 scheduler | 렌더러 성능 경로와 완전히 분리되고 remote/local Git 탐색을 공용 IPC 없이 수행할 수 있다 | Board/renderer polling |
| worktree 동기화 재사용 경로 | `projectService` 내부 로직을 helper로 추출 | 기존 manual scan과 background sync가 같은 등록 규칙을 공유한다 | background service에 중복 구현 |
| PR 상태 조회 | `gh pr list --state all --json ...` 기반 메타데이터 조회 | 기존 gh 의존성을 유지하면서 open/merged 상태를 함께 판단할 수 있다 | GitHub REST API 직접 호출 |
| merge 알럿 중복 억제 | 앱 세션 메모리 키(`taskId + prUrl + mergedAt`) | 불필요한 DB migration 없이 반복 알럿을 막을 수 있다 | task 테이블에 영속 플래그 추가 |
| 보드 UX | Board 컴포넌트에서 board event를 구독해 확인 모달 표시 | Done 이동 액션과 로컬 보드 상태/refresh 흐름을 같은 컴포넌트에 유지한다 | App 전역 알럿에서 직접 task update |

## Implementation Todos

### Todo 1: worktree/PR background sync 회귀 테스트 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 새 helper와 merge 알럿 UX가 필요한 동작을 테스트로 고정한다
- **Work**:
  - `src/desktop/main/services/__tests__/projectService.test.ts`에 등록 프로젝트 worktree 동기화가 새 task를 만들고 기존 task를 재연결하는 테스트 추가
  - `src/desktop/main/services/__tests__/kanbanService.test.ts`에 PR URL 저장과 merge event broadcast 테스트 추가
  - `src/components/__tests__/Board.test.tsx`에 merge event 수신 시 확인 모달 표시 및 Done 이동 action 호출 테스트 추가
- **Convention Notes**: 기존 mock 구조를 재사용하고, background timer/board event는 fake timer와 수동 listener 호출로 검증한다
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/projectService.test.ts src/desktop/main/services/__tests__/kanbanService.test.ts src/components/__tests__/Board.test.tsx`
- **Exit Criteria**: 새 테스트가 현재 구현 기준으로 실패한다
- **Status**: completed

### Todo 2: reusable worktree sync helper와 background scheduler 구현
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 등록 프로젝트 전체에 대해 주기적인 worktree 탐색을 main process에서 비동기로 수행한다
- **Work**:
  - `src/desktop/main/services/projectService.ts`에서 project 단위 worktree/task 동기화 helper를 추출하고 export한다
  - `src/desktop/main/services/backgroundTaskSyncService.ts`를 생성해 initial delay, fixed interval, overlap guard, stop handler를 구현한다
  - `electron/main.js`에서 app ready 시 scheduler를 시작하고 before-quit에 정리한다
- **Convention Notes**: helper는 summary를 반환하고, background scheduler는 렌더링과 무관하게 async로만 동작해야 한다
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/projectService.test.ts`
- **Exit Criteria**: 등록 프로젝트 worktree 동기화가 background service에서 호출 가능하고, 변경 시 board refresh를 유도한다
- **Status**: completed

### Todo 3: PR 상태 동기화와 merge board event 구현
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 새 PR 생성은 자동 링크 저장, merge된 PR은 board event로 확인 알럿을 띄울 수 있게 만든다
- **Work**:
  - `src/desktop/main/services/kanbanService.ts`에 PR metadata 조회 helper와 active task PR sync 함수를 추가한다
  - `src/lib/boardNotifier.ts`에 PR merge 감지 이벤트 타입/payload를 추가한다
  - background scheduler에서 worktree sync 이후 active task PR sync를 호출하고, 변경/merge를 반영한다
- **Convention Notes**: gh CLI가 없거나 원격 연결이 불안정한 경우는 기존 PR 조회처럼 조용히 skip한다
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts`
- **Exit Criteria**: open PR 생성 시 `prUrl`이 저장되고, merged PR은 중복 없이 board event가 발행된다
- **Status**: completed

### Todo 4: Board merge 확인 UX와 다국어 메시지 추가
- **Priority**: 3
- **Dependencies**: Todo 2, Todo 3
- **Goal**: 칸반 페이지에서 merge 감지 시 사용자가 Done 이동 여부를 바로 결정할 수 있게 한다
- **Work**:
  - `src/components/Board.tsx`에서 PR merge board event를 구독하고 확인 모달/알럿 state를 추가한다
  - 확인 시 `updateTaskStatus(taskId, done)`을 호출하고, 취소 시 현재 앱 세션에서 재노출을 막는다
  - `messages/ko.json`, `messages/en.json`, `messages/zh.json`에 merge 알럿 문구를 추가한다
- **Convention Notes**: 기존 DoneConfirmDialog 패턴을 참고하되, PR URL이 있으면 링크를 열 수 있는 가벼운 모달로 유지한다
- **Verification**: `pnpm test -- src/components/__tests__/Board.test.tsx`
- **Exit Criteria**: merge event 수신 시 보드에서 확인 UI가 보이고, 확인 후 Done 상태 변경 action이 호출된다
- **Status**: completed

## Verification Strategy
서비스 단위 테스트로 worktree/PR 동기화 로직을 검증하고, 보드 컴포넌트 테스트로 merge 알럿 UX를 확인한다. 이후 타입체크로 이벤트 payload와 번역 키 추가가 전체 앱에 안전한지 확인한다.
- `pnpm test -- src/desktop/main/services/__tests__/projectService.test.ts src/desktop/main/services/__tests__/kanbanService.test.ts src/components/__tests__/Board.test.tsx`
- `pnpm check`

## Progress Tracking
- Total Todos: 4
- Completed: 4
- Status: Execution complete

## Change Log
- 2026-04-30: Plan created
- 2026-04-30: Todo 1 completed — worktree sync, PR sync, Board merge alert 회귀 테스트 추가
- 2026-04-30: Todo 2 completed — 등록 프로젝트 worktree helper와 background scheduler 구현
- 2026-04-30: Todo 3 completed — active task PR 메타데이터 동기화와 merge board event 추가
- 2026-04-30: Todo 4 completed — Board merge 확인 모달과 다국어 문구 반영
