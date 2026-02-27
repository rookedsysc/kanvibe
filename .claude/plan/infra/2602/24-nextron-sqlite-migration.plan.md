# Nextron + SQLite Migration

## Business Goal
KanVibe를 Electron 데스크탑 앱으로 전환하여 Docker/PostgreSQL 없이 단독 실행 가능하게 만든다. better-sqlite3로 내장 DB를 사용하고, Electron IPC로 데이터 통신하여 네이티브 성능을 확보한다. electron-updater로 자동 업데이트를 지원한다.

## Scope
- **In Scope**: Electron main/renderer 구조 생성, PostgreSQL→SQLite 전환, Server Actions→IPC 전환, Server Components→Client Components 전환, 동적 라우트→Query Param 전환, 인증 제거, hooks API용 미니 HTTP 서버, 터미널 WebSocket 서버, electron-builder 패키징, electron-updater 자동 업데이트, Docker 의존성 제거
- **Out of Scope**: 다중 윈도우, 트레이 아이콘, 기존 vitest 테스트 마이그레이션

## Codebase Analysis Summary
현재 Next.js 16 App Router + Custom Server(server.ts) + PostgreSQL(TypeORM) 아키텍처. Server Actions 6개 파일(~55개 함수), API Routes 3개, Server Components 4개 페이지. WebSocket 서버(터미널 + 보드 알림)는 server.ts에서 별도 포트로 운영. 환경변수 기반 인증(쿠키 세션).

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `main/background.ts` | Electron main process entry | Create |
| `main/preload.ts` | IPC bridge (contextBridge) | Create |
| `main/database.ts` | TypeORM SQLite DataSource | Create |
| `main/ipc/kanban.ts` | Kanban IPC handlers | Create |
| `main/ipc/project.ts` | Project IPC handlers | Create |
| `main/ipc/appSettings.ts` | AppSettings IPC handlers | Create |
| `main/ipc/paneLayout.ts` | PaneLayout IPC handlers | Create |
| `main/ipc/diff.ts` | Diff IPC handlers | Create |
| `main/ipc/terminal.ts` | Terminal WS + IPC handlers | Create |
| `main/ipc/hooks.ts` | Hooks mini HTTP server | Create |
| `main/helpers/create-window.ts` | BrowserWindow factory | Create |
| `renderer/` | Next.js app (current src/) | Migrate |
| `renderer/next.config.ts` | Static export config | Modify |
| `src/entities/*.ts` | TypeORM entities | Modify (enum→simple-enum, uuid→varchar, jsonb→simple-json) |
| `src/migrations/` | 12 PostgreSQL migrations | Replace with 1 SQLite migration |
| `src/app/actions/*.ts` | Server Actions | Remove (replaced by IPC) |
| `src/app/api/**` | API Routes | Remove (hooks → mini HTTP) |
| `src/app/[locale]/page.tsx` | Home (Server Component) | Convert to Client Component |
| `src/app/[locale]/layout.tsx` | Layout (Server Component) | Convert to Client Component |
| `src/app/[locale]/task/[id]/page.tsx` | Task Detail (Server Component) | Convert to Client + Query Param |
| `src/app/[locale]/pane-layout/page.tsx` | Pane Layout (Server Component) | Convert to Client Component |
| `src/app/[locale]/login/` | Login page | Remove |
| `server.ts` | Custom HTTP/WS server | Remove (logic → main/) |
| `boot.js` | Bootstrap | Remove |
| `docker-compose.yml` | PostgreSQL Docker | Remove |
| `package.json` | Dependencies | Modify |
| `electron-builder.yml` | Packaging config | Create |
| `tsconfig.json` | TypeScript config | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| IPC handler naming | New convention | `{domain}:{action}` (e.g., `kanban:getTasksByStatus`) |
| Client hook naming | React convention | `useIpc{Domain}` (e.g., `useIpcKanban`) |
| Entity column types | TypeORM SQLite docs | `simple-enum`, `varchar`, `simple-json` |
| Component props | Existing pattern | Props interface per component |
| i18n | Existing next-intl | `useTranslations()` client-side only |
| Korean comments | CLAUDE.md | 주석/답변은 한국어 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Electron 통합 | electron + electron-builder | Server Actions 제거 + Static Export | Nextron 보일러플레이트 |
| Next.js 빌드 | `output: 'export'` (Static) | IPC 성능 (~0.08ms) | Custom server in Electron (~1ms+) |
| DB | better-sqlite3 + TypeORM | 네이티브 성능, 파일 기반 | sql.js (WASM, 메모리 기반) |
| 데이터 통신 | Electron IPC (`ipcMain.handle`) | 성능 최적 | HTTP localhost |
| 동적 라우팅 | Query Param (`/task?id=xxx`) | Static Export 호환 | Catch-all route |
| 인증 | 제거 | 데스크탑 앱, OS 인증 충분 | 유지 (불필요) |
| Hooks API | Electron main 미니 HTTP | 외부 CLI 도구 호출 필요 | 제거 (기능 손실) |
| Terminal | WS in main process | node-pty main에서만 동작 | IPC (스트림 부적합) |
| 자동 업데이트 | electron-updater | 사용자 요구사항 | 수동 업데이트 |
| Migration | 단일 초기 SQLite migration | 깔끔한 시작 | 12개 개별 수정 |

