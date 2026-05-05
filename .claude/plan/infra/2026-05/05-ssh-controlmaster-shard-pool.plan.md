# SSH ControlMaster Shard Pool

## Business Goal
원격 SSH 기반 작업에서 비대화형 명령 처리량을 높여 remote worktree, diff, AI session 조회 같은 반복 작업의 체감 지연을 줄인다. 기존 SSH config와 OpenSSH ControlMaster 재사용 구조는 유지하되, 단일 ControlMaster socket에 모든 명령이 몰리는 병목을 줄인다.

## Scope
- **In Scope**: `execGit(..., sshHost)` 경로의 host별 동시성 기본값 상향, 여러 ControlMaster socket shard 분산, 기존 환경변수 호환, 관련 Vitest 보강.
- **Out of Scope**: 웹 터미널 attach 최적화, 원격 파일 조회 batching 대개편, SSH 서버 설정 변경, UI 설정 추가.

## Codebase Analysis Summary
비대화형 원격 명령은 `src/lib/gitOperations.ts`의 `execRemote`를 통해 `ssh` 프로세스를 spawn한다. 현재 non-Windows 환경에서는 `src/lib/sshConfig.ts`의 `getKanvibeSSHConnectionReuseOptions()`가 단일 `~/.kanvibe/ssh-%C` ControlPath를 반환하며, `gitOperations.ts`는 host별 limiter로 기본 4개, 환경변수로 최대 16개까지 명령을 실행한다. 기존 테스트는 `src/lib/__tests__/gitOperations.test.ts`에서 spawn args와 동시성 제한을 검증한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/gitOperations.ts` | 원격 SSH 명령 실행, retry, limiter, ControlMaster 종료 | Modify |
| `src/lib/sshConfig.ts` | SSH args와 ControlMaster 재사용 옵션 생성 | Modify |
| `src/lib/__tests__/gitOperations.test.ts` | 원격 SSH 실행/동시성 단위 테스트 | Modify |
| `src/lib/__tests__/sshConfig.test.ts` | SSH args/ControlPath 옵션 단위 테스트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 기존 함수 경계 유지 | `gitOperations.ts`, `sshConfig.ts` | SSH option 생성은 `sshConfig.ts`, 실행 정책은 `gitOperations.ts`에 둔다. |
| 환경변수 호환 | `getRemoteSSHHostMaxConcurrency()` | 기존 `KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY`를 계속 지원한다. |
| 테스트 우선 | `gitOperations.test.ts` 패턴 | spawn mock과 `vi.doMock("@/lib/sshConfig")` 패턴을 재사용한다. |
| 최소 변경 | `CLAUDE.md`, 기존 코드 스타일 | UI/설정 저장소 등 무관 영역은 변경하지 않는다. |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| ControlMaster 확장 방식 | ControlPath shard pool | OpenSSH 기반 구현을 유지하면서 여러 TCP SSH 마스터 연결로 분산한다. | 새 SSH 클라이언트 라이브러리 도입 |
| 기본 host 동시성 | `availableParallelism() * 2`, cap 16 | 사용자 요구인 core 수 * 2에 맞추고 기존 최대 정책을 유지한다. | 고정 16, 기존 4 유지 |
| shard 수 | host 동시성 기준으로 산출하되 shard별 channel 수를 제한 | 서버 `MaxSessions` 리스크를 줄이고 단일 마스터 집중을 피한다. | 동시성만 상향 |
| Windows 동작 | 기존처럼 ControlMaster 비활성 | Windows OpenSSH socket 재사용 경로 차이를 피한다. | Windows까지 shard 적용 |

## Implementation Todos

### Todo 1: Add failing tests for SSH ControlMaster shard distribution
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 여러 원격 명령이 단일 ControlPath가 아닌 shard별 ControlPath로 분산되어야 한다는 기대 동작을 테스트로 고정한다.
- **Work**:
  - `src/lib/__tests__/sshConfig.test.ts`에 shard index가 포함된 KanVibe ControlPath 옵션 테스트를 추가한다.
  - `src/lib/__tests__/gitOperations.test.ts`에 동시 원격 명령이 여러 ControlPath shard를 사용하는 테스트를 추가한다.
- **Convention Notes**: 기존 mock helper와 Korean test naming style을 유지한다.
- **Verification**: `pnpm vitest run src/lib/__tests__/sshConfig.test.ts src/lib/__tests__/gitOperations.test.ts`
- **Exit Criteria**: 새 테스트가 현재 구현에서 기대한 이유로 실패한다.
- **Status**: completed

### Todo 2: Implement ControlMaster shard pool
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: host별 원격 명령을 여러 ControlMaster socket으로 분산하고 기본 동시성을 `core * 2` 기반으로 올린다.
- **Work**:
  - `src/lib/sshConfig.ts`에서 shard index를 받을 수 있는 ControlPath option API를 확장한다.
  - `src/lib/gitOperations.ts`에서 `availableParallelism()` 기반 기본 동시성을 계산한다.
  - host별 limiter가 실행 slot에 shard index를 부여하고 `buildExecSSHArgs()`가 해당 shard ControlPath를 사용하게 한다.
  - timeout/transport failure 시 해당 shard ControlMaster 종료 경로가 같은 ControlPath를 쓰도록 조정한다.
- **Convention Notes**: 환경변수 최대 16 cap과 retry/cooldown 의미를 유지한다.
- **Verification**: `pnpm vitest run src/lib/__tests__/sshConfig.test.ts src/lib/__tests__/gitOperations.test.ts`
- **Exit Criteria**: Todo 1 테스트와 기존 관련 테스트가 통과한다.
- **Status**: completed

### Todo 3: Run final verification and documentation consistency check
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 변경이 타입/테스트 관점에서 안전하고 문서 설명과 모순되지 않음을 확인한다.
- **Work**:
  - 관련 테스트를 재실행한다.
  - `pnpm check`를 실행한다.
  - README의 SSH 설명이 새 동작과 모순되지 않는지 확인하고 필요 시 문구를 좁게 갱신한다.
- **Convention Notes**: 문서 변경은 실제 동작 설명에 필요한 경우에만 수행한다.
- **Verification**: `pnpm vitest run src/lib/__tests__/sshConfig.test.ts src/lib/__tests__/gitOperations.test.ts`, `pnpm check`
- **Exit Criteria**: 관련 테스트와 타입 체크가 통과하고 계획 파일 상태가 완료로 갱신된다.
- **Status**: completed

## Verification Strategy
- `pnpm vitest run src/lib/__tests__/sshConfig.test.ts src/lib/__tests__/gitOperations.test.ts`
- `pnpm check`
- 변경된 SSH args에서 shard ControlPath가 분산되는지 테스트 assertion으로 확인한다.

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-05-05: Plan created
- 2026-05-05: Todo 1 completed — failing tests added for SSH ControlMaster shard distribution
- 2026-05-05: Todo 2 completed — ControlMaster shard pool implemented and related tests passed
- 2026-05-05: Todo 3 completed — final related tests and type check passed
- 2026-05-05: Execution complete
