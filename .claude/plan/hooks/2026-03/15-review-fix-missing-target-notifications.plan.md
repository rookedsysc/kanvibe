# Review Fix for Missing Target Notifications

## Business Goal
필수 코드리뷰 지적을 반영해 실패 알림이 사용자 설정 때문에 조용히 누락되지 않도록 하고, 관련 테스트가 실행 순서에 의존하지 않도록 만들어 후속 유지보수 위험을 줄인다.

## Scope
- **In Scope**: 실패 알림 필터링 로직 수정, hooks status route 테스트의 모듈 리셋 추가, 관련 테스트 기대값 정리 및 검증
- **Out of Scope**: 알림 설정 UI/스키마 변경, 새 사용자 옵션 추가, Hook API 응답 변경

## Codebase Analysis Summary
- `src/components/NotificationListener.tsx`는 성공 상태 변경(`task-status-changed`)과 실패 이벤트(`hook-status-target-missing`)를 모두 처리하지만, 현재는 두 경우 모두 `enabledStatuses` 필터를 적용한다.
- `hook-status-target-missing`는 상태 변경 성공이 아니라 실패 신호이므로 requested status 필터에 막히면 사용자에게 중요한 장애 신호가 누락될 수 있다.
- `src/app/api/hooks/status/__tests__/route.test.ts`는 각 테스트에서 `await import(...)`로 route 모듈을 불러오지만 `beforeEach`에서 `vi.clearAllMocks()`만 호출하고 있어 기존 테스트 패턴보다 격리가 약하다.
- 기존 테스트들은 `NotificationListener.test.tsx`, `useTaskNotification.test.ts`처럼 `vi.resetModules()`를 사용해 동적 import 테스트 격리를 맞추고 있다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/NotificationListener.tsx` | WebSocket 알림 수신 처리 | Modify |
| `src/components/__tests__/NotificationListener.test.tsx` | 알림 수신 필터 테스트 | Modify |
| `src/app/api/hooks/status/__tests__/route.test.ts` | hooks status route 테스트 | Modify |
| `.claude/plan/hooks/2026-03/15-review-fix-missing-target-notifications.plan.md` | 후속 리뷰 반영 계획 | Create |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| KISS/YAGNI | `CODE_PRINCIPLES.md` | 리뷰 지적 범위만 최소 수정으로 반영 |
| 테스트 스타일 | `CLAUDE.md` | Vitest, 영어 테스트명, Given-When-Then 주석 유지 |
| 기존 테스트 패턴 일관성 | 기존 테스트 파일 | 동적 import 테스트는 `vi.resetModules()`로 격리 |
| 기존 성공 알림 의미 보존 | 현재 `NotificationListener.tsx` | `task-status-changed` 필터 동작은 그대로 유지 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 실패 알림 필터링 | `isNotificationEnabled`만 적용 | 실패 신호는 requested status와 무관하게 노출해야 함 | `enabledStatuses` 유지 |
| 설정 모델 | 변경 없음 | 필수 리뷰만 반영하는 최소 범위 수정 | failure 전용 토글 추가 |
| 테스트 격리 | `vi.resetModules()` 추가 | 기존 import 기반 테스트 패턴과 일치 | `clearAllMocks()`만 유지 |

## Implementation Todos

### Todo 1: 실패 알림 필터 semantics 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 실패 알림이 requested status 필터 때문에 누락되지 않도록 한다.
- **Work**:
  - `src/components/NotificationListener.tsx`에서 `hook-status-target-missing` 분기의 `enabledStatuses` 체크 제거
  - `isNotificationEnabled`가 켜져 있으면 실패 알림은 항상 호출되도록 정리
- **Convention Notes**: 성공 상태 변경 알림 분기는 그대로 유지하고, 실패 분기만 책임에 맞게 분리한다.
- **Verification**: NotificationListener 테스트에서 requestedStatus가 필터 밖이어도 실패 알림이 호출되는지 확인
- **Exit Criteria**: 실패 이벤트는 `isNotificationEnabled`만으로 제어된다.
- **Status**: completed

### Todo 2: route 테스트 모듈 격리 보강
- **Priority**: 2
- **Dependencies**: none
- **Goal**: hooks status route 테스트가 모듈 캐시나 테스트 순서에 의존하지 않게 한다.
- **Work**:
  - `src/app/api/hooks/status/__tests__/route.test.ts`의 `beforeEach`에 `vi.resetModules()` 추가
  - 기존 mock clear 동작은 유지
- **Convention Notes**: 이미 사용 중인 테스트 패턴을 따른다.
- **Verification**: route 테스트가 기존 기대값과 함께 통과한다.
- **Exit Criteria**: 각 테스트 시작 전에 모듈 레지스트리가 초기화된다.
- **Status**: completed

### Todo 3: 관련 테스트 기대값 갱신 및 검증
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 새 semantics에 맞는 테스트를 반영하고 회귀가 없는지 확인한다.
- **Work**:
  - `src/components/__tests__/NotificationListener.test.tsx`에서 실패 알림 관련 테스트를 새 동작에 맞게 수정/추가
  - `NODE_ENV=test pnpm test -- src/components/__tests__/NotificationListener.test.tsx src/app/api/hooks/status/__tests__/route.test.ts`
- **Convention Notes**: 테스트명은 영어, Given-When-Then 주석 유지
- **Verification**: 대상 테스트 전부 통과
- **Exit Criteria**: 리뷰 지적 두 건이 테스트로 고정된다.
- **Status**: completed

## Verification Strategy
- `NODE_ENV=test pnpm test -- src/components/__tests__/NotificationListener.test.tsx src/app/api/hooks/status/__tests__/route.test.ts`
- 필요 시 `pnpm check`로 타입 영향도 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-03-15: Plan created
- 2026-03-15: Todo 1 completed - Removed status filter from missing target notifications
- 2026-03-15: Todo 2 completed - Added module reset to hooks status route tests
- 2026-03-15: Todo 3 completed - Updated review fix tests and passed targeted verification
- 2026-03-15: Execution complete