## Data Models

### KanbanTask (SQLite)
| Field | Type | Constraints |
|-------|------|-------------|
| id | varchar | PK, UUID generated in JS |
| title | varchar(255) | NOT NULL |
| description | text | nullable |
| status | simple-enum(todo,progress,pending,review,done) | NOT NULL, default: todo |
| branch_name | varchar(255) | nullable |
| worktree_path | varchar(500) | nullable |
| session_type | simple-enum(tmux,zellij) | nullable |
| session_name | varchar(255) | nullable |
| ssh_host | varchar(255) | nullable |
| agent_type | varchar(50) | nullable |
| project_id | varchar | FK → projects.id, nullable |
| base_branch | varchar(255) | nullable |
| pr_url | varchar(500) | nullable |
| priority | simple-enum(low,medium,high) | nullable |
| display_order | integer | default: 0 |
| created_at | datetime | auto |
| updated_at | datetime | auto |

### Project (SQLite)
| Field | Type | Constraints |
|-------|------|-------------|
| id | varchar | PK, UUID |
| name | varchar(255) | UNIQUE |
| repo_path | varchar(500) | NOT NULL |
| default_branch | varchar(255) | default: main |
| ssh_host | varchar(255) | nullable |
| is_worktree | boolean | default: false |
| color | varchar(7) | nullable |
| created_at | datetime | auto |

### PaneLayoutConfig (SQLite)
| Field | Type | Constraints |
|-------|------|-------------|
| id | varchar | PK, UUID |
| layout_type | varchar(50) | NOT NULL |
| panes | simple-json (text) | NOT NULL |
| project_id | varchar | FK, UNIQUE, nullable |
| is_global | boolean | default: false |
| created_at | datetime | auto |
| updated_at | datetime | auto |

### AppSettings (SQLite)
| Field | Type | Constraints |
|-------|------|-------------|
| id | varchar | PK, UUID |
| key | varchar(100) | UNIQUE |
| value | text | NOT NULL |
| created_at | datetime | auto |
| updated_at | datetime | auto |

## Implementation Todos

