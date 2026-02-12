# Session to Window/Tab 전환 (workmux 스타일)

## Business Goal
현재 task마다 별도 tmux 세션/zellij 세션을 생성하는 방식을 workmux처럼 하나의 메인 세션(프로젝트명) 안에서 tmux window / zellij tab으로 관리하도록 전환한다. 이를 통해 터미널 multiplexer의 세션 관리가 더 깔끔해지고, 하나의 세션 내에서 모든 작업 브랜치를 window/tab으로 전환할 수 있다.

## Scope
- **In Scope**: worktree.ts 세션→window/tab 전환, terminal.ts 연결 방식 변경, server.ts 파라미터 전달
- **Out of Scope**: DB 스키마 변경, UI 컴포넌트 변경, 기존 task 데이터 마이그레이션

## Codebase Analysis Summary
- `src/lib/worktree.ts`: `createWorktreeWithSession()`, `removeWorktreeAndSession()`, `listActiveSessions()` — 핵심 변경 대상
- `src/lib/terminal.ts`: `attachLocalSession()`, `attachRemoteSession()` — window/tab 타겟팅으로 변경
- `server.ts`: terminal 함수 호출부 — branchName 파라미터 추가 전달
- `src/entities/KanbanTask.ts`: `sessionName` 필드 의미 변경 (세션명 → 메인 세션명). 스키마 변경 불필요
- `src/app/actions/kanban.ts`: `createTask()`, `deleteTask()`, `branchFromTask()` — worktree 함수 호출부
- `src/app/api/hooks/start/route.ts`: hook API — worktree 함수 호출부

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/worktree.ts` | worktree + 세션 생성/삭제 | Modify |
| `src/lib/terminal.ts` | 터미널 WebSocket 연결 | Modify |
| `server.ts` | WebSocket 핸들러 (terminal 함수 호출) | Modify |
| `src/app/actions/kanban.ts` | task CRUD (worktree 함수 호출) | Reference (변경 불필요) |
| `src/app/api/hooks/start/route.ts` | Hook API (worktree 함수 호출) | Reference (변경 불필요) |
| `src/entities/KanbanTask.ts` | task 엔티티 | Reference (변경 불필요) |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 모든 주석/JSDoc은 한국어 |
| execGit 활용 | worktree.ts | 로컬/SSH 명령은 execGit()을 통해 실행 |
| SessionType enum | KanbanTask.ts | TMUX / ZELLIJ enum 값 사용 |
| 함수명 네이밍 | CODE_PRINCIPLES.md | 동사 기반 명확한 이름 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 메인 세션 이름 | project.name | workmux 컨벤션, 직관적 | 별도 필드 추가 |
| Window/tab 이름 | branchName sanitized | 기존 sanitize 로직 재활용 | 별도 naming |
| tmux attach | `attach -t session:window` | 특정 window 직접 타겟팅 | 전체 세션 attach |
| zellij tab 생성 | `zellij action --session new-tab` | 외부에서 tab 생성 가능 | layout 파일 |
| zellij tab 삭제 | `go-to-tab-name` → `close-tab` | 이름으로 tab 특정 | index 기반 |
| sessionName 의미 | 메인 세션명 저장 | 스키마 변경 불필요 | 새 필드 추가 |

## Implementation Todos

### Todo 1: worktree.ts — 세션 생성을 window/tab 생성으로 전환
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `createWorktreeWithSession()`에서 tmux new-session → new-window, zellij session → tab으로 변경. 메인 세션 없으면 자동 생성.
- **Work**:
  - `createWorktreeWithSession()` 수정:
    - tmux: `tmux has-session -t "projectName" || tmux new-session -d -s "projectName"` → `tmux new-window -t "projectName" -n "windowName" -c "worktreePath"`
    - zellij: `zellij list-sessions | grep -q "projectName" || zellij --session "projectName" options --detach-on-exit false &` 후 `zellij action --session "projectName" new-tab --name "windowName" --cwd "worktreePath"`
    - sessionName 반환값을 메인 세션명(projectName)으로 변경
  - `removeWorktreeAndSession()` 수정:
    - tmux: `tmux kill-session -t "sessionName"` → `tmux kill-window -t "sessionName:windowName"` (sessionName=메인세션, windowName=branchName sanitized)
    - zellij: `zellij delete-session` → `zellij action --session "sessionName" go-to-tab-name "windowName"` + `zellij action --session "sessionName" close-tab`
    - 함수 시그니처에 windowName(또는 branchName에서 파생) 필요 — 이미 branchName 파라미터 존재
  - `listActiveSessions()` → `listActiveWindows()`로 리네임 및 변경:
    - tmux: `tmux list-windows -t "mainSession" -F "#{window_name}"`
    - zellij: 기존 session list 유지 (tab 리스트는 별도 API 없음)
- **Convention Notes**: execGit() 사용, 한국어 JSDoc
- **Verification**: TypeScript 컴파일 (`npx tsc --noEmit`)
- **Exit Criteria**: worktree.ts가 window/tab 기반으로 동작하며 컴파일 에러 없음
- **Status**: pending

### Todo 2: terminal.ts — window/tab 타겟 연결로 변경
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `attachLocalSession()`, `attachRemoteSession()`이 특정 window/tab을 타겟으로 연결하도록 변경
- **Work**:
  - `attachLocalSession()` 시그니처에 `windowName: string` 파라미터 추가
    - tmux: args를 `["attach-session", "-t", `${sessionName}:${windowName}`]`로 변경
    - zellij: attach 전에 `zellij action --session "${sessionName}" go-to-tab-name "${windowName}"` 실행 (execLocal 사용)
  - `attachRemoteSession()` 시그니처에 `windowName: string` 파라미터 추가
    - tmux: command를 `tmux attach-session -t "${sessionName}:${windowName}"`로 변경
    - zellij: attach 전에 `zellij action --session "${sessionName}" go-to-tab-name "${windowName}"` 명령 전송
- **Convention Notes**: execGit() 또는 execLocal() 사용, 기존 PTY spawn 패턴 유지
- **Verification**: TypeScript 컴파일 (`npx tsc --noEmit`)
- **Exit Criteria**: terminal.ts가 window/tab 타겟으로 연결하며 컴파일 에러 없음
- **Status**: pending

### Todo 3: server.ts — branchName 전달
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: server.ts의 WebSocket 핸들러에서 terminal 함수에 windowName(branchName 파생) 전달
- **Work**:
  - `wss.on("connection")` 핸들러에서 task.branchName으로 windowName 계산
    - `const windowName = task.branchName?.replace(/\//g, "-") || ""`
  - `attachLocalSession()` 호출에 windowName 추가
  - `attachRemoteSession()` 호출에 windowName 추가
- **Convention Notes**: 기존 에러 처리 패턴 유지
- **Verification**: TypeScript 컴파일 (`npx tsc --noEmit`)
- **Exit Criteria**: server.ts가 windowName을 올바르게 전달하며 컴파일 에러 없음
- **Status**: pending

## Verification Strategy
- TypeScript 컴파일: `npx tsc --noEmit`
- 빌드: `npm run build` (Next.js 빌드)

## Progress Tracking
- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-12: Plan created
