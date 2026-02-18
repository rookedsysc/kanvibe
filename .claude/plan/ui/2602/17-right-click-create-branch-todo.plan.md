# Right-click Create Branch TODO

## Business Goal
기존 태스크 카드를 우클릭하여 해당 태스크의 브랜치를 base로 삼아 새 브랜치 기반 TODO를 생성하는 기능을 추가한다. 이를 통해 기존 작업에서 파생 작업을 빠르게 만들 수 있다.

## Scope
- **In Scope**: TaskContextMenu에 새 옵션 추가, CreateTaskModal에 defaultBaseBranch prop 추가, Board.tsx 핸들러 연결, 3개 언어 번역 키 추가
- **Out of Scope**: 컬럼 빈 영역 우클릭, BranchTaskModal 수정, 새 API endpoint 추가

## Codebase Analysis Summary
- `TaskContextMenu`: hasBranch prop으로 분기 옵션 표시 제어. 현재 hasBranch일 때는 삭제만 표시됨
- `CreateTaskModal`: defaultProjectId optional prop 패턴 존재. baseBranch는 프로젝트 선택 시 자동 설정
- `Board.tsx`: contextMenu state에 task 정보 포함. isBranchModalOpen 패턴으로 모달 제어
- `createTask` server action: baseBranch를 input으로 받아 처리

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/TaskContextMenu.tsx` | 우클릭 컨텍스트 메뉴 | Modify |
| `src/components/CreateTaskModal.tsx` | 작업 생성 모달 | Modify |
| `src/components/Board.tsx` | 메인 보드 컴포넌트 | Modify |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| i18n 키 사용 | messages/*.json | 하드코딩 텍스트 금지, 3개 언어 동시 추가 |
| CSS 토큰 사용 | globals.css | Tailwind 클래스에서 디자인 토큰 사용 |
| useCallback 패턴 | Board.tsx | 이벤트 핸들러를 useCallback으로 감싸기 |
| Optional prop 패턴 | CreateTaskModal | defaultProjectId처럼 optional prop으로 기본값 전달 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 모달 | CreateTaskModal 재사용 | 동일한 폼 필드, 코드 중복 방지 | 별도 모달 생성 |
| Base 브랜치 전달 | defaultBaseBranch prop | 기존 defaultProjectId 패턴과 일관 | 별도 state 주입 |
| 메뉴 조건 | hasBranch일 때만 표시 | 브랜치 없는 태스크는 base가 없으므로 의미 없음 | 항상 표시 |

## Implementation Todos

### Todo 1: Add i18n translation keys
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 컨텍스트 메뉴의 새 옵션에 대한 번역 키를 3개 언어에 추가
- **Work**:
  - `messages/ko.json`의 `contextMenu` 객체에 `"createBranchTodo": "브랜치 기반 TODO 생성"` 추가
  - `messages/en.json`의 `contextMenu` 객체에 `"createBranchTodo": "Create branch TODO"` 추가
  - `messages/zh.json`의 `contextMenu` 객체에 `"createBranchTodo": "创建分支TODO"` 추가
- **Convention Notes**: 3개 언어 파일 동시 수정
- **Verification**: JSON 파싱 오류 없는지 확인
- **Exit Criteria**: 3개 언어 파일 모두에 동일 키가 존재
- **Status**: pending

### Todo 2: Add onCreateBranchTodo callback to TaskContextMenu
- **Priority**: 1
- **Dependencies**: none
- **Goal**: TaskContextMenu에 새 옵션 UI와 콜백을 추가
- **Work**:
  - `TaskContextMenuProps`에 `onCreateBranchTodo: () => void` prop 추가
  - `hasBranch`일 때 "브랜치 기반 TODO 생성" 버튼 렌더링 (기존 버튼 스타일 따라감)
  - `t("createBranchTodo")` 번역 키 사용
- **Convention Notes**: 기존 버튼 CSS 클래스 패턴(`w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page transition-colors`) 유지
- **Verification**: TypeScript 타입 에러 없는지 확인
- **Exit Criteria**: hasBranch일 때 새 버튼이 표시되고 콜백 호출
- **Status**: pending

### Todo 3: Add defaultBaseBranch prop to CreateTaskModal
- **Priority**: 1
- **Dependencies**: none
- **Goal**: CreateTaskModal이 외부에서 baseBranch 기본값을 받을 수 있도록 확장
- **Work**:
  - `CreateTaskModalProps`에 `defaultBaseBranch?: string` 추가
  - `useEffect`에서 `defaultBaseBranch`가 있으면 `setBaseBranch(defaultBaseBranch)` 호출
  - `defaultBaseBranch`가 있을 때는 baseBranch select에 해당 값이 옵션으로 포함되도록 처리
- **Convention Notes**: 기존 defaultProjectId 패턴과 동일하게 처리
- **Verification**: TypeScript 타입 에러 없는지 확인
- **Exit Criteria**: defaultBaseBranch prop이 전달되면 baseBranch가 미리 설정됨
- **Status**: pending

### Todo 4: Wire up Board.tsx handlers
- **Priority**: 2
- **Dependencies**: Todo 1, 2, 3
- **Goal**: Board 컴포넌트에서 새 컨텍스트 메뉴 옵션과 CreateTaskModal을 연결
- **Work**:
  - `handleCreateBranchTodo` 콜백 추가 (useCallback): contextMenu를 닫고 CreateTaskModal을 열되 defaultBaseBranch와 defaultProjectId를 설정
  - 새 state 추가: `branchTodoDefaults` (`{ baseBranch: string; projectId: string } | null`)
  - `TaskContextMenu`에 `onCreateBranchTodo={handleCreateBranchTodo}` 전달
  - `CreateTaskModal`에 `defaultBaseBranch={branchTodoDefaults?.baseBranch}` 전달
  - 모달 닫힐 때 `branchTodoDefaults` 초기화
- **Convention Notes**: useCallback 패턴, 기존 handleBranchFromCard 패턴 참조
- **Verification**: TypeScript 빌드 성공 확인
- **Exit Criteria**: 우클릭 → "브랜치 기반 TODO 생성" → CreateTaskModal이 baseBranch 미리 채워진 상태로 열림
- **Status**: pending

## Verification Strategy
- `pnpm build` 성공 확인
- 3개 언어 JSON 파일 문법 확인
- TypeScript 타입 에러 없는지 확인

## Progress Tracking
- Total Todos: 4
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-17: Plan created
