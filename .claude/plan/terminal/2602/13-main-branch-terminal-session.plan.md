# 메인 브랜치 터미널 세션 연결

## Business Goal
메인 브랜치(main/master) 태스크도 tmux/zellij 세션에 연결할 수 있도록 하여, 모든 브랜치에서 통합 터미널 경험을 제공한다.

## Scope
- **In Scope**: 메인 브랜치 전용 세션 생성/삭제 함수, 태스크 상세 페이지 "터미널 연결" UI, 서버 windowName 파생 로직 수정, 태스크 삭제 시 메인 브랜치 세션 정리, i18n 번역 키 추가
- **Out of Scope**: DB 스키마 변경, 프로젝트 등록 시 자동 세션 생성

## Codebase Analysis Summary
- `createDefaultBranchTask()`는 `sessionType`, `sessionName`, `branchName` 없이 태스크 생성
- `server.ts`의 WebSocket 핸들러는 `sessionType`/`sessionName` 필수 체크
- `worktree.ts`의 세션 생성은 git worktree 생성과 결합되어 있음
- `branchName`은 unique 제약이 있어 메인 브랜치에 직접 설정 불가 (여러 프로젝트가 같은 "main" 사용)
- `baseBranch`는 이미 default branch 태스크에 설정되어 있음

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/worktree.ts` | 세션 생성/삭제 유틸리티 | Modify — 메인 브랜치 전용 함수 추가 |
| `src/app/actions/kanban.ts` | Server Actions | Modify — connectSessionToMainBranch 액션 추가, deleteTask 수정 |
| `server.ts` | WebSocket 터미널 서버 | Modify — windowName 파생 로직 수정 |
| `src/app/[locale]/task/[id]/page.tsx` | 태스크 상세 페이지 | Modify — 터미널 연결 UI 추가 |
| `src/components/ConnectTerminalForm.tsx` | 터미널 연결 폼 | Create — 세션 타입 선택 + 연결 버튼 |
| `messages/ko.json` | 한국어 번역 | Modify — 번역 키 추가 |
| `messages/en.json` | 영어 번역 | Modify — 번역 키 추가 |
| `messages/zh.json` | 중국어 번역 | Modify — 번역 키 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Server Action 패턴 | `kanban.ts` | `"use server"` + `revalidatePath("/")` |
| 세션 생성 패턴 | `worktree.ts` | `execGit()` + tmux/zellij 명령 분기 |
| 직렬화 패턴 | `kanban.ts` | `serialize()` 래퍼로 반환값 변환 |
| i18n 번역 | `messages/*.json` | ko/en/zh 3개 언어 동시 추가 |
| CSS 토큰 | `CLAUDE.md` | 디자인 시스템 CSS 변수 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| branchName 처리 | null 유지, baseBranch로 window 파생 | `branchName` unique 제약 때문에 "main" 중복 불가 | branchName에 값 설정 |
| worktreePath | project.repoPath 저장 | 메인 브랜치는 primary checkout | null 유지 |
| 세션 연결 시점 | On-demand (버튼 클릭) | 프로젝트 등록 시 sessionType 미확정 | 등록 시 자동 생성 |
| 삭제 시 처리 | window만 제거 | 메인 브랜치/worktree 삭제 방지 | 전체 정리 (위험) |

## Implementation Todos

### Todo 1: 메인 브랜치 전용 세션 생성/삭제 함수 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: worktree 없이 메인 프로젝트 디렉토리를 가리키는 tmux window/zellij tab을 생성/삭제하는 함수 구현
- **Work**:
  - `src/lib/worktree.ts`에 `createMainBranchSession()` 함수 추가
    - 파라미터: `projectPath`, `defaultBranch`, `sessionType`, `sshHost?`
    - tmux: `tmux has-session` 확인 → `tmux new-window -t sessionName -n windowName -c projectPath`
    - zellij: 세션 확인 → `zellij action new-tab --name windowName --cwd projectPath`
    - 반환: `{ sessionName: string }`
  - `src/lib/worktree.ts`에 `removeMainBranchSession()` 함수 추가
    - 파라미터: `sessionType`, `sessionName`, `defaultBranch`, `sshHost?`
    - window/tab만 제거 (worktree, branch 삭제 안 함)
- **Convention Notes**: 기존 `createWorktreeWithSession` 패턴 따름, `execGit` 사용
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: 함수가 export되고 타입이 올바름
- **Status**: pending

### Todo 2: server.ts windowName 파생 로직 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: branchName이 null인 메인 브랜치 태스크에서도 windowName을 올바르게 파생
- **Work**:
  - `server.ts:73`의 windowName 파생 로직 수정:
    - `branchName` 있으면 `formatWindowName(branchName)` (현행 유지)
    - `branchName` 없고 `baseBranch` 있으면 `formatWindowName(baseBranch)` (신규)
    - 둘 다 없으면 빈 문자열 (현행 유지)
- **Convention Notes**: 기존 코드 스타일 유지
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: 메인 브랜치 태스크의 windowName이 baseBranch에서 파생됨
- **Status**: pending

### Todo 3: connectSessionToMainBranch 액션 + deleteTask 수정
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 메인 브랜치에 세션을 연결하는 Server Action 추가, 삭제 시 안전한 정리
- **Work**:
  - `src/app/actions/kanban.ts`에 `connectSessionToMainBranch()` 함수 추가:
    - 파라미터: `taskId`, `sessionType`
    - task 조회 → project 조회 → `createMainBranchSession()` 호출
    - task에 `sessionType`, `sessionName`, `worktreePath`(=project.repoPath) 설정
    - `status`를 PROGRESS로 변경
    - `revalidatePath("/")` + `revalidatePath(/task/${taskId})`
  - `deleteTask()` 수정:
    - 메인 브랜치 세션인 경우(branchName null + sessionType 존재) `removeMainBranchSession()` 호출
    - 기존 worktree 브랜치 로직은 유지
- **Convention Notes**: serialize 래퍼 사용, revalidatePath 호출
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: 액션 함수가 올바른 타입으로 export됨
- **Status**: pending

### Todo 4: 터미널 연결 UI 컴포넌트 + 태스크 상세 페이지 수정
- **Priority**: 2
- **Dependencies**: Todo 3
- **Goal**: 터미널이 없는 메인 브랜치 태스크에서 세션 연결 버튼 표시
- **Work**:
  - `src/components/ConnectTerminalForm.tsx` 생성:
    - props: `taskId`, `hasProject` (프로젝트 연결 여부)
    - sessionType select (tmux/zellij) + "연결" 버튼
    - `connectSessionToMainBranch` 호출
    - `useTransition` 사용하여 pending 상태 처리
  - `src/app/[locale]/task/[id]/page.tsx` 수정:
    - `hasTerminal` false일 때 기존 "noTerminal" 메시지 대신 `ConnectTerminalForm` 표시
    - 단, `task.projectId`가 있는 경우에만 (프로젝트 미연결 태스크는 기존대로)
- **Convention Notes**: 디자인 시스템 CSS 변수 사용, i18n `useTranslations` 사용
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: 메인 브랜치 태스크 상세 페이지에서 연결 버튼이 보임
- **Status**: pending

### Todo 5: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 터미널 연결 UI에 필요한 번역 키를 ko/en/zh에 추가
- **Work**:
  - `messages/ko.json`의 `taskDetail` 네임스페이스에 추가:
    - `connectTerminal`: "터미널 연결"
    - `connecting`: "연결 중..."
    - `selectSessionType`: "세션 타입"
  - `messages/en.json`에 동일 키 영어 번역 추가
  - `messages/zh.json`에 동일 키 중국어 번역 추가
- **Convention Notes**: 기존 번역 파일 구조/네이밍 패턴 유지
- **Verification**: JSON 유효성 확인
- **Exit Criteria**: 3개 언어 파일에 동일 키가 존재
- **Status**: pending

## Verification Strategy
- `npx tsc --noEmit` 타입 체크 통과
- `npm run build` 빌드 성공
- 메인 브랜치 태스크 상세 페이지에서 연결 버튼 표시 확인

## Progress Tracking
- Total Todos: 5
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-13: Plan created