### Todo 1: Initialize Electron project structure and dependencies
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Electron + electron-builder 기본 프로젝트 구조를 생성하고 필요한 의존성을 설치한다
- **Work**:
  - `package.json`에 electron, electron-builder, electron-serve, electron-store, electron-updater, better-sqlite3, @types/better-sqlite3 추가, pg 제거
  - `package.json`의 `main` 필드를 `"app/background.js"`로 설정
  - `package.json`에 scripts 추가: `"dev": "nextron"`, `"build": "nextron build"`... 대신 커스텀 scripts: `"dev:electron"`, `"build:electron"`, `"pack"`
  - `electron-builder.yml` 생성 (macOS dmg, asar, asarUnpack for native modules)
  - `main/` 디렉토리 생성
  - `main/background.ts` - Electron app lifecycle, window creation, auto-updater 초기화
  - `main/preload.ts` - contextBridge로 IPC API 노출
  - `main/helpers/create-window.ts` - BrowserWindow factory (electron-store로 윈도우 상태 저장)
  - `renderer/` 디렉토리에 현재 Next.js 소스를 구조화 (심볼릭 링크 또는 이동)
  - `renderer/next.config.ts` - `output: 'export'`, `distDir` 설정, `images: { unoptimized: true }`, `trailingSlash: true`
  - `renderer/tsconfig.json` 생성 (extends root)
  - 루트 `tsconfig.json` 수정 (main/renderer 모두 포함)
  - `.gitignore`에 `app/`, `dist/` 추가
- **Convention Notes**: Nextron 관례에 맞는 디렉토리 명명 (`main/`, `renderer/`)
- **Verification**: `tsc --noEmit`으로 타입 체크 통과
- **Exit Criteria**: 프로젝트 구조가 main/renderer로 분리되고, 의존성이 설치되며, TypeScript 에러 없음
- **Status**: pending

### Todo 2: Setup SQLite database layer
- **Priority**: 1
- **Dependencies**: none
- **Goal**: TypeORM SQLite DataSource와 repository 함수를 Electron main process용으로 구성한다
- **Work**:
  - `main/database.ts` 생성:
    - `better-sqlite3` 타입 DataSource
    - `app.getPath('userData')`에 DB 파일 저장 (`kanvibe.sqlite` / `kanvibe.dev.sqlite`)
    - `prepareDatabase` 콜백에서 `PRAGMA journal_mode = WAL` 설정
    - `getDataSource()`, `getTaskRepository()`, `getProjectRepository()`, `getPaneLayoutConfigRepository()`, `getAppSettingsRepository()` 함수
    - Migration 자동 실행 (`migrationsRun: true` 또는 수동 `runMigrations()`)
  - Entity 파일 수정 (SQLite 호환):
    - `src/entities/KanbanTask.ts`: `type: "enum"` → `type: "simple-enum"` (3곳), `type: "uuid"` → `type: "varchar"` (1곳)
    - `src/entities/PaneLayoutConfig.ts`: `type: "jsonb"` → `type: "simple-json"`, `type: "uuid"` → `type: "varchar"`
    - `src/entities/AppSettings.ts`, `src/entities/Project.ts`: 변경 없음 (enum/jsonb/uuid FK 미사용)
  - `src/migrations/` 기존 12개 파일 삭제
  - `src/migrations/0001-InitialSqliteSchema.ts` 생성: SQLite DDL로 전체 스키마 작성
  - `src/lib/database.ts` 삭제 (main/database.ts로 대체)
  - `src/lib/typeorm-cli.config.ts` 삭제 또는 SQLite용으로 수정
- **Convention Notes**: Entity 데코레이터 기존 스타일 유지, 컬럼 이름 snake_case 유지
- **Verification**: main/database.ts에서 DataSource 초기화 + migration 실행 성공 확인
- **Exit Criteria**: SQLite DB 파일 생성되고, 4개 테이블이 올바른 스키마로 생성됨
- **Status**: pending

