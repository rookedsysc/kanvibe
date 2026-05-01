# Fix Packaged App Shortcuts and Dialogs

## Business Goal
패키징된 macOS 앱이 올바른 제품명으로 설치되고, 실행 시 로딩 화면에 고착되지 않으며, 사용자 기대에 맞게 Cmd/Ctrl+N과 ESC 키가 동작하도록 한다.

## Scope
- **In Scope**: Electron productName 수정, 앱/보드 초기 로딩 실패 처리, Cmd/Ctrl+N을 새 창 대신 새 Task 다이얼로그로 연결, 주요 다이얼로그의 ESC 닫기 동작 추가, 관련 테스트와 빌드 검증
- **Out of Scope**: 앱 서명/notarization, 릴리즈 자동화, 다이얼로그 디자인 리워크, 단축키 설정 UI 확장

## Codebase Analysis Summary
Electron 엔트리는 `electron/main.js`이며 패키징명은 `electron-builder.yml`에서 결정된다. 렌더러는 `src/desktop/renderer/App.tsx`와 route 컴포넌트에서 데이터를 비동기로 로드하고, 실패 경로가 일부 없어 `Loading...` 상태가 유지될 수 있다. Board command는 `BoardCommandProvider`에 중앙화되어 있고 `Board`가 create modal handler를 등록한다. 다이얼로그는 공통 컴포넌트 없이 여러 TSX 파일에서 개별 overlay로 구현되어 있다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `electron-builder.yml` | macOS 앱 제품명/DMG 산출물 설정 | Modify |
| `electron/main.js` | Electron BrowserWindow와 Cmd/Ctrl+N 새 창 처리 | Modify |
| `src/desktop/renderer/App.tsx` | 세션 로딩과 전역 desktop event 구독 | Modify |
| `src/desktop/renderer/routes/BoardRoute.tsx` | 보드 초기 데이터 로딩 | Modify |
| `src/desktop/renderer/components/BoardCommandProvider.tsx` | 보드 전역 단축키 처리 | Modify |
| `src/components/*Dialog*.tsx`, `src/components/*Modal*.tsx`, `src/components/ProjectSettings.tsx` | 주요 overlay/dialog UI | Modify |
| 관련 `__tests__` | 회귀 테스트 | Modify/Create |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| React 컴포넌트 | 기존 `src/components` | hooks 기반 함수 컴포넌트와 Tailwind class 유지 |
| 단축키 처리 | `keyboardShortcut.ts`, `BoardCommandProvider.tsx` | `matchShortcutEvent`와 `Mod` abstraction 재사용 |
| 테스트 | 기존 Vitest 테스트 | `@testing-library/react`, `vi.mock`, 사용자-visible 동작 검증 |
| 파일 수정 | repository/developer instruction | 수동 수정은 `apply_patch` 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Cmd/Ctrl+N 처리 위치 | renderer `BoardCommandProvider`에서 create modal 호출 | 이미 `Board`가 create modal handler를 등록하고 있어 중복 구현 없이 요구 동작을 만족한다 | Electron main이 renderer로 IPC 전송 |
| Electron 새 창 단축키 | `electron/main.js`의 Cmd/Ctrl+N 새 창 override 제거 | OS 기본 새 창보다 앱 내 새 task가 우선 요구사항이다 | 다른 단축키로 새 창 기능 이동 |
| Loading 고착 대응 | async 로딩 실패를 잡아 fallback state로 전환 | 패키징 환경 오류가 무한 로딩으로 숨지 않게 한다 | 에러 페이지를 새로 디자인 |
| ESC 닫기 | 작은 reusable hook 추가 후 주요 dialog에 적용 | 반복되는 window keydown 로직을 일관되게 관리한다 | 각 컴포넌트에 개별 effect 작성 |

## Implementation Todos

### Todo 1: 제품명과 Cmd/Ctrl+N 동작 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: DMG 설치 앱명이 `Kanivibe`로 나오고 Cmd/Ctrl+N이 새 창 대신 새 task 다이얼로그를 연다.
- **Work**:
  - `electron-builder.yml`의 `productName`을 `Kanivibe`로 변경한다.
  - `electron/main.js`에서 Cmd/Ctrl+N 새 창 생성 `before-input-event` 처리를 제거한다.
  - `src/desktop/renderer/components/BoardCommandProvider.tsx`에서 `CREATE_BRANCH_TODO_SHORTCUT`도 전역 단축키로 처리해 `openCreateTaskModal`을 호출한다.
  - `BoardCommandProvider.test.tsx`에 Cmd/Ctrl+N 동작 테스트를 추가한다.
