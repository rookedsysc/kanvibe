# 프로젝트 필터 Query String 전환

## Business Goal
프로젝트 필터를 localStorage 대신 URL query string으로 관리하여, 브라우저 탭마다 독립적인 필터 상태를 유지한다. 사용자가 여러 탭을 열어 서로 다른 프로젝트를 필터링하면서 작업할 수 있도록 한다.

## Scope
- **In Scope**: Board.tsx의 필터 상태를 query string(`?projects=id1,id2`)으로 전환, localStorage 관련 코드 제거
- **Out of Scope**: 다른 상태(정렬, 검색어 등)의 query string 전환

## Codebase Analysis Summary
- `Board.tsx:104` — `useState<string[]>([])` 로 `selectedProjectIds` 관리
- `Board.tsx:255-273` — `localStorage` 읽기(mount시) / 쓰기(변경시) useEffect 2개
- `Board.tsx:38` — `FILTER_STORAGE_KEY = "kanvibe:projectFilter"` 상수
- `ProjectSelector.tsx` — Board로부터 `selectedProjectIds`와 `onSelectionChange`를 props로 받음 (변경 불필요)
- `src/app/[locale]/page.tsx` — 서버 컴포넌트, Board를 렌더링

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Board.tsx` | 메인 보드 컴포넌트, 필터 상태 관리 | Modify |
| `src/components/ProjectSelector.tsx` | 프로젝트 셀렉터 UI | Reference (변경 없음) |
| `src/app/[locale]/page.tsx` | 홈페이지 서버 컴포넌트 | Reference (변경 없음) |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | JSDoc 주석은 한국어로 작성 |
| useMemo/useCallback | Board.tsx 기존 패턴 | 메모이제이션 패턴 유지 |
| "use client" | Board.tsx | 클라이언트 컴포넌트 유지 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Query param format | `?projects=id1,id2` | 간결하고 URL 가독성 좋음 | `?projects=id1&projects=id2` |
| URL 업데이트 방식 | `window.history.replaceState` | 리렌더/스크롤 없이 URL만 변경 | `router.replace` (전체 리렌더 유발) |
| 초기값 읽기 | `useSearchParams()` | Next.js 표준 방식 | URL 직접 파싱 |
| 빈 필터 처리 | query param 제거 | 깔끔한 기본 URL | `?projects=` 빈 값 유지 |

## Implementation Todos

### Todo 1: Board.tsx 필터 상태를 query string 기반으로 전환
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `selectedProjectIds` 상태를 URL query string에서 읽고, 변경 시 URL을 업데이트하도록 전환
- **Work**:
  - `next/navigation`에서 `useSearchParams` import 추가
  - `FILTER_STORAGE_KEY` 상수 제거
  - `selectedProjectIds` 초기값을 `useSearchParams()`에서 `projects` param 파싱하여 설정
  - `setSelectedProjectIds` 를 래핑하여 state 변경 시 `window.history.replaceState`로 URL도 업데이트
  - localStorage 읽기 useEffect (L255-265 영역) 제거
  - localStorage 쓰기 useEffect (L267-273 영역) 제거
  - `useSearchParams`는 `Suspense` 바운더리가 필요하므로, 필요시 page.tsx에 Suspense 래핑 추가
- **Convention Notes**: 기존 useMemo/useCallback 패턴 유지, 한국어 주석
- **Verification**: `pnpm build` 성공, 브라우저에서 필터 선택 시 URL 변경 확인
- **Exit Criteria**:
  - 필터 선택 시 URL에 `?projects=id1,id2` 반영
  - 필터 해제 시 query param 제거
  - URL에 projects param이 있는 상태로 페이지 로드 시 필터 복원
  - 서로 다른 탭에서 독립적인 필터 유지
- **Status**: completed

## Verification Strategy
- `pnpm build` 성공
- 브라우저에서 다음 시나리오 확인:
  1. 필터 선택 → URL에 `?projects=...` 반영
  2. 필터 해제 → URL에서 param 제거
  3. URL 직접 입력 → 해당 필터 적용
  4. 두 탭에서 서로 다른 필터 독립 동작

## Progress Tracking
- Total Todos: 1
- Completed: 1
- Status: Execution complete

## Change Log
- 2026-02-21: Plan created
- 2026-02-21: Todo 1 completed — Build, lint, tests all pass
