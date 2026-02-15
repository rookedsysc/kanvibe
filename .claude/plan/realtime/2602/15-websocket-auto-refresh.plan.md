# WebSocket 기반 칸반 보드 자동 새로고침

## Business Goal
외부 Hook API를 통해 작업이 생성/수정되면 칸반 보드가 실시간으로 반영되어야 한다. 또한 브라우저 뒤로가기로 보드에 돌아올 때 항상 최신 데이터를 로드해야 한다.

## Scope
- **In Scope**: 보드 알림 WebSocket 채널, Hook API broadcast 연동, 클라이언트 자동 refresh 훅, 뒤로가기 시 자동 새로고침
- **Out of Scope**: 실시간 부분 업데이트(전체 refresh 방식), 서버 액션 내부 변경 알림(이미 revalidatePath로 처리됨)

## Codebase Analysis Summary
- 커스텀 서버(`server.ts`)에서 Next.js HTTP + 터미널 WebSocket 서버(port+10000) 운영
- 터미널 WS는 `/api/terminal/:taskId` 경로만 처리
- Hook API(`/api/hooks/start`, `/api/hooks/status`)는 DB 변경 후 `revalidatePath` 미호출
- Board 컴포넌트는 `initialTasks` props를 `useState`로 관리, `useEffect`로 prop 변경 시 동기화

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/boardNotifier.ts` | 보드 알림 broadcast 모듈 | Create |
| `src/hooks/useAutoRefresh.ts` | WebSocket 연결 + 뒤로가기 감지 훅 | Create |
| `server.ts` | 커스텀 서버 — WS 경로 추가 | Modify |
| `src/app/api/hooks/start/route.ts` | Hook API — broadcast 호출 추가 | Modify |
| `src/app/api/hooks/status/route.ts` | Hook API — broadcast 호출 추가 | Modify |
| `src/components/Board.tsx` | 보드 — 자동 refresh 훅 사용 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 모든 주석은 한국어 |
| 파일 배치 | project-architecture | 유틸은 `src/lib/`, 훅은 `src/hooks/` |
| 인증 패턴 | server.ts | `validateSessionFromCookie()` 사용 |
| "use client" | Next.js 컨벤션 | 클라이언트 훅 파일에 선언 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 알림 전달 방식 | 기존 WS 서버에 경로 추가 | `ws` 의존성 + 커스텀 서버 이미 존재 | SSE, Polling |
| 알림 메시지 | `{ type: "board-updated" }` 단순 신호 | `router.refresh()`로 전체 재로드하므로 payload 불필요 | Task diff 전송 |
| Broadcast 모듈 | `src/lib/boardNotifier.ts` 싱글턴 Set | API route에서 import 호출 | EventEmitter, Redis |
| 뒤로가기 감지 | `popstate` 이벤트 | 표준 브라우저 API, 안정적 | Performance Observer |

## Implementation Todos

### Todo 1: boardNotifier 모듈 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: WebSocket 클라이언트 Set을 관리하고 broadcast 기능을 제공하는 모듈 생성
- **Work**:
  - `src/lib/boardNotifier.ts` 생성
  - `boardClients: Set<WebSocket>` 전역 Set 관리
  - `addBoardClient(ws)`, `removeBoardClient(ws)`, `broadcastBoardUpdate()` 함수 export
  - `broadcastBoardUpdate()`는 연결된 모든 클라이언트에 `{ type: "board-updated" }` JSON 전송
- **Convention Notes**: 한국어 주석, 간결한 모듈
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 모듈이 생성되고 3개 함수가 export됨
- **Status**: completed

### Todo 2: useAutoRefresh 커스텀 훅 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: WebSocket 연결 + 뒤로가기 감지로 자동 새로고침하는 클라이언트 훅
- **Work**:
  - `src/hooks/useAutoRefresh.ts` 생성 (`"use client"`)
  - WebSocket 연결: `ws://{host}:{wsPort}/api/board/events`로 연결
  - 메시지 수신 시 `router.refresh()` 호출
  - `popstate` 이벤트 감지로 뒤로가기 시 `router.refresh()` 호출
  - 자동 재연결 로직 (연결 끊김 시 3초 후 재시도)
  - cleanup: unmount 시 WebSocket close + 이벤트 리스너 제거
- **Convention Notes**: `"use client"` 선언, `useRouter`는 `@/i18n/navigation`에서 import
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 훅이 생성되고 WebSocket 연결 + popstate 감지 기능이 구현됨
- **Status**: completed

### Todo 3: server.ts에 보드 알림 WebSocket 경로 추가
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 기존 WS 서버에서 `/api/board/events` 경로를 처리하여 boardNotifier에 클라이언트 등록
- **Work**:
  - `server.ts`의 `wss.on("connection")` 핸들러 수정
  - `/api/board/events` 경로 매칭 추가 (기존 터미널 경로 앞에)
  - 인증 확인 (`validateSessionFromCookie`)
  - 인증 성공 시 `addBoardClient(ws)`, close 시 `removeBoardClient(ws)`
  - 기존 터미널 로직은 그대로 유지
- **Convention Notes**: 기존 코드 패턴 유지, `boardNotifier` import 추가
- **Verification**: 서버 빌드 확인
- **Exit Criteria**: WS 서버가 두 경로(`/api/terminal/:id`, `/api/board/events`)를 모두 처리
- **Status**: completed

### Todo 4: Hook API routes에 broadcast 호출 추가
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Hook API가 DB 변경 후 연결된 보드 클라이언트에 알림 전송 + revalidatePath 추가
- **Work**:
  - `src/app/api/hooks/start/route.ts`: 성공 응답 전 `broadcastBoardUpdate()` 호출 + `revalidatePath("/[locale]", "page")` 추가
  - `src/app/api/hooks/status/route.ts`: 성공 응답 전 `broadcastBoardUpdate()` 호출 + `revalidatePath("/[locale]", "page")` 추가
  - 두 파일에 `broadcastBoardUpdate` import 추가
- **Convention Notes**: 기존 import 패턴 유지
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 두 API route가 성공 시 broadcast + revalidatePath 호출
- **Status**: completed

### Todo 5: Board 컴포넌트에 useAutoRefresh 적용
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: Board 컴포넌트에서 자동 새로고침 훅 활성화
- **Work**:
  - `src/components/Board.tsx`에 `useAutoRefresh` import 및 호출
  - Board 함수 최상단에 `useAutoRefresh()` 추가
- **Convention Notes**: 최소한의 변경, 기존 코드에 영향 없음
- **Verification**: 빌드 확인
- **Exit Criteria**: Board 컴포넌트가 WebSocket 연결 + 뒤로가기 자동 새로고침 활성화
- **Status**: completed

## Verification Strategy
- TypeScript 빌드: `npx next build` (또는 `npx tsc --noEmit`)
- 수동 테스트: Hook API 호출 → 보드 자동 갱신 확인
- 수동 테스트: 태스크 상세 → 뒤로가기 → 최신 데이터 확인

## Progress Tracking
- Total Todos: 5
- Completed: 5
- Status: Execution complete

## Change Log
- 2026-02-15: Plan created
- 2026-02-15: All todos completed
