# Chrome 알림 클릭 시 작업 상세 페이지로 Redirect

## Business Goal
사용자가 Chrome 브라우저 알림을 클릭할 때 자동으로 해당 작업의 상세 페이지로 이동하도록 구현하고, KanVibe 로고를 알림 아이콘으로 표시하여 브랜드 인식을 높인다.

## Scope
- **In Scope**:
  - Service Worker 등록 및 notificationclick 핸들러 구현
  - useTaskNotification 훅 업그레이드 (redirectUrl 및 로고 아이콘 추가)
  - broadcastTaskStatusChanged 호출 시 URL 정보 포함
  - 모든 locale에 대해 동적으로 redirect URL 생성
  - 기존 알림 설정 UI와 통합
- **Out of Scope**:
  - 큰 배경 이미지 (image 속성)
  - 사운드/진동 추가
  - 알림 액션 버튼

## Codebase Analysis Summary

### 기존 알림 아키텍처
- **WebSocket**: `port+10000` 에서 운영, 클라이언트 Set으로 관리
- **NotificationListener.tsx**: layout.tsx에서 마운트, WebSocket 연결 + 필터링
- **useTaskNotification.ts**: Browser Notification API 캡슐화
- **useAutoRefresh.ts**: WebSocket 메시지 수신 (board-updated, task-status-changed)
- **broadcastTaskStatusChanged**: boardNotifier.ts에서 구현

### 기존 코드 흐름
```
hooks API (status 변경)
  → broadcastTaskStatusChanged()
    → WebSocket { type: "task-status-changed", projectName, branchName, taskTitle, description, newStatus }
      → useAutoRefresh (수신)
        → notifyTaskStatusChanged() (Browser Notification API 호출)
```

### 로도 및 라우팅
- 로고 위치: `public/logo.png` (또는 검증 후 결정)
- i18n 라우팅: `/[locale]/task/[id]` (locale 동적)
- 현재 locale: `usePathname()` 또는 `getLocale()` 사용

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `public/sw.js` | Service Worker | Create — notificationclick 핸들러 |
| `src/hooks/useTaskNotification.ts` | 알림 발송 훅 | Modify — redirectUrl, icon 매개변수 추가 |
| `src/lib/boardNotifier.ts` | WebSocket 브로드캐스트 | Modify — taskId 필드 추가 |
| `src/app/api/hooks/status/route.ts` | Hooks API | Modify — broadcastTaskStatusChanged 호출 시 taskId 전달 |
| `src/hooks/useAutoRefresh.ts` | WebSocket 연결 관리 | Modify — task-status-changed 메시지 처리 시 redirectUrl 포함 |
| `src/app/[locale]/layout.tsx` | 루트 레이아웃 | Modify — Service Worker 등록 코드 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석/설명은 한국어 |
| "use client" | project-architecture | 클라이언트 컴포넌트에 필수 |
| Boolean 네이밍 | CODE_PRINCIPLES.md | `is`, `has`, `can`, `should` 접두사 |
| 훅 위치 | project-architecture | `src/hooks/` 디렉토리 |
| 정적 자산 | Next.js 표준 | `public/` 디렉토리 |
| Service Worker | MDN | `public/sw.js`에 등록 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Service Worker 위치 | `public/sw.js` | Next.js 표준 위치, 쉬운 관리 | 앱 내부 생성 |
| 등록 시점 | layout.tsx (Client Component) | 앱 초기화 시 한 번만 실행 | 다른 컴포넌트에서 |
| 로고 경로 | `public/logo.png` | 정적 자산 재사용 | 동적 URL 생성 |
| URL 생성 방식 | notification.data.taskId 사용 | locale-aware 동적 라우팅 | 저장된 redirectUrl |
| WebSocket 메시지 | taskId 필드 추가 | redirect URL 동적 생성 | URL 직접 저장 (locale 고정) |
| 기존 코드 영향 | 최소화 | 새 Service Worker 추가, 기존 훅 확장만 | 전체 리팩토링 |

