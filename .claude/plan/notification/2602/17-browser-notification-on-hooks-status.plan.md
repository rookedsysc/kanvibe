# Hooks 상태 변경 시 브라우저 알림

## Business Goal
Claude Code hooks가 task 상태를 변경할 때 브라우저 알림을 발송하여, 사용자가 KanVibe 보드를 보고 있지 않아도 AI 에이전트의 작업 상태 변화를 즉시 인지할 수 있도록 한다.

## Scope
- **In Scope**: `/api/hooks/status` API 경유 상태 변경 시 Browser Notification 발송
- **Out of Scope**: 수동 드래그앤드롭/UI 상태 변경 알림, 알림 설정 UI, 사운드/진동

## Codebase Analysis Summary
- Claude Code hooks (`src/lib/claudeHooksSetup.ts`)가 `/api/hooks/status`로 POST 요청
- `/api/hooks/status/route.ts`에서 DB 업데이트 후 `broadcastBoardUpdate()` 호출
- `broadcastBoardUpdate()` (`src/lib/boardNotifier.ts`)가 WebSocket으로 `{ type: "board-updated" }` 전송
- `useAutoRefresh` (`src/hooks/useAutoRefresh.ts`)가 WebSocket 메시지 수신 시 `router.refresh()` 호출
- 기존 WebSocket: port+10000에서 운영, `boardClients` Set으로 클라이언트 관리

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/boardNotifier.ts` | WebSocket 브로드캐스트 | Modify — 상세 정보 포함 브로드캐스트 함수 추가 |
| `src/app/api/hooks/status/route.ts` | hooks 상태 변경 API | Modify — task 상세 정보 브로드캐스트 |
| `src/hooks/useTaskNotification.ts` | 브라우저 알림 훅 | Create |
| `src/hooks/useAutoRefresh.ts` | WebSocket 연결 관리 | Modify — 알림 메시지 처리 추가 |
| `src/components/Board.tsx` | 메인 보드 컴포넌트 | Modify — 알림 훅 통합 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석/답변은 한국어 |
| "use client" | project-architecture | 클라이언트 컴포넌트에 디렉티브 필수 |
| 훅 위치 | 프로젝트 구조 | `src/hooks/` 디렉토리 |
| Boolean 네이밍 | CODE_PRINCIPLES.md | `is`, `has`, `can`, `should` 접두사 |
| KISS | CODE_PRINCIPLES.md | 단순하고 명확한 코드, 불필요한 복잡성 없음 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 알림 트리거 | hooks API 경유 시에만 | 사용자 요청 범위 | 모든 상태 변경 |
| 전달 채널 | 기존 WebSocket 확장 | 인프라 재사용, 추가 의존성 없음 | SSE, polling |
| 메시지 분리 | 새 타입 `task-status-changed` | 기존 `board-updated` 동작 유지 | 기존 타입에 data 추가 |
| 권한 요청 시점 | Board 마운트 시 | 사용자가 보드 진입 시 자연스럽게 요청 | 설정 페이지 |

## Implementation Todos

### Todo 1: boardNotifier에 상세 브로드캐스트 함수 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: WebSocket으로 task 상태 변경 상세 정보를 전송할 수 있도록 한다
- **Work**:
  - `src/lib/boardNotifier.ts`에 `TaskStatusChangedPayload` 인터페이스 추가: `{ projectName: string; branchName: string; taskTitle: string; description: string | null; newStatus: string }`
  - `broadcastTaskStatusChanged(payload: TaskStatusChangedPayload)` 함수 추가
  - 메시지 형식: `{ type: "task-status-changed", ...payload }`
- **Convention Notes**: export 함수, 한국어 JSDoc 주석
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: `broadcastTaskStatusChanged` 함수가 정상 export됨
- **Status**: pending

### Todo 2: hooks/status API에서 상세 브로드캐스트 호출
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: hooks API가 상태 변경 시 task 상세 정보를 WebSocket으로 브로드캐스트한다
- **Work**:
  - `src/app/api/hooks/status/route.ts`에서 `broadcastTaskStatusChanged` import
  - `broadcastBoardUpdate()` 호출 직후 `broadcastTaskStatusChanged()` 호출
  - payload: `{ projectName, branchName, taskTitle: task.title, description: task.description, newStatus: taskStatus }`
- **Convention Notes**: 기존 `broadcastBoardUpdate()` 호출은 그대로 유지
- **Verification**: TypeScript 컴파일 통과, API 동작 확인
- **Exit Criteria**: hooks API가 두 가지 브로드캐스트 모두 수행

### Todo 3: useTaskNotification 커스텀 훅 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Browser Notification API를 캡슐화한 커스텀 훅을 만든다
- **Work**:
  - `src/hooks/useTaskNotification.ts` 파일 생성
  - `useTaskNotification()` 훅 구현:
    - 마운트 시 `Notification.requestPermission()` 호출
    - `notifyTaskStatusChanged(payload)` 콜백 함수 반환
    - 알림 title: `"{projectName} — {branchName}"`
    - 알림 body: `"{taskTitle}: {newStatus}로 변경"` (description이 있으면 함께 표시)
    - `Notification.permission === "granted"` 일 때만 발송
- **Convention Notes**: `"use client"` 디렉티브, 한국어 주석, `is`/`has` 접두사로 boolean
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: 훅이 정상 export되고 Notification API 호출 로직이 포함됨

### Todo 4: useAutoRefresh에 알림 통합
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 3
- **Goal**: WebSocket에서 `task-status-changed` 메시지 수신 시 브라우저 알림을 발송한다
- **Work**:
  - `src/hooks/useAutoRefresh.ts`에서 `useTaskNotification` import
  - `ws.onmessage` 핸들러에 `task-status-changed` 타입 처리 추가
  - `notifyTaskStatusChanged(data)` 호출
- **Convention Notes**: 기존 `board-updated` 처리 로직은 그대로 유지
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: WebSocket 메시지 수신 시 브라우저 알림이 트리거됨

### Todo 5: 빌드 검증
- **Priority**: 3
- **Dependencies**: Todo 2, Todo 4
- **Goal**: 전체 빌드가 정상 통과하는지 확인한다
- **Work**:
  - `pnpm build` 실행
  - TypeScript 에러 및 lint 에러 확인
  - 에러 발생 시 수정
- **Convention Notes**: N/A
- **Verification**: `pnpm build` 성공
- **Exit Criteria**: 빌드 에러 없음

## Verification Strategy
- `pnpm build` 통과
- 코드 리뷰: 기존 `board-updated` 동작이 그대로 유지되는지 확인
- 알림 흐름: hooks API → broadcastTaskStatusChanged → WebSocket → useAutoRefresh → useTaskNotification → Browser Notification

## Progress Tracking
- Total Todos: 5
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-17: Plan created
