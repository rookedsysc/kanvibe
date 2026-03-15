# Notify on Missing Hook Target

## Business Goal
Hook 상태 변경 요청에서 `branchName` 또는 `projectName`으로 대상을 찾지 못해도 사용자에게는 즉시 상황을 알리고, 실제 태스크 상태는 잘못 변경되지 않도록 하여 운영 혼선을 줄인다.

## Scope
- **In Scope**: `/api/hooks/status`의 project/task 미존재 시 전용 브로드캐스트 추가, 브라우저 알림 수신 처리, 관련 테스트 및 Hook API 문서 업데이트
- **Out of Scope**: Hook 설치 스크립트 수정, DB 스키마 변경, 알림 설정 UI 변경, 404 응답 정책 변경

## Codebase Analysis Summary
- `src/app/api/hooks/status/route.ts`는 `branchName`, `projectName`, `status`를 검증한 뒤 project와 task를 조회하고, 성공 시에만 상태 저장과 `broadcastBoardUpdate`, `broadcastTaskStatusChanged`를 호출한다.
- `src/lib/boardNotifier.ts`는 `board-updated`, `task-status-changed` 메시지를 내부 broadcast endpoint로 전달하는 단일 진입점이다.
- `src/components/NotificationListener.tsx`는 WebSocket 메시지 중 `task-status-changed`만 받아 `useTaskNotification`으로 브라우저 알림을 띄운다.
- `src/hooks/useTaskNotification.ts`는 `taskId`와 `locale`을 포함한 payload를 전제로 Service Worker 알림을 생성한다.
- Hook API 변경은 `README.md`, `docs/README.ko.md`, `docs/README.zh.md`를 함께 수정해야 한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/boardNotifier.ts` | 실시간 브로드캐스트 유틸 | Modify |
| `src/app/api/hooks/status/route.ts` | Hook 상태 변경 API | Modify |
| `src/components/NotificationListener.tsx` | 브라우저 알림 수신 진입점 | Modify |
| `src/hooks/useTaskNotification.ts` | 브라우저 알림 표시 훅 | Modify |
| `src/components/__tests__/NotificationListener.test.tsx` | 알림 수신 테스트 | Modify |
| `src/app/api/hooks/status/__tests__/route.test.ts` | Hook 상태 API 테스트 | Create |
| `README.md` | 영문 사용자 문서 | Modify |
| `docs/README.ko.md` | 국문 사용자 문서 | Modify |
| `docs/README.zh.md` | 중문 사용자 문서 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | `CODE_PRINCIPLES.md` | 주석/JSDoc은 한국어로 유지 |
| KISS/YAGNI | `CODE_PRINCIPLES.md` | 전용 이벤트만 추가하고 불필요한 추상화는 피한다 |
| Hook API 문서 동시 수정 | `CLAUDE.md` | Hook API 변경 시 README 3종을 함께 갱신한다 |
| 테스트 스타일 | `CLAUDE.md` | Vitest, 영어 테스트명, Given-When-Then 주석 패턴을 유지한다 |
| 기존 WebSocket 패턴 유지 | 기존 `boardNotifier.ts`, `NotificationListener.tsx` | 기존 `board-updated`, `task-status-changed` 흐름은 깨지지 않게 유지한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 미매칭 알림 채널 | 전용 broadcast type 추가 | 기존 `task-status-changed` 의미와 `taskId` 전제를 보존한다 | 기존 이벤트에 nullable `taskId` 추가 |
| API 응답 | `404` 유지 | 사용자 요구사항과 현재 호출자 기대를 유지한다 | `200` 또는 `202` 반환 |
| 알림 클릭 동작 | task detail 이동 정보 없이 표시 | 실제 task가 없으므로 잘못된 이동을 방지한다 | 임시 경로 생성 |
| 브로드캐스트 범위 | project 미존재, task 미존재 두 경우 모두 알림 | 운영자가 어떤 lookup이 실패했는지 바로 인지 가능 | task 미존재만 알림 |

## API Contracts

### POST /api/hooks/status
- Headers: `Content-Type: application/json`
- Request: `{"branchName": string, "projectName": string, "status": "todo" | "progress" | "pending" | "review" | "done"}`
- Response: `{"success": true, "data": {...}} | {"success": false, "error": string}`
- Note: project 또는 task가 없으면 상태 저장은 수행하지 않지만, 전용 실시간 알림을 브로드캐스트한 뒤 기존처럼 `404`를 반환한다.

## Data Models (if applicable)
해당 없음.

## Implementation Todos

### Todo 1: 미매칭 Hook 알림 브로드캐스트 계약 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: project/task lookup 실패를 위한 전용 실시간 이벤트를 정의한다.
- **Work**:
  - `src/lib/boardNotifier.ts`에 미매칭 알림 payload 타입과 broadcast 함수 추가
  - payload에 `projectName`, `branchName`, `requestedStatus`, `reason`을 포함
  - 메시지 형식을 `{ type: "hook-status-target-missing", ...payload }`로 고정
- **Convention Notes**: 기존 `sendBroadcast` 사용 패턴을 재사용하고, export 타입/함수 이름은 역할이 드러나도록 작성한다.
- **Verification**: `src/lib/__tests__/boardNotifier.test.ts`와 타입 체크가 새 이벤트를 수용하는지 확인
- **Exit Criteria**: route와 클라이언트가 재사용할 수 있는 전용 broadcast 함수가 export된다.
- **Status**: completed

### Todo 2: hooks/status API에 404 유지 + 미매칭 알림 추가
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: project 또는 task를 못 찾아도 상태 변경 없이 알림만 발송하도록 API를 조정한다.
- **Work**:
  - `src/app/api/hooks/status/route.ts`에서 project 미존재 분기와 task 미존재 분기에 전용 broadcast 호출 추가
  - 성공 분기의 상태 저장, `broadcastBoardUpdate`, `broadcastTaskStatusChanged` 동작은 유지
  - `src/app/api/hooks/status/__tests__/route.test.ts`를 추가해 project/task 미존재 시 브로드캐스트와 `404`를 검증
- **Convention Notes**: 성공 케이스와 실패 케이스의 책임을 분리하고, 상태 저장 로직은 lookup 성공 뒤에만 실행한다.
- **Verification**: 대상 route 테스트가 `404`와 브로드캐스트 호출을 모두 검증한다.
- **Exit Criteria**: lookup 실패 시 DB save 없이 전용 broadcast가 호출되고 응답 상태는 `404`다.
- **Status**: completed

### Todo 3: 브라우저 알림 수신 경로를 미매칭 이벤트까지 확장
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 2
- **Goal**: `hook-status-target-missing` 메시지를 브라우저 알림으로 표시한다.
- **Work**:
  - `src/hooks/useTaskNotification.ts`에 미매칭 알림용 payload/표시 함수 추가
  - `src/components/NotificationListener.tsx`에서 새 메시지 타입을 수신해 알림 설정을 통과하면 전용 알림 호출
  - `src/components/__tests__/NotificationListener.test.tsx`에 새 이벤트 처리 테스트 추가
- **Convention Notes**: 기존 `task-status-changed` 흐름을 깨지 말고, 이동 정보가 없는 알림은 `taskId` 없이 별도 처리한다.
- **Verification**: NotificationListener 테스트에서 새 이벤트 수신 시 전용 알림 함수 호출 여부를 확인한다.
- **Exit Criteria**: WebSocket으로 새 이벤트를 받으면 사용자가 미매칭 상황을 브라우저 알림으로 인지할 수 있다.
- **Status**: completed

### Todo 4: Hook API 문서와 최종 검증 반영
- **Priority**: 4
- **Dependencies**: Todo 2, Todo 3
- **Goal**: 변경된 Hook API 동작을 문서화하고 관련 검증을 완료한다.
- **Work**:
  - `README.md`, `docs/README.ko.md`, `docs/README.zh.md`의 `/api/hooks/status` 설명에 미매칭 시 알림 발송 + 404 유지 동작을 반영
  - 관련 테스트 명령을 실행하고 결과를 확인
- **Convention Notes**: 세 문서의 의미를 맞추되 각 언어 표현은 자연스럽게 유지한다.
- **Verification**: 대상 테스트 통과, 문서 설명 일관성 확인
- **Exit Criteria**: README 3종이 동일한 동작을 설명하고, 구현 검증이 완료된다.
- **Status**: completed

## Verification Strategy
- `NODE_ENV=test pnpm test -- src/app/api/hooks/status/__tests__/route.test.ts`
- `NODE_ENV=test pnpm test -- src/components/__tests__/NotificationListener.test.tsx`
- 필요 시 관련 기존 테스트(`src/lib/__tests__/boardNotifier.test.ts`)도 함께 확인
- 문서에서 `/api/hooks/status` 설명이 영/한/중에서 동일 의미인지 수동 검토

## Progress Tracking
- Total Todos: 4
- Completed: 4
- Status: Execution complete

## Change Log
- 2026-03-15: Plan created
- 2026-03-15: Todo 1 completed - Added dedicated broadcast contract for missing hook targets
- 2026-03-15: Todo 2 completed - Kept 404 responses while broadcasting missing project/task events
- 2026-03-15: Todo 3 completed - Extended browser notifications for missing hook target events
- 2026-03-15: Todo 4 completed - Updated Hook API docs and passed targeted tests plus type check
- 2026-03-15: Execution complete