### Todo 3: Create IPC handlers for kanban operations
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: `src/app/actions/kanban.ts`의 모든 서버 액션을 Electron IPC handler로 전환한다
- **Work**:
  - `main/ipc/kanban.ts` 생성:
    - `kanban:getTasksByStatus` → `getTasksByStatus()` 로직 이전
    - `kanban:getMoreDoneTasks` → `getMoreDoneTasks(offset, limit)`
    - `kanban:getTaskById` → `getTaskById(taskId)`
    - `kanban:getTaskIdByProjectAndBranch` → `getTaskIdByProjectAndBranch(projectId, branchName)`
    - `kanban:createTask` → `createTask(input)`
    - `kanban:updateTaskStatus` → `updateTaskStatus(taskId, newStatus)`
    - `kanban:updateTask` → `updateTask(taskId, updates)`
    - `kanban:updateProjectColor` → `updateProjectColor(projectId, color)`
    - `kanban:deleteTask` → `deleteTask(taskId)`
    - `kanban:branchFromTask` → `branchFromTask(...)`
    - `kanban:connectTerminalSession` → `connectTerminalSession(...)`
    - `kanban:reorderTasks` → `reorderTasks(status, orderedIds)`
    - `kanban:moveTaskToColumn` → `moveTaskToColumn(...)`
    - `kanban:fetchAndSavePrUrl` → `fetchAndSavePrUrl(taskId)`
  - `registerKanbanHandlers()` 함수로 모든 handler를 `ipcMain.handle`에 등록
  - `revalidatePath` 호출 제거 → 대신 `BrowserWindow.webContents.send('board:refresh')` 이벤트 발송
  - `main/database.ts`의 repository 함수 import
- **Convention Notes**: IPC 채널명은 `{domain}:{camelCaseAction}` 패턴
- **Verification**: IPC handler가 에러 없이 등록되는지 확인
- **Exit Criteria**: kanban.ts의 14개 함수가 모두 IPC handler로 전환됨
- **Status**: pending

### Todo 4: Create IPC handlers for project operations
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: `src/app/actions/project.ts`의 모든 서버 액션을 Electron IPC handler로 전환한다
- **Work**:
  - `main/ipc/project.ts` 생성:
    - `project:getAll` → `getAllProjects()`
    - `project:getById` → `getProjectById(projectId)`
    - `project:register` → `registerProject(name, repoPath, sshHost?)`
    - `project:delete` → `deleteProject(projectId)`
    - `project:scanAndRegister` → `scanAndRegisterProjects(rootPath, sshHost?)`
    - `project:listSubdirectories` → `listSubdirectories(parentPath, sshHost?)`
    - `project:getBranches` → `getProjectBranches(projectId)`
    - `project:getHooksStatus` → hooks 상태 조회 함수들 (claude, gemini, codex, opencode)
    - `project:installHooks` → hooks 설치 함수들
    - `project:getTaskHooksStatus` → task hooks 상태 조회
    - `project:installTaskHooks` → task hooks 설치
  - `registerProjectHandlers()` 함수
  - `revalidatePath` → `BrowserWindow.webContents.send('board:refresh')`
- **Convention Notes**: IPC 채널명 패턴 일관성 유지
- **Verification**: IPC handler 등록 에러 없음
- **Exit Criteria**: project.ts의 ~20개 함수가 모두 IPC handler로 전환됨
- **Status**: pending

### Todo 5: Create IPC handlers for appSettings and paneLayout
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: appSettings, paneLayout 서버 액션을 IPC handler로 전환한다
- **Work**:
  - `main/ipc/appSettings.ts` 생성:
    - `settings:getSidebarDefaultCollapsed`, `settings:setSidebarDefaultCollapsed`
    - `settings:getSidebarHintDismissed`, `settings:dismissSidebarHint`
    - `settings:getDoneAlertDismissed`, `settings:dismissDoneAlert`
    - `settings:getNotificationSettings`, `settings:setNotificationEnabled`, `settings:setNotificationStatuses`
    - `settings:getDefaultSessionType`, `settings:setDefaultSessionType`
  - `main/ipc/paneLayout.ts` 생성:
    - `paneLayout:getGlobal`, `paneLayout:getProject`, `paneLayout:getEffective`, `paneLayout:getAll`
    - `paneLayout:save`, `paneLayout:delete`
  - 각각 `registerAppSettingsHandlers()`, `registerPaneLayoutHandlers()` 함수
- **Convention Notes**: 기존 비즈니스 로직 그대로 이전, serialize() 패턴 유지
- **Verification**: IPC handler 등록 에러 없음
- **Exit Criteria**: appSettings 11개 + paneLayout 6개 함수 전환 완료
- **Status**: pending

