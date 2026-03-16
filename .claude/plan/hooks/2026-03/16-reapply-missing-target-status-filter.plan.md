# Reapply Missing-Target Status Filter

## Business Goal
사용자가 비활성화한 상태에 대해서는 Hook 대상 누락 실패 알림도 함께 차단해 알림 설정 의미를 일관되게 유지하고 불필요한 운영 알림 노이즈를 줄인다.

## Scope
- **In Scope**: `hook-status-target-missing` 브라우저 알림에 `requestedStatus` 필터 재적용, `NotificationListener` 테스트 기대값 수정, 관련 회귀 검증
- **Out of Scope**: 실패 알림 전용 설정 UI 추가, Hook API 응답/브로드캐스트 계약 변경, 알림 저장소 스키마 변경

## Codebase Analysis Summary
- `src/components/NotificationListener.tsx`는 WebSocket 이벤트에서 `task-status-changed`와 `hook-status-target-missing`를 분기 처리한다.
- 성공 알림은 이미 `enabledStatuses`를 따르지만, 현재 실패 알림 분기는 `isNotificationEnabled`만 확인해 비활성 상태에도 알림을 노출한다.
- `src/components/__tests__/NotificationListener.test.tsx`에는 현재 이 동작을 고정하는 회귀 테스트가 있으며, 이번 작업에서는 그 기대값을 정책에 맞게 뒤집어야 한다.
- `src/app/api/hooks/status/__tests__/route.test.ts`는 브로드캐스트 payload 회귀를 확인하는 참조 테스트로 유지하고, 계약 변경이 없음을 함께 검증한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/NotificationListener.tsx` | WebSocket 알림 필터링 진입점 | Modify |
| `src/components/__tests__/NotificationListener.test.tsx` | missing-target 필터 회귀 테스트 | Modify |
| `src/app/api/hooks/status/__tests__/route.test.ts` | hooks status route 계약 회귀 확인 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 최소 수정 | `CODE_PRINCIPLES.md` | 상태 필터 재적용만 수행하고 새 설정 모델은 추가하지 않는다 |
| 테스트 스타일 | `CLAUDE.md` | Vitest, 영어 테스트명, Given-When-Then 주석 패턴 유지 |
| 기존 성공 알림 의미 보존 | 현재 `NotificationListener.tsx` 패턴 | `task-status-changed` 분기는 건드리지 않고 missing-target 분기만 수정한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 실패 알림 필터 기준 | `requestedStatus`를 `enabledStatuses`와 비교 | 사용자 설정 의미를 실패 알림에도 동일하게 적용 | 실패 알림 항상 노출 |
| 설정 모델 | 변경 없음 | 범위를 최소화하고 기존 저장 포맷을 유지 | failure 전용 토글 추가 |
| 검증 범위 | NotificationListener + hooks status route 테스트 | UI 정책과 브로드캐스트 계약을 함께 회귀 검증 | NotificationListener 테스트만 실행 |

## Implementation Todos

### Todo 1: missing-target 분기 현재 동작 확인
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 재적용이 필요한 실제 동작과 테스트 기대값을 확인한다.
- **Work**:
  - `src/components/NotificationListener.tsx`의 `hook-status-target-missing` 분기를 확인한다.
  - `src/components/__tests__/NotificationListener.test.tsx`의 missing-target 회귀 테스트 기대값을 확인한다.
- **Convention Notes**: 관련 파일만 읽고 범위를 확장하지 않는다.
- **Verification**: 수정 대상 분기와 테스트가 실제로 존재함을 확인한다.
- **Exit Criteria**: 필터 재적용 위치와 영향 범위가 확정된다.
- **Status**: completed

### Todo 2: missing-target 상태 필터 재적용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 비활성 상태에 대한 missing-target 알림이 전송되지 않도록 한다.
- **Work**:
  - `src/components/NotificationListener.tsx`의 `hook-status-target-missing` 분기에서 `enabledStatuses.includes(data.requestedStatus)` 체크를 추가한다.
  - `settingsRef` 패턴과 `isNotificationEnabled` 체크는 유지한다.
- **Convention Notes**: 기존 stale-closure 방지용 `settingsRef` 구조를 그대로 사용한다.
- **Verification**: disabled status 시 알림 함수가 호출되지 않아야 한다.
- **Exit Criteria**: missing-target 실패 알림이 status 설정을 따른다.
- **Status**: completed

### Todo 3: 회귀 테스트 기대값 갱신 및 검증
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 변경된 정책을 테스트로 고정하고 브로드캐스트 계약 회귀가 없는지 확인한다.
- **Work**:
  - `src/components/__tests__/NotificationListener.test.tsx`의 missing-target 테스트명을 정책에 맞게 수정한다.
  - disabled status 케이스의 기대값을 `not.toHaveBeenCalled()`로 변경한다.
  - `NODE_ENV=test pnpm exec vitest run src/components/__tests__/NotificationListener.test.tsx src/app/api/hooks/status/__tests__/route.test.ts`를 실행한다.
- **Convention Notes**: 테스트명은 영어, Given-When-Then 주석 유지.
- **Verification**: 대상 테스트 2개 파일이 모두 통과한다.
- **Exit Criteria**: 정책 변경이 테스트로 고정되고 관련 회귀 검증이 통과한다.
- **Status**: completed

## Verification Strategy
- `NODE_ENV=test pnpm exec vitest run src/components/__tests__/NotificationListener.test.tsx src/app/api/hooks/status/__tests__/route.test.ts`
- `task-status-changed` 성공 알림 경로와 `hook-status-target-missing` 실패 알림 경로가 각각 기대한 필터를 따르는지 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-03-16: Plan created
- 2026-03-16: Todo 1 completed - Confirmed missing-target branch and regression test expectations
- 2026-03-16: Todo 2 completed - Reapplied requestedStatus filtering in NotificationListener
- 2026-03-16: Todo 3 completed - Updated regression test and passed targeted verification
- 2026-03-16: Execution complete
