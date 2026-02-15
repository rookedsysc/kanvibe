# Fix: Main Branch tmux Session Connection

## Business Goal
메인 브랜치(base branch)에서 터미널 연결 시 "can't find window: main" 에러가 발생하는 버그를 수정한다. 워크트리가 아닌 base project에서도 tmux 세션에 정상 연결되도록 한다.

## Scope
- **In Scope**: `isTmuxWindowAlive` trim 불일치 수정, `scanAndRegisterProjects` 메인 브랜치 자동 연결 쿼리 수정
- **Out of Scope**: formatWindowName의 leading space 디자인 변경, zellij 관련 수정, SSH 원격 연결

## Codebase Analysis Summary
터미널 연결 시 `server.ts` → `attachLocalSession` → `isTmuxWindowAlive` 순으로 호출된다. `formatWindowName`은 모든 브랜치명 앞에 공백을 추가한다 (` main`, ` feat-login` 등). 이 패턴은 tmux window 생성(`worktree.ts`)과 alive 체크(`worktree.ts`의 `isWindowAlive`)에서는 일관되게 동작하지만, `terminal.ts`의 `isTmuxWindowAlive`에서만 `.trim()` 사용으로 불일치가 발생한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/terminal.ts` | 터미널 세션 attach/detach, alive 체크 | Modify |
| `src/app/actions/project.ts` | 프로젝트 스캔, 메인 브랜치 자동 연결 | Modify |
| `src/lib/worktree.ts` | tmux window 생성, `isWindowAlive` 참조 구현 | Reference |
| `server.ts` | WebSocket 핸들러, windowName 파생 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어로 작성 |
| 기존 패턴 일관성 | worktree.ts `isWindowAlive` | `includes()` 방식으로 window 이름 매치 |

## Implementation Todos

### Todo 1: `isTmuxWindowAlive` trim 불일치 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: tmux window 존재 여부 체크가 leading space가 있는 windowName에서도 정상 동작하도록 수정
- **Work**:
  - `src/lib/terminal.ts` 23행의 `w.trim() === windowName`을 `w.includes(windowName)`으로 변경
  - `worktree.ts`의 `isWindowAlive`와 동일한 매칭 방식 적용
- **Convention Notes**: 한국어 주석 유지
- **Verification**: 코드 리뷰로 `isWindowAlive`와 동일한 로직 확인
- **Exit Criteria**: `isTmuxWindowAlive`가 leading space가 포함된 windowName을 정상 매치
- **Status**: pending

### Todo 2: 메인 브랜치 자동 연결 쿼리 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `scanAndRegisterProjects`에서 재스캔 시 메인 브랜치 태스크의 tmux 세션을 자동 감지하여 연결 정보를 설정
- **Work**:
  - `src/app/actions/project.ts` 260행의 `branchName: IsNull()`을 `branchName: project.defaultBranch`로 변경
  - 대응하는 `!mainBranchTask.sessionType` 조건은 유지 (이미 연결된 경우 스킵)
- **Convention Notes**: TypeORM findOneBy 패턴 유지
- **Verification**: 코드 리뷰로 `createDefaultBranchTask`의 branchName 값과 일치 확인
- **Exit Criteria**: 재스캔 시 메인 브랜치 태스크에 활성 tmux 세션이 자동 연결됨
- **Status**: pending

## Verification Strategy
- `npm run build`로 TypeScript 컴파일 에러 없음 확인
- 변경된 두 파일의 로직이 기존 패턴과 일관성 있는지 코드 리뷰

## Progress Tracking
- Total Todos: 2
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-15: Plan created
