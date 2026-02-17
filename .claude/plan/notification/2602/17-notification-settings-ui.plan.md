# 알림 설정 UI 구현

## Business Goal
사용자가 Chrome 브라우저 알림을 세밀하게 제어할 수 있도록 한다. 알림 전역 ON/OFF와 상태별 필터링(TODO, PROGRESS, PENDING, REVIEW, DONE)을 설정하여, 원하는 상태 변경에 대해서만 알림을 받을 수 있게 한다.

## Scope
- **In Scope**: 알림 전역 토글, 상태별 필터 체크박스, ProjectSettings 패널 내 UI, AppSettings DB 저장, NotificationListener 필터링, 3개 언어 번역
- **Out of Scope**: 사운드/진동, 프로젝트별 필터링, 알림 히스토리, 알림 커스텀 메시지

## Codebase Analysis Summary
- 기존 알림 시스템: WebSocket → NotificationListener → useTaskNotification → Browser Notification
- `AppSettings` 엔티티: key-value 패턴, `getAppSetting`/`setAppSetting` 헬퍼 존재
- `ProjectSettings.tsx`: 기존 설정 패널, toggle switch 패턴(`role="switch"`, `aria-checked`) 존재
- `NotificationListener.tsx`: layout.tsx에서 마운트, WebSocket 연결 및 알림 발송
- `useTaskNotification.ts`: Browser Notification API 캡슐화
- TaskStatus: TODO, PROGRESS, PENDING, REVIEW, DONE

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/actions/appSettings.ts` | AppSettings CRUD | Modify — 알림 설정 getter/setter 추가 |
| `src/components/ProjectSettings.tsx` | 설정 패널 UI | Modify — 알림 설정 섹션 추가 |
| `src/components/NotificationListener.tsx` | 알림 수신 | Modify — 설정 props 받아 필터링 적용 |
| `src/hooks/useTaskNotification.ts` | 알림 발송 훅 | Modify — 상태 필터링 로직 추가 |
| `src/app/[locale]/layout.tsx` | 루트 레이아웃 | Modify — 알림 설정을 NotificationListener에 전달 |
| `messages/ko.json` | 한국어 번역 | Modify — 알림 설정 키 추가 |
| `messages/en.json` | 영어 번역 | Modify — 알림 설정 키 추가 |
| `messages/zh.json` | 중국어 번역 | Modify — 알림 설정 키 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석/답변은 한국어 |
| "use client" | project-architecture | 클라이언트 컴포넌트에 디렉티브 필수 |
| Boolean 네이밍 | CODE_PRINCIPLES.md | `is`, `has`, `can`, `should` 접두사 |
| AppSettings 패턴 | appSettings.ts | `getAppSetting`/`setAppSetting` 사용 |
| Toggle 패턴 | ProjectSettings.tsx | `role="switch"`, `aria-checked` |
| i18n | CLAUDE.md | 3개 언어 동시 번역, `useTranslations("settings")` |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 저장소 | AppSettings key-value | 기존 패턴 재사용, migration 불필요 | 전용 Entity |
| 키 구조 | `notification_enabled` + `notification_statuses` | 단순하고 확장 가능 | 단일 JSON |
| 설정 전달 | layout → props → NotificationListener | `revalidatePath`로 자동 반영, 기존 패턴과 일관 | API fetch |
| 기본값 | 전체 활성화 | 기존 동작 유지 (backward compatible) | 전체 비활성화 |

## Implementation Todos

### Todo 1: AppSettings에 알림 설정 server action 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 알림 설정을 DB에서 읽고 쓸 수 있는 server action을 추가한다
- **Work**:
  - `src/app/actions/appSettings.ts`에 상수 추가: `NOTIFICATION_ENABLED_KEY = "notification_enabled"`, `NOTIFICATION_STATUSES_KEY = "notification_statuses"`
  - `getNotificationSettings()` 함수 추가: `{ isEnabled: boolean, enabledStatuses: string[] }` 반환. 키 없으면 기본값 `{ isEnabled: true, enabledStatuses: ["todo","progress","pending","review","done"] }`
  - `setNotificationEnabled(enabled: boolean)` 함수 추가: `setAppSetting` 사용 + `revalidatePath("/")`
  - `setNotificationStatuses(statuses: string[])` 함수 추가: JSON.stringify로 저장 + `revalidatePath("/")`
- **Convention Notes**: `"use server"` 디렉티브, 한국어 주석, 기존 `setSidebarDefaultCollapsed` 패턴 따름
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: 4개 함수가 정상 export됨
- **Status**: pending

### Todo 2: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 알림 설정 UI에 필요한 번역 키를 3개 언어에 추가한다
- **Work**:
  - `messages/ko.json`의 `settings` 객체에 추가:
    - `"notificationSection"`: `"알림"`
    - `"notificationEnabled"`: `"브라우저 알림"`
    - `"notificationEnabledDescription"`: `"작업 상태 변경 시 브라우저 알림을 받습니다."`
    - `"notificationStatusFilter"`: `"알림 받을 상태"`
    - `"notificationStatusFilterDescription"`: `"선택한 상태로 변경될 때만 알림을 받습니다."`
  - `messages/en.json`의 `settings` 객체에 동일 키로 영어 번역 추가
  - `messages/zh.json`의 `settings` 객체에 동일 키로 중국어 번역 추가
- **Convention Notes**: 3개 언어 동시 추가, 기존 키 네이밍 패턴(`camelCase`) 따름
- **Verification**: JSON 파싱 정상
- **Exit Criteria**: 3개 파일에 5개 키씩 추가됨
- **Status**: pending

### Todo 3: NotificationListener에 설정 props 추가 및 필터링 적용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: NotificationListener가 알림 설정을 props로 받아 필터링을 적용한다
- **Work**:
  - `src/components/NotificationListener.tsx`에 props 인터페이스 추가: `{ isNotificationEnabled: boolean; enabledStatuses: string[] }`
  - `useTaskNotification` 훅에 `enabledStatuses` 전달
  - `src/hooks/useTaskNotification.ts`의 `notifyTaskStatusChanged`에 필터링 추가: `isNotificationEnabled`가 false이면 리턴, `enabledStatuses`에 포함되지 않으면 리턴
  - `src/app/[locale]/layout.tsx`에서 `getNotificationSettings()` 호출하여 props 전달
- **Convention Notes**: `"use client"` 디렉티브 유지, Boolean prop은 `is` 접두사
- **Verification**: TypeScript 컴파일 통과
- **Exit Criteria**: 설정에 따라 알림이 필터링됨
- **Status**: pending

### Todo 4: ProjectSettings에 알림 설정 UI 섹션 추가
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: ProjectSettings 패널에 알림 설정 섹션을 추가한다
- **Work**:
  - `src/components/ProjectSettings.tsx`에 props 추가: `notificationSettings: { isEnabled: boolean; enabledStatuses: string[] }`
  - "상세 페이지" 섹션 아래에 "알림" 섹션 추가
  - 전역 토글: 기존 `sidebarDefaultCollapsed` 토글 패턴 재사용 (`role="switch"`, `aria-checked`)
  - 상태별 체크박스: TODO, PROGRESS, PENDING, REVIEW, DONE 5개 체크박스
  - 전역 토글 OFF 시 체크박스 영역 비활성화 (opacity + pointer-events)
  - `startTransition` 내에서 `setNotificationEnabled`, `setNotificationStatuses` 호출
  - Board.tsx에서 notificationSettings를 ProjectSettings에 전달
- **Convention Notes**: 기존 toggle 패턴, `useTranslations("settings")`, Tailwind CSS 변수
- **Verification**: TypeScript 컴파일 통과, UI 렌더링 정상
- **Exit Criteria**: 토글과 체크박스가 정상 동작하고 DB에 저장됨
- **Status**: pending

### Todo 5: 빌드 검증
- **Priority**: 3
- **Dependencies**: Todo 3, Todo 4
- **Goal**: 전체 빌드가 정상 통과하는지 확인한다
- **Work**:
  - `pnpm build` 실행
  - TypeScript 에러 및 lint 에러 확인
  - 에러 발생 시 수정
- **Convention Notes**: N/A
- **Verification**: `pnpm build` 성공
- **Exit Criteria**: 빌드 에러 없음
- **Status**: pending

## Verification Strategy
- `pnpm build` 통과
- 알림 설정 흐름: ProjectSettings → Server Action → DB → layout.tsx → NotificationListener → 필터링
- 기존 알림 동작: 설정 미변경 시 기존과 동일하게 전체 상태에 대해 알림

## Progress Tracking
- Total Todos: 5
- Completed: 5
- Status: All todos completed

## Change Log
- 2026-02-17: Plan created
- 2026-02-17: All todos executed and verified (build + tests pass)