### Todo 6: Create IPC handlers for diff operations
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: diff 서버 액션을 IPC handler로 전환한다
- **Work**:
  - `main/ipc/diff.ts` 생성:
    - `diff:getGitDiffFiles` → `getGitDiffFiles(taskId)`
    - `diff:getOriginalFileContent` → `getOriginalFileContent(taskId, filePath)`
    - `diff:getFileContent` → `getFileContent(taskId, filePath)`
    - `diff:saveFileContent` → `saveFileContent(taskId, filePath, content)`
  - `registerDiffHandlers()` 함수
  - `validateFilePath()` 보안 검증 로직 그대로 유지
- **Convention Notes**: path traversal 보안 검증 필수 유지
- **Verification**: IPC handler 등록 에러 없음
- **Exit Criteria**: diff.ts의 4개 함수 전환 완료
- **Status**: pending

### Todo 7: Setup terminal WebSocket and board notification in main process
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: server.ts의 WebSocket 서버(터미널 + 보드 알림)를 Electron main process로 이전한다
- **Work**:
  - `main/ipc/terminal.ts` 생성:
    - WebSocket 서버 생성 (별도 포트, 현재와 동일 구조)
    - `ws.on('connection')` 핸들러: terminal attach, board events
    - `attachLocalSession()`, `attachRemoteSession()` 호출
    - `addBoardClient()`, `removeBoardClient()`, `getBoardClients()` 보드 알림
    - IPC handler: `terminal:getWsPort` → 렌더러에서 WS 포트 조회
  - 인증 검증 제거 (데스크탑 앱이므로 `validateSessionFromCookie` 불필요)
  - `main/background.ts`에서 WS 서버 시작
  - `src/lib/terminal.ts`, `src/lib/boardNotifier.ts`는 main 프로세스에서 직접 import
- **Convention Notes**: node-pty, ssh2는 main process에서만 사용
- **Verification**: WS 서버가 정상 시작되는지 확인
- **Exit Criteria**: 터미널 WebSocket 연결 + 보드 알림 broadcast 동작
- **Status**: pending

### Todo 8: Setup hooks mini HTTP server
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: 외부 AI 에이전트가 호출하는 hooks API를 Electron main process의 미니 HTTP 서버로 제공한다
- **Work**:
  - `main/ipc/hooks.ts` 생성:
    - `createServer()` 로 미니 HTTP 서버 생성
    - `POST /api/hooks/start` → 기존 `src/app/api/hooks/start/route.ts` 로직
    - `GET /api/hooks/status` → 기존 `src/app/api/hooks/status/route.ts` 로직
    - `/_internal/broadcast` → 보드 알림 broadcast
  - `startHooksServer(port)` 함수 → `main/background.ts`에서 호출
  - 기존 `src/app/api/hooks/` 디렉토리 삭제
  - 기존 `src/app/api/directories/route.ts` → IPC handler로 전환 (`directories:list`)
- **Convention Notes**: HTTP 응답 형식 기존과 동일하게 유지 (외부 CLI 호환)
- **Verification**: `curl` 로 hooks API 호출 성공
- **Exit Criteria**: 외부 AI 에이전트가 기존과 동일한 URL로 hooks를 호출할 수 있음
- **Status**: pending

### Todo 9: Create IPC client layer for renderer
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 렌더러(Next.js)에서 IPC를 호출하는 클라이언트 유틸리티 레이어를 만든다
- **Work**:
  - `renderer/preload.d.ts` 생성: `window.ipc` 타입 선언
  - `renderer/src/lib/ipc.ts` 생성:
    - 타입 안전한 IPC 클라이언트 함수들
    - `ipcKanban.getTasksByStatus()` → `window.ipc.invoke('kanban:getTasksByStatus')`
    - `ipcKanban.createTask(input)` → `window.ipc.invoke('kanban:createTask', input)`
    - `ipcProject.getAll()` → `window.ipc.invoke('project:getAll')`
    - `ipcSettings.getSidebarDefaultCollapsed()` 등
    - `ipcDiff.getGitDiffFiles(taskId)` 등
    - `ipcPaneLayout.getGlobal()` 등
  - 기존 `import { xxx } from "@/app/actions/yyy"` 패턴을 `import { ipcXxx } from "@/lib/ipc"` 로 전환하기 위한 API 계약
  - `onBoardRefresh(callback)` → `window.ipc.on('board:refresh', callback)` 이벤트 리스너
