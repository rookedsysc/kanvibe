# 새 태스크 생성 시 hooks 비동기 설치 및 실패 알럿

## Business Goal
새 태스크 생성 시 hooks 설치 대기 때문에 UI 응답이 느려지는 문제를 줄이고, 백그라운드 설치가 실패해도 사용자가 즉시 재설치 필요 상태를 인지할 수 있게 한다.

## Scope
- **In Scope**: 새 태스크 생성 경로에서 hooks 설치를 비동기로 전환, 실패 이벤트 브로드캐스트 추가, 앱 상단 실패 알럿 표시, 관련 회귀 테스트 추가
- **Out of Scope**: 기존 수동 hooks 재설치 UX 변경, worktree/세션 생성 플로우 재설계, 프로젝트 스캔/복구 경로의 동작 변경

## Codebase Analysis Summary
태스크 생성은 `kanbanService.createTask`에서 worktree/세션 생성 후 `installKanvibeHooks`를 직접 기다린 뒤 보드 업데이트를 브로드캐스트한다. 렌더러는 `window.kanvibeDesktop.onBoardEvent`로 메인 프로세스 이벤트를 구독하고 있고, 현재는 상태 변경 알림과 보드 리프레시만 처리한다. 상단 전역 토스트 시스템은 없지만 고정 배너를 App 레벨에 추가하기 쉬운 구조다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/desktop/main/services/kanbanService.ts` | 새 태스크 생성 및 hooks 자동 설치 | Modify |
| `src/lib/boardNotifier.ts` | 데스크톱 렌더러로 전달되는 board event 정의 | Modify |
| `src/desktop/renderer/App.tsx` | 전역 렌더러 셸 | Modify |
| `src/desktop/renderer/components/NotificationListener.tsx` | 기존 board event 구독 패턴 참고 | Reference |
| `src/desktop/main/services/__tests__/kanbanService.test.ts` | 태스크 생성 서비스 회귀 테스트 | Modify |
| `src/desktop/renderer/components/__tests__/NotificationListener.test.tsx` | board event 기반 컴포넌트 테스트 패턴 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Board event 브로드캐스트 | `src/lib/boardNotifier.ts` | 메인 서비스는 payload type을 명시하고 board event로 렌더러에 전달한다 |
| 렌더러 경량 메시지 패턴 | `src/components/HooksStatusDialog.tsx` | 실패 메시지는 작은 배너/알럿 스타일로 간결하게 노출한다 |
| 테스트 스타일 | `src/desktop/main/services/__tests__/kanbanService.test.ts` | Given/When/Then 주석과 vi mock 기반 단위 테스트를 유지한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| hooks 설치 실행 시점 | task 저장 후 백그라운드 `setTimeout`으로 스케줄 | 생성 응답을 막지 않으면서 기존 설치 로직 재사용 가능 | createTask 반환을 확장해 프론트에서 별도 후속 호출 |
| 실패 피드백 전달 | `boardNotifier`에 실패 이벤트 추가 | 기존 Electron board event 채널을 재사용해 구현 범위를 최소화 | 별도 IPC 채널 추가 |
| 사용자 피드백 UI | App 레벨 상단 고정 알럿 | 라우트 전환 후에도 보여줄 수 있고 “위에 살짝” 요구에 맞음 | 브라우저 notification 또는 각 페이지별 인라인 메시지 |

## Implementation Todos

### Todo 1: 비차단 hooks 설치 회귀 테스트 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: createTask가 hooks 설치 완료를 기다리지 않고 반환하는 동작과 실패 이벤트 방출을 테스트로 고정한다
- **Work**:
  - `src/desktop/main/services/__tests__/kanbanService.test.ts`에 background schedule 기준 기대값 추가
  - hooks 설치 Promise가 pending이어도 `createTask`가 resolve되는 케이스 작성
  - hooks 설치 reject 시 실패 이벤트 브로드캐스트 payload를 검증하는 테스트 작성
- **Convention Notes**: 기존 vi mock 구조와 Given/When/Then 패턴을 유지한다
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts`
- **Exit Criteria**: 새 테스트가 실패 상태로 추가되고, 구현 후 green으로 돌아간다
- **Status**: completed

### Todo 2: 메인 프로세스 비동기 hooks 설치 및 실패 이벤트 구현
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 태스크 생성 응답을 block하지 않도록 hooks 설치를 백그라운드화한다
- **Work**:
  - `src/lib/boardNotifier.ts`에 hooks 설치 실패 payload/type 추가
  - `src/desktop/main/services/kanbanService.ts`에서 `displayOrder` 조회를 앞당겨 병렬화
  - task 저장 후 background hooks install helper를 스케줄하고 실패 시 이벤트/로그를 남긴다
- **Convention Notes**: 기존 createTask 예외 처리 범위는 유지하고, 실패 로깅은 provider 에러를 그대로 살린다
- **Verification**: `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts`
- **Exit Criteria**: createTask 호출이 hooks 설치 완료와 분리되고 실패 시 event가 발행된다
- **Status**: completed

### Todo 3: 전역 상단 실패 알럿 렌더링
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: 백그라운드 hooks 설치 실패를 사용자가 즉시 인지할 수 있게 한다
- **Work**:
  - App 레벨 렌더러 컴포넌트를 추가해 `task-hook-install-failed` board event를 구독
  - 상단 고정 배너 스타일, 자동 숨김, 수동 닫기 동작을 구현
  - 다국어 메시지와 렌더러 테스트를 추가
- **Convention Notes**: 기존 Tailwind 토큰과 작은 인라인 메시지 톤을 유지한다
- **Verification**: `pnpm test -- src/desktop/renderer/components/__tests__/BoardEventAlert.test.tsx`
- **Exit Criteria**: 실패 이벤트 수신 시 상단 알럿이 노출되고 자동/수동 dismiss가 동작한다
- **Status**: completed

## Verification Strategy
서비스와 렌더러 단위 테스트를 우선 검증하고, 타입체크로 이벤트/번역 추가에 따른 정합성을 확인한다.
- `pnpm test -- src/desktop/main/services/__tests__/kanbanService.test.ts src/desktop/renderer/components/__tests__/BoardEventAlert.test.tsx`
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-04-24: Plan created
- 2026-04-24: createTask 비차단 hooks 설치 및 실패 board event 추가
- 2026-04-24: App 레벨 상단 hooks 실패 알럿과 다국어 메시지 추가
- 2026-04-24: kanbanService 및 BoardEventAlert 회귀 테스트 검증
