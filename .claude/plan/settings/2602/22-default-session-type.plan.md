# 기본 세션 타입 설정 기능

## Business Goal
새 작업 생성 시 세션 타입이 항상 tmux로 고정되어 있어 zellij 사용자가 매번 수동으로 변경해야 한다. 설정에서 기본 세션 타입을 선택할 수 있게 하여 사용자 편의성을 높인다.

## Scope
- **In Scope**: appSettings에 기본 세션 타입 설정 추가, ProjectSettings UI, CreateTaskModal/BranchTaskModal 기본값 반영, i18n 번역
- **Out of Scope**: hooks API(`/api/hooks/start`)의 기본 세션 타입 (외부에서 직접 전달하므로 제어 불가)

## Codebase Analysis Summary
- `appSettings.ts`에 KV 스토어 기반 설정 getter/setter 패턴이 존재 (`getAppSetting`/`setAppSetting`)
- `ProjectSettings.tsx`에 사이드바, 알림 등 설정 UI가 섹션별로 구성됨
- `CreateTaskModal`의 세션 타입 `<select>`는 tmux가 첫 번째 option으로 하드코딩
- `BranchTaskModal`은 `useState<SessionType>(SessionType.TMUX)`로 초기화

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/actions/appSettings.ts` | 설정 getter/setter | Modify |
| `src/components/ProjectSettings.tsx` | 설정 패널 UI | Modify |
| `src/components/CreateTaskModal.tsx` | 작업 생성 모달 | Modify |
| `src/components/BranchTaskModal.tsx` | 브랜치 분기 모달 | Modify |
| `src/components/Board.tsx` | Board 컴포넌트 (props 전달) | Modify |
| `src/app/[locale]/page.tsx` | 메인 페이지 (설정 로딩) | Modify |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 설정 키 상수 | appSettings.ts | `UPPER_SNAKE_CASE` + `_KEY` 접미사 |
| getter/setter 네이밍 | appSettings.ts | `get{Setting}` / `set{Setting}` 패턴 |
| 번역 키 | messages/*.json | `settings` 네임스페이스 하위 |
| 주석 언어 | CODE_PRINCIPLES.md | 한국어 |

## Implementation Todos

### Todo 1: appSettings에 기본 세션 타입 getter/setter 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: DB에 기본 세션 타입을 저장/조회하는 함수 추가
- **Work**:
  - `src/app/actions/appSettings.ts`에 `DEFAULT_SESSION_TYPE_KEY` 상수 추가
  - `getDefaultSessionType()` 함수 추가 — `SessionType`을 반환, 미설정 시 `SessionType.TMUX` 반환
  - `setDefaultSessionType(sessionType: SessionType)` 함수 추가
- **Convention Notes**: 기존 `getSidebarDefaultCollapsed`/`setSidebarDefaultCollapsed` 패턴 따름
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: getter/setter가 올바르게 export됨
- **Status**: pending

### Todo 2: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 3개 언어 파일에 기본 세션 타입 설정 관련 번역 추가
- **Work**:
  - `messages/ko.json`의 `settings` 네임스페이스에 추가:
    - `defaultSessionTypeSection`: "작업 생성"
    - `defaultSessionType`: "기본 세션 타입"
    - `defaultSessionTypeDescription`: "새 작업 생성 시 기본으로 선택되는 세션 타입입니다."
  - `messages/en.json`, `messages/zh.json`에 동일 키로 번역 추가
- **Convention Notes**: 기존 번역 키 네이밍 패턴 따름
- **Verification**: JSON 파싱 가능 여부 확인
- **Exit Criteria**: 3개 언어 파일에 동일 키 존재
- **Status**: pending

### Todo 3: ProjectSettings에 기본 세션 타입 선택 UI 추가
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 설정 패널에서 기본 세션 타입을 선택할 수 있는 UI 추가
- **Work**:
  - `ProjectSettingsProps`에 `defaultSessionType: SessionType` 추가
  - 알림 섹션 위에 "작업 생성" 섹션 추가
  - `<select>` 드롭다운으로 tmux/zellij 선택 가능하게 구현
  - `setDefaultSessionType()` 호출하여 변경 저장
- **Convention Notes**: 기존 사이드바 접기 토글 UI 패턴 참고
- **Verification**: UI 렌더링 확인
- **Exit Criteria**: 설정에서 세션 타입 변경 시 DB에 저장
- **Status**: pending

### Todo 4: Board/page.tsx에서 기본 세션 타입 전달
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 메인 페이지에서 기본 세션 타입을 로딩하여 Board → CreateTaskModal/BranchTaskModal로 전달
- **Work**:
  - `src/app/[locale]/page.tsx`에서 `getDefaultSessionType()` 호출 추가
  - `Board` props에 전달
  - `BoardProps`에 `defaultSessionType: SessionType` 추가
  - `Board`에서 `CreateTaskModal`, `BranchTaskModal`, `ProjectSettings`에 prop 전달
- **Convention Notes**: 기존 `sidebarDefaultCollapsed` 전달 패턴 따름
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 설정값이 모달까지 전달됨
- **Status**: pending

### Todo 5: CreateTaskModal/BranchTaskModal에서 기본값 반영
- **Priority**: 3
- **Dependencies**: Todo 4
- **Goal**: 세션 타입 선택의 기본값을 설정에서 가져온 값으로 적용
- **Work**:
  - `CreateTaskModal`: props에 `defaultSessionType` 추가, `<select>` 기본값으로 사용
  - `BranchTaskModal`: props에 `defaultSessionType` 추가, `useState` 초기값으로 사용
- **Convention Notes**: CreateTaskModal은 uncontrolled select, BranchTaskModal은 controlled state
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 모달 열 때 설정된 기본값이 선택되어 있음
- **Status**: pending

## Verification Strategy
- TypeScript 빌드 성공 확인
- 3개 언어 JSON 파일 파싱 확인

## Progress Tracking
- Total Todos: 5
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-22: Plan created
