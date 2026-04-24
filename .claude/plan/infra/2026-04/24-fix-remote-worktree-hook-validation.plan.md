# Remote Worktree And Hook Validation Fixes

## Business Goal
원격 프로젝트에서 기존 worktree를 TODO 태스크로 안정적으로 복원하고, bare common repo를 worktree처럼 다루면서 발생하는 hook/diff 오탐을 줄이며, 선택적 CLI(`gh`) 부재로 인한 불필요한 오류 로그를 제거한다.

## Scope
- **In Scope**: 스캔 시 git 공통 저장소 경로 정규화, 기본 브랜치 태스크의 worktree 경로 보정, hook 서버 검증 오탐 완화, `gh` 미설치 예외 처리, 관련 unit test 추가
- **Out of Scope**: hook 상태 UI 재디자인, 원격 SSH 재접속 전략 변경, E2E 테스트 추가

## Codebase Analysis Summary
프로젝트 등록과 worktree 스캔은 `projectService`가 담당하고, git/SSH 유틸은 `gitOperations`와 `hostFileAccess`에 분산돼 있다. 기본 브랜치 태스크는 현재 `project.repoPath`를 그대로 `worktreePath`로 저장하는데, bare common repo일 때 `git status`가 실패한다. hook 검증은 `hookServerStatus`에서 원격 `curl` probe를 SSH로 실행하며, SSH transport 오류도 미설치로 처리하고 있다. PR URL 조회는 `kanbanService`에서 `gh` CLI를 직접 실행한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/desktop/main/services/projectService.ts` | 프로젝트 등록, root task 보정, worktree 스캔 | Modify |
| `src/lib/hookServerStatus.ts` | hook 서버 URL/헬스 검증 | Modify |
| `src/desktop/main/services/kanbanService.ts` | PR URL 조회 | Modify |
| `src/desktop/main/services/__tests__/projectService.test.ts` | 프로젝트/스캔 회귀 테스트 | Modify |
| `src/lib/__tests__/hookServerStatus.test.ts` | hook 검증 테스트 | Modify |
| `src/desktop/main/services/__tests__/kanbanService.test.ts` | `gh` 조회 테스트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 기존 서비스 계층 테스트 패턴 | `src/desktop/main/services/__tests__/*` | `vi.hoisted` 기반 mock, Given/When/Then 구조 유지 |
| hook 상태 계산 방식 | `src/lib/*HooksSetup.ts`, `src/lib/hookServerStatus.ts` | 설치 여부 판정은 boolean contract 유지, 오탐만 줄이고 구조는 유지 |
| 최소 범위 수정 | repo 전반 | 관련 서비스와 테스트만 수정하고 UI/DB 스키마는 건드리지 않음 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 스캔 결과 저장 경로 | linked worktree 경로를 git common repo 경로로 정규화 | 기존 프로젝트와 동일 repo를 안정적으로 매칭하고 worktree 스캔 대상을 잃지 않기 위해 | worktree를 별도 프로젝트로 유지 |
| 기본 브랜치 태스크 경로 | 실제 default branch worktree가 있을 때만 `worktreePath` 보유 | bare common repo에 `git status`를 실행하는 회귀를 막기 위해 | 항상 `project.repoPath` 저장 |
| 원격 hook 헬스체크 오류 처리 | SSH transport 오류는 reachability 오탐으로 간주하지 않음 | SSH probe 실패가 hook misconfiguration과 동일하지 않기 때문 | 모든 probe 실패를 미설치로 처리 |
| PR URL 조회 | `gh` 미설치는 정상적인 null 결과로 처리 | optional dependency 부재로 사용자 로그를 오염시키지 않기 위해 | 현재처럼 예외 로그 유지 |

## Implementation Todos

### Todo 1: 프로젝트 등록 경로와 root task 경로 정규화
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 스캔/등록 단계에서 linked worktree와 bare common repo를 올바르게 구분한다.
- **Work**:
  - `src/desktop/main/services/projectService.ts`에 scanned repo path를 git common repo 기준으로 정규화하는 helper를 추가한다.
  - `registerProject`, `scanAndRegisterProjects`가 정규화된 repo path를 기준으로 프로젝트를 등록/매칭하도록 수정한다.
  - 기본 브랜치 태스크 보정 시 실제 default branch worktree만 `worktreePath`로 저장하고, 없으면 `null`을 저장한다.
- **Convention Notes**: 기존 service 함수 내부 helper 스타일과 오류 무시 패턴을 유지한다.
- **Verification**: 관련 unit test 실행, 스캔 대상이 common repo 기준으로 매칭되는지 확인
- **Exit Criteria**: existing project가 common repo path를 써도 linked worktree 스캔 후 TODO 태스크가 생성되고, bare repo path가 root task `worktreePath`로 남지 않는다.
- **Status**: completed

### Todo 2: hook/PR 예외 처리 안정화
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 원격 hook 검증과 `gh` 조회에서 사용자에게 불필요한 오류를 남기지 않는다.
- **Work**:
  - `src/lib/hookServerStatus.ts`에서 SSH transport 오류를 hook 설치 실패로 단정하지 않도록 처리한다.
  - `src/desktop/main/services/kanbanService.ts`에서 `gh` 미설치(`ENOENT`)를 조용히 `null`로 처리한다.
- **Convention Notes**: 기존 반환 타입을 유지하고, optional dependency 실패는 noisy log 없이 복구 가능한 흐름으로 맞춘다.
- **Verification**: hook/PR 관련 unit test 실행
- **Exit Criteria**: SSH probe transport error와 `gh` 미설치가 회귀 테스트로 고정된다.
- **Status**: completed

### Todo 3: 회귀 테스트 보강
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 이번 수정 범위를 테스트로 고정한다.
- **Work**:
  - `projectService.test.ts`에 common repo 정규화와 root task `worktreePath` 보정 테스트를 추가한다.
  - `hookServerStatus.test.ts`에 SSH transport 오류 fallback 테스트를 추가한다.
  - `kanbanService.test.ts`에 `gh` ENOENT 무시 테스트를 추가한다.
- **Convention Notes**: 테스트명은 한국어 설명을 유지하고 Given/When/Then 흐름을 따른다.
- **Verification**: `pnpm test -- --runInBand` 대신 관련 vitest 파일 단위 실행
- **Exit Criteria**: 새 테스트가 실패-수정-통과 흐름으로 검증되고 전체 관련 스위트가 통과한다.
- **Status**: completed

## Verification Strategy
관련 테스트 파일만 선택 실행하여 수정 범위를 빠르게 검증한다.
- `pnpm test src/desktop/main/services/__tests__/projectService.test.ts`
- `pnpm test src/lib/__tests__/hookServerStatus.test.ts`
- `pnpm test src/desktop/main/services/__tests__/kanbanService.test.ts`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-04-24: Plan created
- 2026-04-24: Todo 1 completed — scanned repo path를 common repo 기준으로 정규화하고 root task worktreePath 보정 로직을 추가
- 2026-04-24: Todo 2 completed — remote hook reachability probe의 SSH transport 오탐과 `gh` ENOENT 로그를 제거
- 2026-04-24: Todo 3 completed — projectService, kanbanService, hookServerStatus 회귀 테스트를 추가하고 통과 확인
