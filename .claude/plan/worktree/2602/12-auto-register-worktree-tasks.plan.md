# Worktree 자동 TODO 등록

## Business Goal
프로젝트 스캔 시 기존 git worktree를 자동으로 감지하여 KanbanTask(TODO)로 등록함으로써, 이미 진행 중인 브랜치 작업을 칸반 보드에서 즉시 추적할 수 있도록 한다.

## Scope
- **In Scope**: `listWorktrees` 함수 추가, `scanAndRegisterProjects`에 worktree 스캔 통합, `ScanResult` 확장, UI 결과 표시
- **Out of Scope**: 기존 task와 worktree 상태 동기화, 별도 worktree 스캔 버튼

## Codebase Analysis Summary
- `scanAndRegisterProjects`가 `scanGitRepos`로 `.git` 디렉토리를 탐색하여 프로젝트 등록
- `KanbanTask`에 `branchName`(unique), `worktreePath`, `projectId` 필드 존재
- `execGit` 함수가 로컬/SSH 명령 실행을 추상화
- `createTask`가 KanbanTask 생성 패턴 제공

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/gitOperations.ts` | Git 명령 실행 유틸 | Modify - `listWorktrees` 함수 추가 |
| `src/app/actions/project.ts` | 프로젝트 CRUD 서버 액션 | Modify - worktree 스캔 로직 추가 |
| `src/app/actions/kanban.ts` | 칸반 태스크 CRUD | Reference - `getTaskRepository` 패턴 참조 |
| `src/components/ProjectSettings.tsx` | 프로젝트 설정 UI | Modify - worktree 스캔 결과 표시 |
| `messages/ko.json` | 한국어 번역 | Modify - worktree 관련 메시지 추가 |
| `messages/en.json` | 영어 번역 | Modify - worktree 관련 메시지 추가 |
| `messages/zh.json` | 중국어 번역 | Modify - worktree 관련 메시지 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 함수 네이밍 | CODE_PRINCIPLES.md | `listWorktrees` - 조회는 `get/fetch/list` 동사 사용 |
| Korean 주석 | CODE_PRINCIPLES.md | 주석은 한국어, 서술형 |
| execGit 패턴 | gitOperations.ts | sshHost 분기 처리 포함 |
| Server Action export | project.ts | `"use server"` 파일 내 async 함수 export |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Worktree 목록 조회 | `git worktree list --porcelain` | 파싱 안정적, 경로/브랜치 정확 | `__worktrees` 디렉토리 직접 스캔 |
| 중복 체크 기준 | branchName | DB unique 제약조건과 일치 | worktreePath |
| default branch 제외 | bare worktree 스킵 | 메인 브랜치는 작업 대상 아님 | 전체 포함 |
| 함수 배치 | gitOperations.ts | 기존 git 유틸 함수와 동일 모듈 | worktree.ts에 추가 |

## Implementation Todos

### Todo 1: `listWorktrees` 함수 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: git worktree 목록을 파싱하여 반환하는 유틸 함수 구현
- **Work**:
  - `src/lib/gitOperations.ts`에 `WorktreeInfo` 인터페이스 추가 (`path`, `branch`, `isBare` 필드)
  - `listWorktrees(repoPath: string, sshHost?: string | null): Promise<WorktreeInfo[]>` 함수 구현
  - `git -C "{repoPath}" worktree list --porcelain` 명령 실행
  - 파싱: `worktree {path}`, `branch refs/heads/{name}`, `bare` 라인 처리
  - bare worktree 포함하여 반환 (필터링은 호출 측에서)
- **Convention Notes**: `execGit` 패턴 사용, Korean 주석
- **Verification**: 함수가 올바른 타입을 반환하는지 빌드 확인
- **Exit Criteria**: `listWorktrees` 함수가 export되고 `WorktreeInfo[]`를 반환
- **Status**: completed

### Todo 2: `scanAndRegisterProjects`에 worktree 스캔 통합
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 프로젝트 스캔 완료 후 모든 등록 프로젝트의 worktree를 감지하여 TODO task 자동 생성
- **Work**:
  - `ScanResult` 인터페이스에 `worktreeTasks: string[]` 필드 추가 (생성된 task 이름 목록)
  - `scanAndRegisterProjects` 함수 하단에 worktree 스캔 로직 추가:
    1. DB에서 모든 프로젝트 조회 (기존 + 신규)
    2. 각 프로젝트마다 `listWorktrees(project.repoPath, project.sshHost)` 호출
    3. bare가 아닌 worktree에 대해 branchName으로 기존 KanbanTask 조회
    4. 기존 task가 없으면 `taskRepo.create({ title: branchName, branchName, worktreePath, projectId, status: TODO })` 후 save
  - `getTaskRepository` import 추가
  - `KanbanTask`, `TaskStatus` import 추가
- **Convention Notes**: 기존 for-of 루프 패턴 유지, error 핸들링은 result.errors에 push
- **Verification**: 빌드 성공 확인
- **Exit Criteria**: worktree가 있는 프로젝트 스캔 시 TODO task 자동 생성, 중복 스킵
- **Status**: completed

### Todo 3: UI에 worktree 스캔 결과 표시
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 프로젝트 스캔 결과에 worktree 자동 등록 건수를 사용자에게 알림
- **Work**:
  - `messages/ko.json`에 `projectSettings.worktreeTasksRegistered` 키 추가: `"{count}개의 worktree가 TODO로 등록되었습니다"`
  - `messages/en.json`에 동일 키: `"{count} worktrees registered as TODO"`
  - `messages/zh.json`에 동일 키: `"{count}个worktree已注册为TODO"`
  - `src/components/ProjectSettings.tsx`의 `handleScan` 성공 메시지에 worktree 결과 추가
  - `result.worktreeTasks.length > 0`이면 추가 메시지 표시
- **Convention Notes**: next-intl `t()` 함수 사용, 기존 메시지 표시 패턴 유지
- **Verification**: 빌드 성공 확인, UI에서 스캔 후 메시지 표시
- **Exit Criteria**: worktree 등록 결과가 사용자에게 표시됨
- **Status**: completed

## Verification Strategy
- `npm run build` (또는 `next build`)로 타입 에러 없이 빌드 성공 확인
- worktree가 존재하는 프로젝트를 스캔했을 때 TODO task가 자동 생성되는지 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-02-12: Plan created
- 2026-02-12: Execution complete — all 3 todos implemented