- **Convention Notes**: 네임스페이스별 객체로 그룹핑 (`ipcKanban`, `ipcProject` 등)
- **Verification**: TypeScript 타입 체크 통과
- **Exit Criteria**: 모든 IPC 채널에 대한 타입 안전한 클라이언트 함수 존재
- **Status**: pending

### Todo 10: Convert Server Components to Client Components
- **Priority**: 3
- **Dependencies**: Todo 3, 4, 5, 6, 9
- **Goal**: Server Component 페이지를 Client Component로 전환하고, Server Action 호출을 IPC로 교체한다
- **Work**:
  - `renderer/src/app/[locale]/page.tsx`:
    - `"use client"` 추가
    - `async function HomePage()` → `function HomePage()`
    - `await getTasksByStatus()` 등 → `useEffect` + `ipcKanban.getTasksByStatus()`
    - `useState` 로 데이터 관리, `useEffect` 로 초기 로딩
    - `await getAvailableHosts()` → `ipcProject.listSshHosts()` 또는 별도 IPC
  - `renderer/src/app/[locale]/layout.tsx`:
    - `setRequestLocale`, `getMessages()` → 클라이언트 방식으로 전환
    - `getNotificationSettings()` → `ipcSettings.getNotificationSettings()`
    - `NextIntlClientProvider`는 유지 (messages를 JSON import로 로드)
  - `renderer/src/app/[locale]/task/page.tsx` (기존 `task/[id]/page.tsx` → `task/page.tsx`):
    - 동적 라우트 제거, Query Param 방식으로 전환
    - `const searchParams = useSearchParams(); const id = searchParams.get('id');`
    - 모든 서버 데이터 fetching → IPC 호출
    - `handleStatusChange`, `handleDelete` 인라인 Server Action → IPC 호출 함수
    - `generateMetadata` 제거 (static export 미지원) → `useEffect`에서 `document.title` 설정
  - `renderer/src/app/[locale]/task/diff/page.tsx`:
    - Query Param 방식으로 전환 (`/task/diff?id=xxx`)
  - `renderer/src/app/[locale]/pane-layout/page.tsx`:
    - Server Action 호출 → IPC 호출로 전환
  - 로그인 관련 삭제:
    - `renderer/src/app/[locale]/login/` 디렉토리 삭제
    - `renderer/src/components/LoginForm.tsx` 삭제
    - `renderer/src/app/actions/auth.ts` 삭제
    - `renderer/src/lib/auth.ts` 삭제
- **Convention Notes**: 기존 컴포넌트 구조와 디자인 토큰 유지, `useTranslations()` 클라이언트 훅 사용
- **Verification**: `next build` (static export) 성공, 타입 에러 없음
- **Exit Criteria**: 모든 페이지가 `"use client"` Client Component로 동작하고, Server Action import 없음
- **Status**: pending

