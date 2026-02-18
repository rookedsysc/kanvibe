# Fix base_branch 누락 버그

## Business Goal
kanban_tasks 테이블의 base_branch 컬럼이 파생 브랜치 태스크에서 빈 값으로 저장되는 버그를 수정하여, 브랜치 계층 관계(부모-자식) 시각화가 가능하도록 한다.

## Scope
- **In Scope**: 태스크 생성 코드 3곳 수정 + 기존 데이터 보정 마이그레이션
- **Out of Scope**: git merge-base 기반 실제 부모 브랜치 탐지, UI/프론트엔드 변경

## Codebase Analysis Summary
태스크 생성 경로 3곳 중 2곳에서 baseBranch를 설정하지 않음. orphan 태스크 연결 시에도 미설정.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/actions/project.ts` | worktree 스캔 및 태스크 생성 | Modify |
| `src/app/api/hooks/start/route.ts` | AI hook API 태스크 생성 | Modify |
| `src/migrations/` | 기존 데이터 보정 | Create |
| `src/lib/database.ts` | 마이그레이션 등록 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 마이그레이션 패턴 | 기존 마이그레이션 파일 | TypeORM MigrationInterface, up/down 구현 |
| 마이그레이션 등록 | CLAUDE.md | database.ts의 migrations 배열에 import 추가 |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어로 작성 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| base branch 결정 방식 | project.defaultBranch 기본값 | 단순/안정적, SSH 호환 | git merge-base |
| 데이터 보정 방식 | TypeORM 마이그레이션 | 프로젝트 컨벤션 준수 | SQL 스크립트 수동 실행 |

## Implementation Todos

### Todo 1: worktree 스캔 태스크 생성 시 baseBranch 설정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: scanAndRegisterProjects에서 새 태스크/orphan 태스크에 baseBranch를 설정한다
- **Work**:
  - `src/app/actions/project.ts`의 `scanAndRegisterProjects` 함수 내 worktree 스캔 루프에서:
    - 새 태스크 생성 시 `baseBranch: project.defaultBranch` 추가 (line ~288)
    - orphan 태스크 연결 시 `orphanTask.baseBranch = project.defaultBranch` 추가 (line ~270)
- **Convention Notes**: 기존 코드 스타일 유지, 한국어 주석
- **Verification**: TypeScript 컴파일 통과 (`pnpm build`)
- **Exit Criteria**: 두 경로 모두 baseBranch가 설정됨
- **Status**: pending

### Todo 2: AI hook API에서 baseBranch 반영
- **Priority**: 1
- **Dependencies**: none
- **Goal**: POST /api/hooks/start에서 계산한 base 값을 task.baseBranch에 저장한다
- **Work**:
  - `src/app/api/hooks/start/route.ts`의 `POST` 함수에서:
    - worktree 생성 시 계산한 `base` 값을 `task.baseBranch = base`로 설정 (line ~44 이후)
- **Convention Notes**: 기존 코드 스타일 유지
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: hook API를 통해 생성된 태스크의 baseBranch에 올바른 값 저장
- **Status**: pending

### Todo 3: 기존 데이터 보정 마이그레이션 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 기존 빈 base_branch를 프로젝트의 defaultBranch로 채우는 마이그레이션 생성
- **Work**:
  - `src/migrations/` 에 새 마이그레이션 파일 생성
  - UP: projects 테이블의 default_branch를 JOIN하여 빈 base_branch를 채움
  - DOWN: 보정된 값을 NULL로 되돌림 (rollback)
  - `src/lib/database.ts`의 migrations 배열에 새 마이그레이션 import 추가
- **Convention Notes**: 기존 마이그레이션 파일 패턴 준수, database.ts에 등록 필수
- **Verification**: 마이그레이션 파일 문법 검증 (TypeScript 컴파일)
- **Exit Criteria**: 마이그레이션 파일 생성 및 database.ts에 등록 완료
- **Status**: pending

## Verification Strategy
- `pnpm build` — TypeScript 컴파일 성공 확인
- 마이그레이션 SQL 로직 검토

## Progress Tracking
- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-18: Plan created