- **Convention Notes**: 기존 `matchShortcutEvent` 기반 단축키 매칭을 재사용한다.
- **Verification**: `pnpm test src/desktop/renderer/components/__tests__/BoardCommandProvider.test.tsx`
- **Exit Criteria**: 테스트에서 Cmd/Ctrl+N이 create modal handler를 호출하고 electron 새 창 override가 남아 있지 않다.
- **Status**: completed

### Todo 2: Loading 고착 방지
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 세션/보드 초기 로딩 promise가 실패해도 `Loading...`에 영구 고착되지 않는다.
- **Work**:
  - `src/desktop/renderer/App.tsx`에서 `getSessionState()` 실패 시 unauthenticated fallback으로 상태를 설정하고 desktop event API가 없을 때도 안전하게 동작하게 한다.
  - `src/desktop/renderer/routes/BoardRoute.tsx`에서 초기 데이터 로딩 실패 시 빈 보드에 필요한 기본 데이터를 설정한다.
  - App/BoardRoute 테스트에 실패 promise가 loading을 해제하는 회귀 테스트를 추가한다.
- **Convention Notes**: 기존 `RouteLoadingFallback`과 route cache 구조를 유지한다.
- **Verification**: `pnpm test src/desktop/renderer/__tests__/App.test.tsx src/desktop/renderer/routes/__tests__/BoardRoute.test.tsx`
- **Exit Criteria**: 실패 경로에서도 loading 텍스트가 사라지고 login 또는 빈 board fallback이 렌더링된다.
- **Status**: completed

### Todo 3: 주요 다이얼로그 ESC 닫기
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Create/Branch/Done/Hooks/AI Sessions/Project settings/Project branch tasks 등 주요 다이얼로그가 ESC로 닫힌다.
- **Work**:
  - `src/hooks/useEscapeKey.ts` 또는 유사한 작은 hook을 추가한다.
  - 주요 dialog/modal 컴포넌트에서 open 여부와 pending 상태를 고려해 hook을 적용한다.
  - 대표 다이얼로그 테스트에 ESC 닫기 케이스를 추가한다.
- **Convention Notes**: 입력 내부 ESC를 직접 사용하는 컴포넌트와 충돌하지 않도록 open 상태에서 window keydown만 최소 처리한다.
- **Verification**: `pnpm test src/components/__tests__/DoneConfirmDialog.test.tsx src/components/__tests__/CreateTaskModal.test.tsx src/components/__tests__/HooksStatusDialog.test.tsx src/components/__tests__/AiSessionsDialog.test.tsx src/components/__tests__/ProjectSettings.test.tsx`
- **Exit Criteria**: ESC keydown이 각 다이얼로그의 `onClose`/`onCancel`을 호출한다.
- **Status**: completed

### Todo 4: 통합 검증
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2, Todo 3
- **Goal**: 수정이 타입/빌드/패키징 경로를 깨지 않음을 확인한다.
- **Work**:
  - focused tests를 실행한다.
  - `pnpm build`를 실행한다.
  - 가능한 환경이면 `pnpm dist`를 실행하고, 불가능하면 실패 사유를 기록한다.
- **Convention Notes**: 검증 실패 시 원인별로 수정하고 재시도한다.
- **Verification**: focused tests, `pnpm build`, `pnpm dist`
- **Exit Criteria**: 가능한 검증은 통과하고, 환경 제약이 있는 검증은 구체적 사유가 기록된다.
- **Status**: completed

## Verification Strategy
- `pnpm test`로 관련 단축키, 로딩, ESC 동작 테스트를 확인한다.
- `pnpm build`로 renderer/main 빌드가 통과하는지 확인한다.
- 환경이 지원하면 `pnpm dist`로 DMG 산출 경로까지 확인한다.

## Progress Tracking
- Total Todos: 4
- Completed: 4
- Status: Execution complete

## Change Log
- 2026-05-01: Plan created
- 2026-05-01: Todo 1 completed — productName and Cmd/Ctrl+N command behavior updated
- 2026-05-01: Todo 2 completed — session and board loading failure fallbacks added
- 2026-05-01: Todo 3 completed — ESC close handling added to major dialogs
- 2026-05-01: Todo 4 completed — type-check, full test suite, build, dist:dir, and dist path validation completed; DMG creation is blocked on Linux by macOS-only dmg-license optional dependency; lint still reports pre-existing broad config violations outside this task
