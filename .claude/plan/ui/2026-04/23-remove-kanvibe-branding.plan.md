# Remove KanVibe Branding

## Business Goal
사용자에게 노출되는 KanVibe 로고, 앱 이름, 알림 아이콘 브랜딩을 제거해 제품 화면에서 특정 브랜드 식별자가 보이지 않도록 한다.

## Scope
- **In Scope**: 로그인/보드 화면의 KanVibe 텍스트와 로고 제거, 브라우저/문서 제목의 KanVibe 제거, 브라우저/데스크톱 알림에서 전용 아이콘 제거, 관련 테스트 기대값 갱신, 사용하지 않는 브랜드 이미지 자산 삭제
- **Out of Scope**: 내부 IPC 채널명, 환경 변수명, hook 파일명, DB 경로, 패키지명, 문서/README의 프로젝트 설명 문구 일괄 리네이밍

## Codebase Analysis Summary
React UI는 `src/components`와 `src/desktop/renderer/routes`에 있고, 다국어 문자열은 `messages/*.json`에서 관리된다. 보드 헤더는 `Board.tsx`에서 직접 로고 이미지를 참조하고, 로그인 화면은 `LoginForm.tsx`에서 앱 이름을 하드코딩한다. 알림 아이콘은 브라우저 훅과 Electron 메인 프로세스 양쪽에서 `public/icons/icon-192x192.png`를 참조한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Board.tsx` | 보드 상단 로고/타이틀 표시 | Modify |
| `src/components/LoginForm.tsx` | 로그인 카드 타이틀 표시 | Modify |
| `src/desktop/renderer/routes/*.tsx` | 문서 title 설정 | Modify |
| `src/hooks/useTaskNotification.ts` | 브라우저 알림 옵션 구성 | Modify |
| `electron/main.js` | Electron 데스크톱 알림 옵션 구성 | Modify |
| `src/desktop/main/services/desktopNotificationService.ts` | Electron 알림 아이콘 옵션 타입 | Modify |
| `messages/*.json` | 보드 타이틀 번역 문자열 | Modify |
| `electron-builder.yml` | 패키징된 앱 표시 이름 | Modify |
| `tests/e2e/login.spec.js` | 로그인 E2E 기대값 | Modify |
| `src/hooks/__tests__/useTaskNotification.test.ts` | 브라우저 알림 아이콘 기대값 | Modify |
| `public/kanvibe-logo.svg`, `public/icons/*.png` | 브랜드 이미지 자산 | Delete |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 최소 범위 변경 | `CODE_PRINCIPLES.md` | 요구된 브랜딩 제거 외 리팩토링은 하지 않는다 |
| 프론트 반응형 유지 | `FRONTEND.md` | 헤더 레이아웃이 깨지지 않도록 정렬만 조정한다 |
| 기존 테스트 스타일 유지 | Existing tests | 현재 Vitest/Playwright assertion 패턴을 유지한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 내부 식별자 처리 | 유지 | IPC/env/hook 계약을 변경하면 기능 회귀 범위가 커진다 | 전체 리네이밍 |
| 알림 아이콘 처리 | 아이콘 옵션 생략 | 전용 브랜드 아이콘 파일을 삭제해도 OS/브라우저 기본 알림 아이콘으로 동작한다 | 대체 아이콘 추가 |
| 화면 타이틀 처리 | 브랜드 영역 제거 | 빈 타이틀 텍스트보다 DOM 자체를 제거하는 편이 접근성과 레이아웃에 낫다 | 빈 문자열 렌더링 |

## Implementation Todos

### Todo 1: Remove visible UI branding
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 로그인/보드/문서 제목에서 KanVibe 텍스트와 로고를 제거한다.
- **Work**:
  - `src/components/Board.tsx`에서 로고 이미지와 타이틀 영역을 제거하고 헤더 컨트롤을 오른쪽 정렬로 조정
  - `src/components/LoginForm.tsx`에서 하드코딩된 KanVibe heading 제거
  - `src/desktop/renderer/routes/BoardRoute.tsx`, `DiffRoute.tsx`, `TaskDetailRoute.tsx`, `PaneLayoutRoute.tsx`의 KanVibe fallback title 제거
  - `index.html` title을 비운다
  - `messages/en.json`, `messages/ko.json`, `messages/zh.json`의 `board.title` 값을 비운다
  - `electron-builder.yml`의 패키징 표시 이름에서 KanVibe를 제거한다
- **Convention Notes**: 기존 Tailwind 클래스 스타일을 유지하고 불필요한 컴포넌트 분리는 하지 않는다.
- **Verification**: `rg -n "KanVibe|kanvibe-logo\\.svg" src index.html messages tests/e2e/login.spec.js`
- **Exit Criteria**: 사용자 화면 관련 파일에서 KanVibe 브랜드명과 로고 참조가 사라진다.
- **Status**: completed

### Todo 2: Remove notification icon branding
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 브라우저/Electron 알림에서 브랜드 아이콘 파일 의존성을 제거한다.
- **Work**:
  - `src/hooks/useTaskNotification.ts`에서 `icon` 옵션과 `NOTIFICATION_ICON_PATH` 상수를 제거
  - `electron/main.js`에서 `getNotificationIconPath` 함수와 `iconPath` 전달을 제거
  - `src/desktop/main/services/desktopNotificationService.ts`에서 `iconPath`를 optional로 바꾸고 값이 있을 때만 Electron 옵션에 포함
  - `public/kanvibe-logo.svg`, `public/icons/icon-192x192.png`, `public/icons/icon-512x512.png` 삭제
- **Convention Notes**: 기존 알림 payload 계약은 유지한다.
- **Verification**: `rg -n "icon-192x192|icon-512x512|kanvibe-logo\\.svg|NOTIFICATION_ICON_PATH|getNotificationIconPath" src electron public`
- **Exit Criteria**: 브랜드 이미지 자산과 직접 참조가 제거된다.
- **Status**: completed

### Todo 3: Update tests and run verification
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 변경된 UI/알림 동작에 맞게 테스트를 조정하고 회귀를 확인한다.
- **Work**:
  - `tests/e2e/login.spec.js`에서 KanVibe heading assertion을 제거하고 로그인 폼 필드 assertion으로 대체
  - `src/hooks/__tests__/useTaskNotification.test.ts`에서 알림 icon 기대값 제거
  - 필요한 경우 `src/desktop/main/services/__tests__/desktopNotificationService.test.ts`의 옵션 타입 사용을 갱신
  - `pnpm test -- --runInBand` 또는 가능한 Vitest 명령과 `pnpm check` 실행
- **Convention Notes**: 기존 테스트 fixture의 프로젝트명 `kanvibe`는 사용자 데이터 예시라 유지한다.
- **Verification**: `pnpm test -- --runInBand`, `pnpm check`
- **Exit Criteria**: 테스트와 타입 체크가 통과하거나, 환경 문제는 명확히 기록된다.
- **Status**: completed

## Verification Strategy
- `rg`로 사용자 화면 관련 KanVibe 텍스트와 삭제한 이미지 참조 잔존 여부 확인
- `pnpm test -- --runInBand`로 단위 테스트 실행
- `pnpm check`로 타입 안정성 확인
- `pnpm lint`로 lint 상태 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-04-23: Plan created
- 2026-04-23: Todo 1 completed - Removed visible UI branding from login, board, document titles, and locale title strings
- 2026-04-23: Todo 2 completed - Removed notification icon references and deleted brand image assets
- 2026-04-23: Todo 3 completed - Updated tests and verified targeted branding changes; full test and lint baseline failures were recorded
- 2026-04-23: Execution complete
