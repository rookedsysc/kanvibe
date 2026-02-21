# Session Per Branch 리팩토링

## Business Goal

현재 프로젝트 단위의 단일 세션 내에서 브랜치별 window/tab을 관리하는 구조를 제거하고, 브랜치별 독립 세션 모델로 전환한다. 세션이 없으면 자동 생성하여 진입하며, 브랜치 이름의 `/`를 `-`로 치환하여 세션 이름으로 사용한다.

## Scope
- **In Scope**: terminal.ts, worktree.ts, server.ts에서 window/tab 관련 코드 제거 및 세션 기반으로 전환
- **Out of Scope**: DB 스키마 변경 (sessionName 컬럼은 varchar이므로 값만 변경됨), 프론트엔드 UI 변경

## Codebase Analysis Summary

현재 구조: 프로젝트명 기반 단일 세션 (`kanvibe`) 내에 브랜치별 window/tab (`feat-something`)을 생성. `formatWindowName`은 앞에 공백을 붙여 ` feat-something` 형태로 반환.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/terminal.ts` | PTY 기반 터미널 연결/포커스 관리 | Modify |
| `src/lib/worktree.ts` | worktree 생성/삭제 + 세션 관리 | Modify |
| `server.ts` | WebSocket 핸들러에서 windowName 파생 | Modify |
| `src/app/actions/project.ts` | 프로젝트 스캔 시 세션/window 생성 | Modify |
| `src/app/actions/kanban.ts` | connectTerminalSession | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 주석 한국어 | CODE_PRINCIPLES.md | 모든 주석은 한국어로 작성 |
| JSDoc 스타일 | 기존 코드 | `/** */` 형태 사용 |
| 에러 무시 패턴 | terminal.ts | catch 블록에 한국어 주석으로 사유 명시 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 세션 이름 형식 | `branchName.replace(/\//g, "-")` | zellij `/` 제한 + tmux/zellij 통일 | 프로젝트명+브랜치 조합 (불필요하게 긴 이름) |
| 세션 자동 생성 | tmux/zellij 모두 없으면 생성 | 현재 zellij는 세션 없으면 실패하는데, 통일 | 에러 반환 (UX 나쁨) |
| pane layout 타겟 | 세션의 기본 window (`:0`) | 독립 세션 생성 시 기본 window 존재 | 별도 window 생성 (불필요) |

## Implementation Todos

### Todo 1: worktree.ts에서 formatWindowName → formatSessionName 전환 및 window/tab 로직 제거
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 브랜치별 독립 세션 모델로 전환
- **Work**:
  - `formatWindowName` → `formatSessionName`으로 이름 변경, 앞 공백 제거: `branchName.replace(/\//g, "-")`
  - `WorktreeSession` 인터페이스 유지 (변경 없음)
  - `createWorktreeWithSession`: window 생성 대신 독립 세션 생성
    - tmux: `tmux new-session -d -s "{sessionName}" -c "{worktreePath}"`
    - zellij: `cd "{worktreePath}" && zellij --session "{sessionName}" &`
    - 반환값 `sessionName`을 `formatSessionName(branchName)`으로 변경
  - `createSessionWithoutWorktree`: 동일하게 독립 세션 생성
  - `isWindowAlive` → `isSessionAlive`로 변경: 세션 존재 여부만 확인
  - `listActiveWindows` 제거 또는 `listActiveSessions`로 변경
  - `removeSessionOnly`: window/tab kill 대신 세션 전체를 종료
    - tmux: `tmux kill-session -t "{sessionName}"`
    - zellij: `zellij delete-session "{sessionName}"`
    - `branchName` 파라미터로 `formatSessionName`을 호출하여 세션 이름 파생
  - `applyPaneLayout`: 타겟을 `"sessionName":0`으로 변경
  - `applyPaneLayoutAsync`: `windowName` 파라미터 제거, 세션의 기본 window 사용
- **Convention Notes**: 한국어 주석, `/** */` JSDoc 형태 유지
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: worktree.ts에서 window/tab 관련 코드가 모두 세션 기반으로 전환됨
- **Status**: pending

### Todo 2: terminal.ts에서 window 관련 함수 제거 및 세션 기반으로 전환
- **Priority**: 1
- **Dependencies**: none
- **Goal**: PTY 연결과 포커스를 세션 단위로 전환
- **Work**:
  - `TerminalEntry` 인터페이스에서 `windowName` 필드 제거
  - `getTmuxWindowIndex` 함수 제거
  - `isTmuxWindowAlive` 함수 제거
  - `isZellijSessionAlive` 함수는 유지 (세션 존재 확인에 여전히 필요)
  - `attachLocalSession`:
    - `windowName` 파라미터 제거
    - tmux window 자동 생성 로직 → 세션 자동 생성으로 변경: `tmux has-session -t "{sessionName}" || tmux new-session -d -s "{sessionName}" -c "{cwd}"`
    - zellij도 세션 없으면 자동 생성: `zellij --session "{sessionName}" &`
    - PTY spawn args: tmux → `["attach-session", "-t", sessionName]`, zellij → `["attach", sessionName]`
    - zellij의 `go-to-tab-name` 호출 제거
    - `TerminalEntry` 생성 시 `windowName` 제거
  - `attachRemoteSession`:
    - `windowName` 파라미터 제거
    - tmux command: `tmux attach-session -t "{sessionName}"`
    - zellij command: `zellij attach "{sessionName}"` (go-to-tab-name 제거)
  - `focusSession`:
    - tmux: `tmux switch-client -t "{sessionName}"` (select-window 대신)
    - zellij: `zellij attach "{sessionName}"` 또는 무시 (외부에서 focus 불가)
- **Convention Notes**: 한국어 주석 유지, catch 블록에 사유 주석
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: terminal.ts에서 window/tab 관련 코드가 모두 제거됨
- **Status**: pending

### Todo 3: server.ts와 action 파일에서 windowName 참조 제거
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 호출부에서 window 관련 인터페이스 제거
- **Work**:
  - `server.ts`:
    - `formatWindowName` import 제거
    - `windowName` 변수 파생 로직 제거
    - `attachLocalSession` 호출에서 `windowName` 인자 제거
    - `attachRemoteSession` 호출에서 `windowName` 인자 제거
  - `src/app/actions/project.ts`:
    - `formatWindowName` → `formatSessionName` import 변경
    - `windowName` 변수 사용부를 `sessionName`으로 변경
    - `isWindowAlive` → `isSessionAlive` 호출 변경
  - `src/app/actions/kanban.ts`:
    - `connectTerminalSession`에서 `sessionName`을 `formatSessionName(branchForWindow)`로 저장
- **Convention Notes**: 기존 코드 패턴 유지
- **Verification**: TypeScript 컴파일 에러 없음, `pnpm build` 성공
- **Exit Criteria**: 전체 코드베이스에서 `windowName`, `formatWindowName` 참조가 없음
- **Status**: pending

### Todo 4: 테스트 파일 업데이트
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 테스트가 새로운 세션 모델에 맞게 동작
- **Work**:
  - `src/app/actions/__tests__/` 내 테스트 파일에서 windowName 관련 참조 확인 및 수정
  - import 변경 (`formatWindowName` → `formatSessionName`)
- **Convention Notes**: Given-When-Then 패턴, 영어 테스트명
- **Verification**: `pnpm test` 통과
- **Exit Criteria**: 모든 테스트 통과
- **Status**: pending

## Verification Strategy
- TypeScript 컴파일: `pnpm build` 성공
- 테스트: `pnpm test` 통과
- grep 검증: `windowName`, `formatWindowName`, `getTmuxWindowIndex`, `isTmuxWindowAlive` 참조가 코드베이스에 없음

## Progress Tracking
- Total Todos: 4
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-21: Plan created