### Todo 11: Update all component Server Action imports to IPC calls
- **Priority**: 3
- **Dependencies**: Todo 9, 10
- **Goal**: 개별 컴포넌트에서 Server Action을 직접 import하는 부분을 IPC 클라이언트 호출로 전환한다
- **Work**:
  - `Board.tsx`: `import { createTask, ... } from "@/app/actions/kanban"` → `import { ipcKanban } from "@/lib/ipc"`
  - `CreateTaskModal.tsx`: `createTask()` → `ipcKanban.createTask()`
  - `BranchTaskModal.tsx`: `branchFromTask()` → `ipcKanban.branchFromTask()`
  - `ConnectTerminalForm.tsx`: `connectTerminalSession()` → `ipcKanban.connectTerminalSession()`
  - `ProjectSettings.tsx`: `registerProject()`, `deleteProject()` → `ipcProject.*`
  - `ProjectBranchTasksModal.tsx`: project 관련 → `ipcProject.*`
  - `ProjectColorEditor.tsx`: `updateProjectColor()` → `ipcKanban.updateProjectColor()`
  - `PriorityEditor.tsx`: `updateTask()` → `ipcKanban.updateTask()`
  - `PaneLayoutEditor.tsx`: `savePaneLayout()`, etc → `ipcPaneLayout.*`
  - `HooksStatusDialog.tsx`: hooks 관련 → `ipcProject.*`
  - `DiffPageClient.tsx`: diff 관련 → `ipcDiff.*`
  - `DiffFileTree.tsx`: diff 관련 → `ipcDiff.*`
  - `DoneConfirmDialog.tsx`: `dismissDoneAlert()` → `ipcSettings.*`
  - `CollapsibleSidebar.tsx`: sidebar 관련 → `ipcSettings.*`
  - `TaskDetailTitleCard.tsx`: task 관련 → `ipcKanban.*`
  - `TaskDetailInfoCard.tsx`: task 관련 → `ipcKanban.*`
  - `DeleteTaskButton.tsx`: Server Action prop → IPC 호출 prop/direct
  - `DoneStatusButton.tsx`: Server Action prop → IPC 호출 prop/direct
  - `NotificationListener.tsx`: WebSocket URL 변경 (WS 포트를 IPC로 조회)
  - `Terminal.tsx` / `TerminalLoader.tsx`: WebSocket URL 변경
  - `revalidatePath` 대체: `board:refresh` IPC 이벤트 수신 시 데이터 refetch
- **Convention Notes**: 기존 컴포넌트 API(props)를 최대한 유지, IPC 호출만 교체
- **Verification**: `tsc --noEmit` + `next build` 성공
- **Exit Criteria**: `grep -r "from \"@/app/actions" renderer/src/components/` 결과 0건
- **Status**: pending

### Todo 12: Configure next-intl for static export
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: next-intl이 `output: 'export'` 모드에서 정상 동작하도록 설정한다
- **Work**:
  - `renderer/src/i18n/request.ts` 수정: static rendering 호환 방식으로 변경
  - `renderer/src/i18n/routing.ts`: 기존 유지 (locale 목록, 기본 locale)
  - `renderer/src/i18n/navigation.ts`: `useRouter`, `Link` 등은 클라이언트에서 동작하므로 유지
  - `renderer/src/app/[locale]/layout.tsx`:
    - `setRequestLocale(locale)` 유지 (static rendering에서 필요)
    - `getMessages()` → messages JSON을 직접 import 또는 dynamic import로 로드
    - `generateStaticParams()` 유지 (각 locale별 페이지 생성)
  - `messages/ko.json`, `messages/en.json`, `messages/zh.json` 파일 위치 확인 및 조정
  - `next-intl/plugin` 설정이 static export와 호환되는지 확인
- **Convention Notes**: 기존 번역 키 구조 그대로 유지
- **Verification**: `next build`에서 각 locale (ko, en, zh) 페이지가 정상 생성
- **Exit Criteria**: 3개 locale × 4개 페이지 = 12개 정적 HTML 파일 생성
- **Status**: pending

### Todo 13: Setup electron-updater for auto-updates
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: electron-updater를 설정하여 앱 자동 업데이트를 지원한다
- **Work**:
  - `main/updater.ts` 생성:
    - `autoUpdater.checkForUpdatesAndNotify()` 호출
    - 업데이트 이벤트 핸들링: `checking-for-update`, `update-available`, `update-downloaded`, `error`
    - 다운로드 진행률 → BrowserWindow에 IPC 이벤트 전송
  - `main/background.ts`에서 앱 시작 시 `checkForUpdates()` 호출
  - IPC handlers:
    - `updater:checkForUpdates` → 수동 업데이트 확인
    - `updater:quitAndInstall` → 업데이트 설치 및 앱 재시작
  - `electron-builder.yml`에 publish 설정 추가 (GitHub releases)
  - `renderer/src/components/UpdateNotification.tsx` (선택적): 업데이트 알림 UI