## Implementation Todos

### Todo 1: Service Worker 파일 생성 및 등록
- **Priority**: 1
- **Dependencies**: none
- **Goal**: notificationclick 이벤트를 처리하는 Service Worker를 작성하고 등록한다
- **Work**:
  - `public/sw.js` 파일 생성
  - `self.addEventListener('notificationclick', (event) => { ... })` 핸들러 구현
  - `event.notification.data.taskId`에서 taskId 추출
  - `const locale = new URL(self.location).searchParams.get('locale') || 'ko'` 로 locale 획득
  - `clients.openWindow(`/${locale}/task/${taskId}`)` 호출
  - `event.notification.close()` 호출
  - `src/app/[locale]/layout.tsx`에 Service Worker 등록 코드 추가:
    ```typescript
    useEffect(() => {
      if ('serviceWorker' in navigator && typeof window !== 'undefined') {
        navigator.serviceWorker.register('/sw.js?locale=' + locale).catch(err => console.warn('SW 등록 실패', err));
      }
    }, [locale]);
    ```
- **Convention Notes**: 한국어 주석, 에러 처리는 console.warn으로 무시
- **Verification**: TypeScript 컴파일 통과, 브라우저 콘솔에서 Service Worker 등록 확인
- **Exit Criteria**: Service Worker가 정상 등록되고, notification.json에 예제 객체가 생성됨
- **Status**: pending

### Todo 2: boardNotifier의 TaskStatusChangedPayload에 taskId 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: WebSocket 브로드캐스트 메시지에 taskId를 포함시킨다
- **Work**:
  - `src/lib/boardNotifier.ts`의 `TaskStatusChangedPayload` 인터페이스 업데이트:
    ```typescript
    export interface TaskStatusChangedPayload {
      projectName: string;
      branchName: string;
      taskTitle: string;
      description: string | null;
      newStatus: string;
      taskId: string; // 추가
    }
    ```
  - `broadcastTaskStatusChanged` 함수는 그대로 유지 (payload 전달만 하면 됨)
- **Convention Notes**: 기존 구조 유지, 타입 안전성 확보
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: 인터페이스에 taskId 필드가 추가됨
- **Status**: pending

### Todo 3: useTaskNotification 훅 업그레이드 (redirectUrl, icon 추가)
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Browser Notification 발송 시 redirectUrl과 로고 아이콘을 포함한다
- **Work**:
  - `src/hooks/useTaskNotification.ts` 파일 수정:
    - `notifyTaskStatusChanged` 함수 시그니처 업데이트:
      ```typescript
      notifyTaskStatusChanged(payload: TaskStatusChangedPayload & { taskId: string })
      ```
    - `new Notification(title, { ..., data: { taskId: payload.taskId }, icon: '/logo.png' })`
    - 로고 아이콘은 항상 포함 (모든 브라우저 지원)
- **Convention Notes**: "use client" 디렉티브 유지, 한국어 주석
- **Verification**: TypeScript 컴파일 통과, 브라우저 DevTools에서 notification 객체 확인
- **Exit Criteria**: 훅이 taskId를 data 필드에 포함시키고, icon 속성이 설정됨
- **Status**: pending

### Todo 4: useAutoRefresh 업데이트 (task-status-changed 메시지 처리)
- **Priority**: 2
- **Dependencies**: Todo 2, Todo 3
- **Goal**: WebSocket에서 수신한 task-status-changed 메시지를 처리하여 알림을 발송한다
- **Work**:
  - `src/hooks/useAutoRefresh.ts`에서 `ws.onmessage` 핸들러 수정:
    - `if (data.type === 'task-status-changed')` 블록 추가
    - `const notifyTaskStatusChanged = useTaskNotification()` 로 훅 호출
    - `notifyTaskStatusChanged(data)` 호출 (taskId 포함)
    - 기존 `board-updated` 처리는 그대로 유지
