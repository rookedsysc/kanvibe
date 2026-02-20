# Session-per-Branch: tmux/zellij 세션 기반 전환

## Business Goal
현재 프로젝트 단위로 하나의 tmux 세션(또는 zellij 세션)을 공유하고 브랜치별 window/tab을 생성하는 방식을, 각 브랜치마다 독립적인 세션을 생성하는 방식으로 전환한다. 이를 통해 브랜치 간 전환이 `tmux switch-client` / `zellij attach`로 단순해지고, 세션 격리로 안정성이 향상된다.

## Scope
- **In Scope**: tmux/zellij 모두 세션 기반으로 전환, 세션 이름 형식 변경(`projectName/branchName`), zellij 세션 이름 길이 안전장치
- **Out of Scope**: DB 마이그레이션 (기존 task의 session_name 업데이트), 기존 tmux 세션 정리 스크립트, PTY 공유 방식 변경

## Codebase Analysis Summary
현재 시스템은 프로젝트 이름으로 하나의 공유 세션을 생성하고, 각 브랜치를 tmux window / zellij tab으로 추가하는 구조이다. `sessionName = path.basename(projectPath)`로 프로젝트 단위, `windowName = formatWindowName(branchName)`로 브랜치를 식별한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/worktree.ts` | worktree + tmux/zellij 세션 생성/삭제 핵심 로직 | Modify |
| `src/lib/terminal.ts` | PTY 생성 및 tmux/zellij attach 로직 | Modify |
| `src/app/actions/project.ts` | 프로젝트 스캔 시 세션 감지 및 메인 브랜치 태스크 생성 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 주석 언어 | CODE_PRINCIPLES.md | 한국어로 작성 |
| 함수 주석 | 기존 코드 | JSDoc 스타일, `/** ... */` |
| 세션 명령어 패턴 | worktree.ts | `execGit()` 래퍼로 실행, sshHost 분기 지원 |
| 에러 처리 | 기존 코드 | try-catch로 감싸고 실패 시 무시 또는 에러 로깅 |
| 네이밍 | CODE_PRINCIPLES.md | 동사 기반, 비즈니스 역할 명시 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 세션 이름 형식 | `projectName/branchName` (sanitized) | `tmux ls` 시 프로젝트별 그룹핑, uniqueness 보장 | branchName만 (프로젝트 간 충돌), projectName_branchName (가독성 낮음) |
| tmux 세션 이름 제한 | 검증 불필요 | 메인테이너 "no defined limit" 확인 | truncation 추가 |
| zellij 세션 이름 제한 | 소켓 경로 108바이트 기준 truncation | macOS 기준 ~62자 제한 | 무제한 (macOS에서 오류 발생) |
| focusSession tmux 동작 | no-op | 세션 기반에서는 각 PTY가 독립 세션이므로 window 전환 불필요 | switch-client (PTY에서는 의미 없음) |
| formatWindowName 유지 | 제거하고 formatSessionName으로 대체 | window 개념이 사라지므로 혼란 방지 | 이름만 변경 |

## Implementation Todos

### Todo 1: formatSessionName 헬퍼 함수 추가 및 formatWindowName 교체
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 세션 이름 생성 로직을 `projectName/branchName` 형식으로 변경하고, zellij용 길이 안전장치를 추가한다
- **Work**:
  - `src/lib/worktree.ts`의 `formatWindowName` 함수를 `formatSessionName(projectName: string, branchName: string): string`으로 교체
  - 형식: `${projectName}/${branchName.replace(/\//g, "-")}` (leading space 제거)
  - `sanitizeZellijSessionName(sessionName: string): string` 헬퍼 추가: 소켓 경로 길이 기반 truncation (최대 60자로 안전하게 제한)
  - `formatWindowName` 참조를 모두 `formatSessionName`으로 변경
  - `WorktreeSession` 인터페이스는 유지 (sessionName 필드 의미만 변경)
- **Convention Notes**: 함수명은 동사 기반이 아닌 formatter이므로 `format` 접두사 유지. JSDoc 주석 한국어.
- **Verification**: TypeScript 컴파일 성공 (`npx tsc --noEmit`)
- **Exit Criteria**: `formatSessionName` 함수가 존재하고, `formatWindowName` 참조가 0개
- **Status**: pending

### Todo 2: createWorktreeWithSession 세션 기반 전환
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: worktree 생성 시 공유 세션의 window 대신 독립 세션을 생성하도록 변경한다
- **Work**:
  - `src/lib/worktree.ts`의 `createWorktreeWithSession()` 수정
  - tmux 경로:
    - `sessionName = formatSessionName(projectName, branchName)` (기존: `projectName`)
    - `tmux has-session` 체크 → 있으면 재사용, 없으면 `tmux new-session -d -s "sessionName" -c "worktreePath"`
    - `tmux new-window` 호출 제거
    - `isWindowAlive` → `isSessionAlive`로 교체 (Todo 4에서 구현, 여기서는 `tmux has-session` 직접 사용)
  - zellij 경로:
    - `sessionName = sanitizeZellijSessionName(formatSessionName(projectName, branchName))`
    - `zellij --session "sessionName" --cwd "worktreePath" &` 로 독립 세션 생성
    - `zellij action new-tab` 호출 제거
  - pane 레이아웃: `applyPaneLayoutAsync`의 `windowName` 대신 세션의 기본 window(첫 번째 window) 사용
- **Convention Notes**: `execGit()` 래퍼 유지, sshHost 분기 유지
- **Verification**: TypeScript 컴파일 성공
- **Exit Criteria**: `tmux new-window` / `zellij action new-tab` 호출이 이 함수에서 제거됨
- **Status**: pending

### Todo 3: createSessionWithoutWorktree 세션 기반 전환
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: worktree 없이 세션을 생성하는 함수도 독립 세션 방식으로 변경한다
- **Work**:
  - `src/lib/worktree.ts`의 `createSessionWithoutWorktree()` 수정
  - tmux: `tmux new-session -d -s "sessionName" -c "cwd"` (has-session 체크 후)
  - zellij: `zellij --session "sessionName" --cwd "cwd" &`
  - window/tab 생성 코드 제거
  - `sessionName` 생성에 `formatSessionName` 사용
- **Convention Notes**: 기존 함수 시그니처 유지 (하위 호환)
- **Verification**: TypeScript 컴파일 성공
- **Exit Criteria**: window/tab 생성 코드가 제거되고 독립 세션 생성으로 대체됨
- **Status**: pending

### Todo 4: removeSessionOnly 및 유틸리티 함수 전환
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 세션 삭제, 생존 확인, 목록 조회 함수를 세션 기반으로 변경한다
- **Work**:
  - `removeSessionOnly()`: tmux `kill-window` → `kill-session`, zellij `close-tab` → `kill-session`
  - `isWindowAlive()` → `isSessionAlive()`로 rename 및 로직 변경: tmux `has-session -t`, zellij `list-sessions | grep`
  - `listActiveWindows()` → `listActiveSessions()`로 rename: tmux `list-sessions -F '#{session_name}'`, zellij `list-sessions`
  - `applyPaneLayout()`: target을 `"sessionName":"windowName"` → `"sessionName"` (기본 window 사용)
- **Convention Notes**: 함수 rename 시 참조하는 모든 곳 업데이트. `find_referencing_symbols`로 확인.
- **Verification**: TypeScript 컴파일 성공, `isWindowAlive` / `listActiveWindows` 참조 0개
- **Exit Criteria**: window 기반 유틸리티가 모두 session 기반으로 전환됨
- **Status**: pending

### Todo 5: terminal.ts attachLocalSession 세션 기반 전환
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 4
- **Goal**: 로컬 터미널 연결 시 세션에 직접 attach하도록 변경한다
- **Work**:
  - `src/lib/terminal.ts`의 `attachLocalSession()` 수정
  - `isTmuxWindowAlive()` → `isTmuxSessionAlive(sessionName)`: `tmux has-session -t "sessionName"` 사용
  - 자동 생성 로직: `tmux new-session -d -s "sessionName" -c "dir"` (window 생성 제거)
  - attach args: `["attach-session", "-t", sessionName]` (windowIndex 불필요)
  - `getTmuxWindowIndex()` 함수 제거 (더 이상 사용 안 함)
  - zellij: `["attach", sessionName]` (go-to-tab-name 호출 제거)
- **Convention Notes**: `activeTerminals` Map의 `TerminalEntry`에서 `windowName` 필드는 호환성을 위해 유지하되 tmux에서는 사용하지 않음
- **Verification**: TypeScript 컴파일 성공
- **Exit Criteria**: `tmux attach-session -t session:windowIndex` 패턴이 `tmux attach-session -t sessionName`으로 변경됨
- **Status**: pending

### Todo 6: terminal.ts focusSession 및 attachRemoteSession 전환
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 4
- **Goal**: 포커스 전환 및 원격 연결도 세션 기반으로 변경한다
- **Work**:
  - `focusSession()`: tmux 분기를 no-op으로 변경 (세션 기반에서는 window 전환이 불필요). zellij도 독립 세션이므로 no-op.
  - `attachRemoteSession()`: tmux `attach-session -t "sessionName"` (`:windowName` 제거), zellij `attach "sessionName"` (go-to-tab-name 제거)
- **Convention Notes**: no-op 처리 시 주석으로 사유 명시
- **Verification**: TypeScript 컴파일 성공
- **Exit Criteria**: `select-window` / `go-to-tab-name` 호출이 제거됨
- **Status**: pending

### Todo 7: project.ts scanAndRegisterProjects 세션 감지 로직 변경
- **Priority**: 4
- **Dependencies**: Todo 1, Todo 4
- **Goal**: 프로젝트 스캔 시 세션 감지 로직을 새 세션 이름 형식에 맞게 변경한다
- **Work**:
  - `src/app/actions/project.ts`의 `scanAndRegisterProjects()` 수정
  - worktree 스캔 블록: `sessionName = formatSessionName(projectName, branchName)`, `isSessionAlive()` 사용
  - 메인 브랜치 세션 감지 블록: 동일하게 `formatSessionName` + `isSessionAlive` 사용
  - `formatWindowName` import 제거, `formatSessionName` import 추가
- **Convention Notes**: `isWindowAlive` → `isSessionAlive` rename에 맞춤
- **Verification**: TypeScript 컴파일 성공
- **Exit Criteria**: `scanAndRegisterProjects`에서 `formatWindowName` / `isWindowAlive` 참조 0개
- **Status**: pending

### Todo 8: 최종 빌드 검증 및 정리
- **Priority**: 5
- **Dependencies**: Todo 2, Todo 3, Todo 4, Todo 5, Todo 6, Todo 7
- **Goal**: 전체 빌드 성공 확인 및 미사용 코드 정리
- **Work**:
  - `npx tsc --noEmit`으로 타입 검증
  - `formatWindowName`, `isTmuxWindowAlive`, `getTmuxWindowIndex`, `isWindowAlive`, `listActiveWindows` 등 미사용 함수/import 완전 제거 확인
  - 기존 테스트 파일(`src/app/api/hooks/start/__tests__/route.test.ts`)에서 sessionName 관련 테스트 데이터가 있으면 새 형식에 맞게 업데이트
- **Convention Notes**: 미사용 코드는 주석 처리 없이 완전 삭제
- **Verification**: `pnpm build` 또는 `npx tsc --noEmit` 성공
- **Exit Criteria**: 빌드 성공, window 기반 코드 잔여물 0개
- **Status**: pending

## Verification Strategy
- TypeScript 컴파일: `npx tsc --noEmit`
- 기존 테스트: `pnpm test` (있는 경우)
- grep 검증: `grep -r "new-window\|kill-window\|list-windows\|select-window\|formatWindowName\|isWindowAlive\|listActiveWindows" src/lib/ src/app/` → 0 결과

## Progress Tracking
- Total Todos: 8
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-20: Plan created