- **Convention Notes**: 자동 업데이트는 production 빌드에서만 활성화
- **Verification**: `electron-builder --publish never`로 패키지 생성 후 updater 초기화 에러 없음
- **Exit Criteria**: 앱 시작 시 업데이트 확인 로직이 동작하고, IPC로 상태 전달
- **Status**: pending

### Todo 14: Remove deprecated files and update configurations
- **Priority**: 3
- **Dependencies**: Todo 3, 4, 5, 6, 7, 8, 10, 11
- **Goal**: 더 이상 사용하지 않는 파일과 의존성을 정리하고, 설정 파일을 업데이트한다
- **Work**:
  - 삭제 대상:
    - `server.ts` (main/으로 이전됨)
    - `boot.js` (Electron이 대체)
    - `docker-compose.yml` (PostgreSQL 제거)
    - `src/app/actions/` 디렉토리 전체 (IPC로 대체됨)
    - `src/app/api/` 디렉토리 전체 (IPC/HTTP로 대체됨)
    - `src/app/[locale]/login/` (인증 제거)
    - `src/components/LoginForm.tsx` (인증 제거)
    - `src/lib/auth.ts` (인증 제거)
    - `src/lib/database.ts` (main/database.ts로 대체)
    - `src/lib/typeorm-cli.config.ts` (CLI migration 불필요)
    - `src/migrations/1770854400000-*.ts` ~ `src/migrations/1771400000000-*.ts` (12개 모두)
  - `package.json`:
    - `pg` 제거
    - `scripts` 업데이트: `db:up`, `db:down`, `typeorm`, `migration:*` 제거
    - `postinstall`에 `electron-builder install-app-deps` 추가
  - `.env.example` 수정: `KANVIBE_USER`, `KANVIBE_PASSWORD`, `DB_PORT` 제거, `PORT` 유지
  - `CLAUDE.md` 업데이트: Tech Stack 변경, Database Migration 섹션 수정
- **Convention Notes**: 사용하지 않는 코드는 완전히 제거 (backward-compat hack 금지)
- **Verification**: `pnpm install` + `tsc --noEmit` + `next build` 성공
- **Exit Criteria**: 레거시 파일 0개, 미사용 의존성 0개, 빌드 성공
- **Status**: pending

### Todo 15: Integration test - full app build and run
- **Priority**: 4
- **Dependencies**: Todo 10, 11, 12, 13, 14
- **Goal**: 전체 앱을 빌드하고 Electron으로 실행하여 주요 기능이 동작하는지 확인한다
- **Work**:
  - `next build` (static export) 성공 확인
  - Electron main process 컴파일 확인
  - `electron .` 또는 dev 모드로 앱 실행
  - 확인 항목:
    - 메인 페이지 로딩 + 칸반 보드 표시
    - 태스크 생성/수정/삭제
    - 프로젝트 등록/삭제
    - 태스크 드래그 앤 드롭
    - 터미널 WebSocket 연결
    - 보드 알림 (외부 hooks 호출 시)
    - Pane Layout 설정
    - i18n 언어 전환 (ko/en/zh)
    - electron-updater 초기화 에러 없음
  - `electron-builder` 로 패키지 빌드 (macOS DMG 등)
- **Convention Notes**: 수동 테스트 체크리스트
- **Verification**: 앱 실행 후 주요 기능 동작 확인
- **Exit Criteria**: Electron 앱이 정상 실행되고 주요 CRUD + 터미널 + 알림이 동작
- **Status**: pending

## Verification Strategy
- `tsc --noEmit`: 전체 TypeScript 타입 체크
- `next build`: Static export 빌드 성공
- Electron dev 모드 실행: UI 표시 + IPC 통신 확인
- SQLite DB 생성 확인: `userData/kanvibe.dev.sqlite` 파일 존재 + 테이블 생성
- 터미널 WebSocket 연결: node-pty 세션 attach 확인
- Hooks API HTTP 호출: `curl http://localhost:PORT/api/hooks/status` 응답 확인
- `electron-builder`: DMG/패키지 생성 성공

## Progress Tracking
- Total Todos: 15
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-24: Plan created
