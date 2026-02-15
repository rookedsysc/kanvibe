# ProjectSelector 단일 선택 모드 드롭다운 UI 변경

## Business Goal
새 작업 생성 모달에서 프로젝트 선택 시, 기존 input 기반 UI가 사용하기 불편하므로 검색 필터의 멀티 선택 드롭다운 스타일과 동일한 UX로 변경하여 프로젝트 선택 편의성을 향상시킨다.

## Scope
- **In Scope**: `ProjectSelector.tsx`의 단일 선택 모드(line 333~461) 렌더링 변경
- **Out of Scope**: 멀티 선택 모드 변경 없음, CreateTaskModal 로직 변경 없음, Board.tsx 변경 없음

## Codebase Analysis Summary
`ProjectSelector`는 discriminated union props로 single/multi 모드를 지원한다.
- 멀티 모드: 클릭 트리거(칩 표시) + 드롭다운(검색 input + 체크박스 리스트)
- 단일 모드: input 자체가 트리거 겸 검색 → 포커스 시 기존 프로젝트명 pre-fill, 혼란 유발

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/ProjectSelector.tsx` | 프로젝트 선택 컴포넌트 | Modify |
| `src/components/CreateTaskModal.tsx` | 새 작업 생성 모달 (단일 선택 사용처) | Reference |
| `src/components/Board.tsx` | 칸반 보드 (멀티 선택 사용처) | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Tailwind CSS 변수 토큰 | CLAUDE.md | `bg-bg-page`, `text-text-primary` 등 사용 |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어로 작성 |
| 기존 멀티 선택 스타일 | ProjectSelector.tsx line 205~330 | 동일한 className 패턴 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 트리거 UI | 클릭 트리거 div + 선택된 프로젝트 텍스트 표시 | 멀티 모드와 일관된 UX | 기존 input 유지 |
| 검색 위치 | 드롭다운 내부 검색 input | 멀티 모드와 동일한 패턴 | 트리거 자체를 검색으로 유지 |
| 선택 시 동작 | 즉시 선택 + 드롭다운 닫힘 + 검색 초기화 | 단일 선택이므로 선택 완료 시 닫는 것이 자연스러움 | 드롭다운 유지 |

## Implementation Todos

### Todo 1: 단일 선택 모드 렌더링을 드롭다운 스타일로 변경
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 단일 선택 모드의 UI를 멀티 선택 드롭다운 스타일(클릭 트리거 + 드롭다운 내부 검색)로 변경
- **Work**:
  - `ProjectSelector.tsx`의 단일 선택 렌더링(line 387~461)을 멀티 선택 렌더링(line 205~330) 스타일로 교체
  - 트리거 div: 선택된 프로젝트명을 텍스트로 표시 (칩 스타일 없이 단순 텍스트), 없으면 placeholder
  - 드롭다운: 검색 input(상단) + 프로젝트 리스트(체크박스 없이, 선택된 항목은 `font-medium`으로 강조)
  - `allOption`이 있는 경우 드롭다운 상단에 "전체" 옵션 유지
  - 포커스 자동 이동: 드롭다운 열릴 때 `searchInputRef`에 포커스
  - 키보드 네비게이션: 기존 `handleSingleKeyDown` 로직 유지 (ArrowDown/Up, Enter, Escape)
  - `triggerInputRef` 제거 (더 이상 input이 트리거가 아님)
- **Convention Notes**: 멀티 선택 모드의 className 패턴 그대로 재사용 (compact 지원 포함)
- **Verification**: 빌드 성공 (`npm run build`), CreateTaskModal에서 프로젝트 선택/검색/변경 동작 확인
- **Exit Criteria**: 단일 선택 모드가 드롭다운 스타일로 동작하며, 프로젝트 검색/선택/키보드 네비게이션이 정상 동작
- **Status**: completed

## Verification Strategy
- `npm run build` 성공
- CreateTaskModal에서 프로젝트 선택, 검색, 변경 동작 확인
- Board.tsx의 멀티 선택 필터가 영향받지 않음 확인

## Progress Tracking
- Total Todos: 1
- Completed: 1
- Status: Execution complete

## Change Log
- 2026-02-16: Plan created
