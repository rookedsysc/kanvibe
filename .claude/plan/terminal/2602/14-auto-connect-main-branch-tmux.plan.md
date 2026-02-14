# 메인 브랜치 태스크 tmux 세션 자동 연결

## Business Goal
프로젝트 등록 시 메인 브랜치 태스크에 tmux 세션을 자동으로 연결하여, 워크트리가 아닌 메인 브랜치 태스크에서도 즉시 터미널을 사용할 수 있도록 한다.

## Scope
- **In Scope**: `createDefaultBranchTask()`에서 tmux 세션 자동 생성, `scanAndRegisterProjects()`에서 메인 브랜치 활성 세션 감지
- **Out of Scope**: zellij 기본값 지원, 원격 SSH 프로젝트 자동 연결, ConnectTerminalForm 제거 (fallback으로 유지)

## Codebase Analysis Summary
- `createDefaultBranchTask()` (project.ts:18-27): 현재 projectId, baseBranch만 설정하고 세션 정보를 설정하지 않음
- `createSessionWithoutWorktree()` (worktree.ts:82-122): 워크트리 없이 tmux window/zellij tab을 생성하는 함수 (이미 구현됨)
- `connectTerminalSession()` (kanban.ts:241-281): 기존 태스크에 세션을 연결하는 서버 액션 (참고 패턴)
- `scanAndRegisterProjects()` (project.ts:106-238): 워크트리 브랜치를 스캔하여 활성 세션 감지 (메인 브랜치는 미감지)
- task detail page (page.tsx:48): `hasTerminal = task.sessionType && task.sessionName`으로 터미널 표시 판단

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/actions/project.ts` | 프로젝트 등록 및 메인 브랜치 태스크 생성 | Modify |
| `src/lib/worktree.ts` | 세션 생성 함수 | Reference |
| `src/app/actions/kanban.ts` | connectTerminalSession 참고 패턴 | Reference |
| `src/app/[locale]/task/[id]/page.tsx` | 태스크 상세 페이지 (변경 불필요) | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 세션 타입 기본값 | scanAndRegisterProjects line 209 | tmux를 기본 세션 타입으로 사용 |
| 에러 처리 | createTask lines 97-99 | 세션 생성 실패 시 console.error + 태스크는 세션 없이 저장 |
| 직렬화 패턴 | serialize() 함수 | 엔티티 반환 시 JSON.parse(JSON.stringify()) |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 기본 세션 타입 | SessionType.TMUX | scanAndRegisterProjects에서 tmux 기본값 사용하는 기존 컨벤션 | zellij, 사용자 선택 |
| 세션 생성 실패 처리 | 무시하고 태스크 저장 | createTask의 기존 에러 처리 패턴과 동일 | 에러 반환, 재시도 |

## Implementation Todos

### Todo 1: createDefaultBranchTask에서 tmux 세션 자동 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 프로젝트 등록 시 메인 브랜치 태스크에 tmux 세션을 자동 연결한다
- **Work**:
  - `src/app/actions/project.ts`의 `createDefaultBranchTask()` 함수 수정
  - `createSessionWithoutWorktree()` 호출하여 tmux window 생성
  - 태스크에 `sessionType: SessionType.TMUX`, `sessionName`, `worktreePath: project.repoPath` 설정
  - `status: TaskStatus.PROGRESS`로 변경 (워크트리 태스크와 동일)
  - 세션 생성 실패 시 기존 동작 유지 (세션 없는 TODO 태스크)
- **Convention Notes**: 에러 처리는 createTask의 try-catch + console.error 패턴 사용
- **Verification**: 프로젝트 등록 후 메인 브랜치 태스크에 터미널 표시 확인
- **Exit Criteria**: `hasTerminal`이 true가 되어 TerminalLoader 렌더링
- **Status**: pending

### Todo 2: scanAndRegisterProjects에서 메인 브랜치 태스크 활성 세션 감지
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 스캔 시 기존 메인 브랜치 태스크에 활성 tmux 세션이 있으면 자동 연결한다
- **Work**:
  - `src/app/actions/project.ts`의 `scanAndRegisterProjects()` 함수 수정
  - 워크트리 스캔 루프 이후, 각 프로젝트의 메인 브랜치 태스크를 조회
  - 메인 브랜치 태스크에 `sessionType`이 없고, tmux 세션에 해당 window가 활성인 경우 세션 정보 업데이트
  - `isWindowAlive()`로 `formatWindowName(project.defaultBranch)` 윈도우 존재 여부 확인
- **Convention Notes**: isWindowAlive 호출 시 SessionType.TMUX 기본값 사용
- **Verification**: 이미 tmux 세션이 있는 프로젝트를 재스캔 시 메인 브랜치 태스크 연결 확인
- **Exit Criteria**: 활성 세션이 있는 메인 브랜치 태스크의 sessionType, sessionName이 설정됨
- **Status**: pending

## Verification Strategy
- `npm run build`로 빌드 성공 확인
- 프로젝트 등록 시나리오: 새 프로젝트 등록 → 메인 브랜치 태스크에 터미널 표시 확인
- 스캔 시나리오: 활성 tmux 세션이 있는 프로젝트 재스캔 → 메인 브랜치 태스크 세션 연결 확인

## Progress Tracking
- Total Todos: 2
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-14: Plan created
