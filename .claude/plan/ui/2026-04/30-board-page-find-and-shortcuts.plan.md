# Board Page Find Shortcut and Platform Quick Search

## Business Goal
칸반 보드 화면에서 사용자가 브라우저처럼 현재 보이는 프로젝트/텍스트를 빠르게 찾을 수 있게 하고, 기존 전역 태스크 검색 단축키는 운영체제에 맞는 기대 동작을 유지하도록 명확히 보장한다.

## Scope
- **In Scope**: 보드 화면 전용 `Cmd/Ctrl+F` 검색 바 추가, DOM 텍스트 기반 다음/이전 찾기, `Esc` 닫기, 전역 태스크 검색 `Mod+Shift+O`의 플랫폼별 단축키 동작/표시 검증
- **Out of Scope**: 터미널 검색, 태스크 상세/디프 화면 검색, 전역 태스크 검색 기능 재설계, 서버/API 변경

## Codebase Analysis Summary
현재 전역 태스크 검색은 `TaskQuickSearchDialog`가 앱 루트에 마운트되어 키다운 이벤트를 감지하는 구조다. 단축키 파싱과 플랫폼별 `Mod` 분기는 `keyboardShortcut.ts`에 모여 있다. 보드 화면은 `BoardRoute`가 데이터를 불러와 `Board` 컴포넌트로 전달하며, 이번 요구사항은 보드 화면 전용 UI라서 `Board` 내부 또는 그 인접 계층에 검색 바를 두는 것이 자연스럽다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/desktop/renderer/utils/keyboardShortcut.ts` | 전역 단축키 파싱/표시 유틸 | Modify |
| `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | 전역 태스크 검색 다이얼로그 | Reference |
| `src/desktop/renderer/routes/BoardRoute.tsx` | 보드 라우트 진입점 | Reference |
| `src/components/Board.tsx` | 칸반 보드 메인 UI | Modify |
| `messages/ko.json` | 한국어 UI 문구 | Modify |
| `messages/en.json` | 영어 UI 문구 | Modify |
| `messages/zh.json` | 중국어 UI 문구 | Modify |
| `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` | 전역 검색 단축키 테스트 | Modify |
| `src/components/__tests__/Board.test.tsx` | 보드 UI 테스트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 단축키 처리 일관성 | `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | 전역 키다운 핸들러는 입력 캡처 상태를 존중하고 `preventDefault`를 최소 범위로 적용한다 |
| 플랫폼 분기 | `src/desktop/renderer/utils/keyboardShortcut.ts` | `Mod` 추상화를 유지하고 맥/비맥 동작은 유틸에서 검증한다 |
| 테스트 패턴 | `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` | Testing Library로 사용자 이벤트 중심의 동작을 검증한다 |
| 네이밍/단순성 | `.claude/core/CODE_PRINCIPLES.md` | 역할이 드러나는 이름을 쓰고 작은 단위로 구현한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| `Cmd/Ctrl+F` 범위 | 보드 화면 전용 검색 바 | 요구사항이 칸반 창 프로젝트/텍스트 찾기에 한정됨 | 앱 전역 검색 바 |
| 검색 구현 | 렌더러 DOM 기반 `window.find` 래핑 | Electron 메인 IPC 없이 현재 화면 텍스트를 바로 찾을 수 있음 | `webContents.findInPage` IPC 연동 |
| 전역 검색 단축키 | `Mod+Shift+O` 유지 | 기존 설정 구조와 저장 포맷을 바꾸지 않음 | 플랫폼별 문자열 저장 |

## Implementation Todos

### Todo 1: 단축키와 보드 검색 동작을 테스트로 고정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 보드 전용 `Cmd/Ctrl+F` 검색과 전역 태스크 검색의 플랫폼별 단축키 기대값을 실패하는 테스트로 먼저 정의한다
- **Work**:
  - `src/components/__tests__/Board.test.tsx`에 보드 검색 바 열기, 검색어 입력, 다음/이전/닫기 동작 테스트 추가
  - `window.find`를 테스트 환경에서 mock 하여 호출 인자와 반환 흐름을 검증
  - `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` 또는 `keyboardShortcut` 테스트에 맥에서는 `Meta+Shift+O`, 리눅스/비맥에서는 `Ctrl+Shift+O`가 매칭되는 테스트 추가
- **Convention Notes**: 테스트 이름은 행동을 드러내고 한 테스트에서 한 가지 시나리오만 다룬다
- **Verification**: `pnpm test -- --runInBand src/components/__tests__/Board.test.tsx src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx src/desktop/renderer/utils/__tests__/keyboardShortcut.test.ts`
- **Exit Criteria**: 새 테스트가 구현 전 상태에서 기대한 이유로 실패한다
- **Status**: completed

### Todo 2: 보드 전용 페이지 검색 바 구현
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 보드 화면에서만 `Cmd/Ctrl+F`로 작은 검색 바를 열고 현재 보이는 텍스트를 순방향/역방향으로 찾게 한다
- **Work**:
  - `src/components/Board.tsx`에 보드 전용 검색 바 상태와 키다운 핸들러 추가 또는 전용 하위 컴포넌트로 분리
  - 검색 바 열기 시 입력 포커스, `Enter` 다음 찾기, `Shift+Enter` 이전 찾기, 버튼 기반 다음/이전, `Esc` 닫기 처리
  - 검색 결과 없음 상태를 UI로 표시하고 닫을 때 선택/상태를 정리
  - `messages/*.json`에 검색 바 제목/placeholder/버튼/없음 상태 문구 추가
- **Convention Notes**: 보드의 기존 상태 로직을 불필요하게 섞지 않도록 검색 UI 상태는 국소적으로 관리한다
- **Verification**: Todo 1의 테스트 재실행
- **Exit Criteria**: 보드 화면에서 요구한 검색 UX가 테스트와 수동 시나리오 기준으로 동작한다
- **Status**: completed

### Todo 3: 전역 태스크 검색 단축키 플랫폼 기대 동작 보강
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 기존 `Mod+Shift+O` 전역 검색이 맥과 비맥에서 각각 기대한 조합으로 동작하고 표시되도록 보장한다
- **Work**:
  - `src/desktop/renderer/utils/keyboardShortcut.ts`에서 필요한 경우 플랫폼 분기 보조 유틸 추가
  - `TaskQuickSearchDialog`의 표시 문자열과 단축키 매칭이 플랫폼별 기대와 일치하는지 보강
  - 기존 설정값이 비어 있을 때도 기본 단축키가 의도대로 동작하는지 확인
- **Convention Notes**: 저장 포맷은 기존 `Mod+Shift+O`를 유지하고, 플랫폼 차이는 유틸 계층에 둔다
- **Verification**: Todo 1의 관련 테스트 재실행
- **Exit Criteria**: 맥/비맥 단축키 테스트가 모두 통과한다
- **Status**: completed

## Verification Strategy
전체 구현 후 관련 단위 테스트와 타입 검사를 실행해 검색 UI 동작과 단축키 회귀를 함께 검증한다.
- `pnpm test -- --runInBand src/components/__tests__/Board.test.tsx src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx src/desktop/renderer/utils/__tests__/keyboardShortcut.test.ts`
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-04-30: Plan created
- 2026-04-30: Todo 1 completed — 보드 페이지 검색과 플랫폼별 전역 검색 단축키를 테스트로 고정
- 2026-04-30: Todo 2 completed — 보드 전용 Cmd/Ctrl+F 페이지 검색 바와 UI 문구를 추가
- 2026-04-30: Todo 3 completed — 기존 Mod 기반 전역 검색 단축키의 맥/비맥 동작을 회귀 테스트로 검증
- 2026-04-30: Final verification completed — 관련 테스트와 타입 체크 통과
