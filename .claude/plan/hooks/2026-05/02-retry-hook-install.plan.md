# Retry Hook Install

## Business Goal
새 task 생성 직후 hook 설치가 일시적인 파일/원격 명령 실패 때문에 자주 실패로 끝나는 문제를 줄이고, 사용자가 별도 수동 재설치를 반복하지 않게 한다.

## Scope
- **In Scope**: `installKanvibeHooks()`에 제한된 재시도 정책 추가, 기존 실패 전파 정책 유지, 관련 단위 테스트 보강
- **Out of Scope**: hook 설치 UI 변경, provider별 hook 스크립트 구조 변경, SSH 연결 정책 재설계

## Codebase Analysis Summary
새 로컬 worktree task 생성은 `kanbanService.createTask()`에서 `scheduleTaskHookInstall()`로 백그라운드 설치를 예약한다. 예약된 작업은 `installKanvibeHooks()`를 한 번만 호출하고 실패하면 즉시 `broadcastTaskHookInstallFailed()`를 보낸다. `installKanvibeHooks()`는 로컬 provider 설치를 병렬 실행하고, 원격 provider 설치를 순차 실행하지만 재시도 로직은 없다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/kanvibeHooksInstaller.ts` | Claude/Gemini/Codex/OpenCode hook 설치 단일 진입점 | Modify |
| `src/lib/__tests__/kanvibeHooksInstaller.test.ts` | hook installer 동작 테스트 | Modify |
| `src/desktop/main/services/__tests__/kanbanService.test.ts` | 새 task 백그라운드 hook 설치 테스트 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 테스트 우선 | `superpowers:test-driven-development` | 실패하는 회귀 테스트를 먼저 추가하고 확인한다 |
| 최소 수정 | 기존 서비스 패턴 | hook 설치 진입점 중심으로 변경하고 호출부 리팩터링은 피한다 |
| 오류 로깅 | `kanvibeHooksInstaller.ts` 기존 패턴 | provider, targetPath, taskId, sshHost, error를 구조화해 로그로 남긴다 |
| 파일 수정 | developer instruction | 수동 편집은 `apply_patch`를 사용한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| retry 위치 | `installKanvibeHooks()` wrapper | 새 task, 수동 재설치, 프로젝트 복구가 같은 설치 안정성을 공유한다 | `scheduleTaskHookInstall()`에만 적용하면 원격/수동 경로가 빠진다 |
| retry 횟수 | 총 3회 | 일시 실패 완화와 사용자 대기 시간 사이의 균형이 좋다 | 2회는 효과가 낮고, 5회 이상은 실패 확정까지 너무 늦다 |
| retry 간격 | 짧은 선형 지연 | worktree 직후 파일 시스템/SSH 안정화 시간을 준다 | 즉시 retry, exponential backoff |

## Implementation Todos

### Todo 1: hook 설치 재시도 회귀 테스트 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 일시적인 설치 실패가 재시도 후 성공해야 한다는 기대를 테스트로 고정한다.
- **Work**:
  - `src/lib/__tests__/kanvibeHooksInstaller.test.ts`에 로컬 provider가 1회 실패 후 성공하는 케이스를 추가한다.
  - 기존 영구 실패 테스트는 재시도 후에도 실패하는 정책을 기대하도록 조정한다.
- **Convention Notes**: Vitest 기존 mock 구조와 한국어 테스트명을 유지한다.
- **Verification**: `pnpm test -- src/lib/__tests__/kanvibeHooksInstaller.test.ts`
- **Exit Criteria**: 구현 전 새 테스트가 재시도 부재 때문에 실패한다.
- **Status**: completed

### Todo 2: `installKanvibeHooks()` 재시도 구현
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: hook 설치 전체 시도를 최대 3회까지 재시도하고, 마지막 실패만 호출자에게 전파한다.
- **Work**:
  - 기존 설치 본문을 내부 `installKanvibeHooksOnce()`로 분리한다.
  - exported `installKanvibeHooks()`는 실패 시 짧은 지연 후 재시도한다.
  - retry 로그에는 attempt/maxAttempts/targetPath/taskId/sshHost/error를 포함한다.
- **Convention Notes**: provider별 setup 함수와 verification 로직은 변경하지 않는다.
- **Verification**: `pnpm test -- src/lib/__tests__/kanvibeHooksInstaller.test.ts`
- **Exit Criteria**: 새 회귀 테스트와 기존 installer 테스트가 통과한다.
- **Status**: completed

### Todo 3: 영향 범위 검증
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 새 task 백그라운드 설치 동작과 타입 안정성을 확인한다.
- **Work**:
  - `src/desktop/main/services/__tests__/kanbanService.test.ts` focused test를 실행한다.
  - 타입 체크를 실행한다.
- **Convention Notes**: 실패 시 해당 원인만 최소 수정한다.
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts`, `pnpm check`
- **Exit Criteria**: 관련 테스트와 타입 체크가 통과한다.
- **Status**: completed

## Verification Strategy
- `pnpm test -- src/lib/__tests__/kanvibeHooksInstaller.test.ts`
- `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts`
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-05-02: Plan created
- 2026-05-02: Todo 1 completed — added retry regression tests and confirmed they fail before implementation
- 2026-05-02: Todo 2 completed — wrapped hook installation with three-attempt retry behavior
- 2026-05-02: Todo 3 completed — verified kanban task creation tests and TypeScript check