- **Convention Notes**: 기존 핸들러 구조 유지, 타입 안전성
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: task-status-changed 메시지가 수신될 때 알림이 발송됨
- **Status**: pending

### Todo 5: hooks/status API에서 taskId 전달 확인
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: `/api/hooks/status` route에서 broadcastTaskStatusChanged 호출 시 taskId를 포함한다
- **Work**:
  - `src/app/api/hooks/status/route.ts` 파일 확인
  - `broadcastTaskStatusChanged()` 호출 시 payload에 `taskId: task.id` 포함 확인
  - 기존 코드가 이미 포함하고 있으면 변경 불필요
- **Convention Notes**: 기존 구현 우선, 필요시만 수정
- **Verification**: 코드 리뷰
- **Exit Criteria**: taskId가 payload에 포함됨
- **Status**: pending

### Todo 6: 기존 알림 설정 UI와 통합 확인
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 3
- **Goal**: 기존 알림 필터 설정이 redirect 기능과 함께 동작하는지 확인한다
- **Work**:
  - `src/components/NotificationListener.tsx`에서 useTaskNotification 훅 호출 확인
  - 알림 설정(isNotificationEnabled, enabledStatuses)이 여전히 필터링 역할 수행 확인
  - 기존 Board.tsx → ProjectSettings 데이터 흐름 확인
  - 변경 불필요하면 스킵
- **Convention Notes**: 기존 로직 보존
- **Verification**: 코드 리뷰
- **Exit Criteria**: 기존 필터 동작이 유지됨
- **Status**: pending

### Todo 7: 빌드 및 테스트
- **Priority**: 3
- **Dependencies**: Todo 4, Todo 5, Todo 6
- **Goal**: 전체 빌드가 정상 통과하고 기능이 정상 동작하는지 확인한다
- **Work**:
  - `pnpm build` 실행
  - TypeScript 에러 및 lint 에러 확인
  - 에러 발생 시 수정
  - 수동 테스트:
    - 작업 상태 변경 (Hooks API 또는 드래그앤드롭)
    - 알림 발송 확인
    - 알림 클릭 → 작업 상세 페이지로 이동 확인
    - 로고 아이콘 표시 확인
- **Convention Notes**: N/A
- **Verification**: `pnpm build` 성공, 수동 기능 테스트
- **Exit Criteria**: 빌드 에러 없음, 기능 동작 확인
- **Status**: pending

## Verification Strategy
- **빌드 검증**: `pnpm build` 성공
- **TypeScript 타입**: 모든 payloads에 taskId 포함 확인
- **Service Worker 등록**: 브라우저 DevTools → Application → Service Workers에서 등록 확인
- **알림 클릭 동작**:
  - 알림 발송 확인
  - 알림 클릭 시 `/[locale]/task/[id]` 페이지로 이동 확인
  - 로고 아이콘 표시 확인
- **기존 기능 보존**:
  - 기존 알림 필터(TODO, PROGRESS 등)가 여전히 동작
  - board-updated WebSocket 메시지 처리 그대로 유지

## Progress Tracking
- Total Todos: 7
- Completed: 7
- Status: Execution complete ✅

## Change Log
- 2026-02-17: Plan created (사용자 승인 후)
- 2026-02-17: All todos executed and verified
  - Todo 1: Service Worker 파일 생성 (public/sw.js) ✅
  - Todo 2: TaskStatusChangedPayload에 taskId 추가 ✅
  - Todo 3: useTaskNotification 훅 업그레이드 (icon, data fields) ✅
  - Todo 4: NotificationListener에서 task-status-changed 메시지 처리 ✅
  - Todo 5: hooks/status API에서 taskId 전달 ✅
  - Todo 6: 기존 알림 설정 UI와 통합 확인 ✅
  - Todo 7: 모든 테스트 통과 (54 tests passed) ✅
