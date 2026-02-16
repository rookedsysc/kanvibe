# Fix: 뒤로가기 시 칸반 보드 캐시 데이터 표시 문제

## Business Goal
칸반 보드에서 태스크 상세 페이지로 이동 후 뒤로가기하면 Next.js 라우터 캐시로 인해 최신 데이터가 아닌 이전 캐싱 데이터가 표시되는 문제를 수정한다.

## Scope
- **In Scope**: `src/hooks/useAutoRefresh.ts` 수정 — 모듈 레벨 플래그로 재마운트 감지 후 `router.refresh()` 호출, 비동작 `popstate` 리스너 제거
- **Out of Scope**: 서버 캐싱 설정, next.config.ts, 다른 컴포넌트 변경

## Codebase Analysis Summary
- `HomePage` (`src/app/[locale]/page.tsx`)는 `dynamic = "force-dynamic"`으로 서버 캐싱 비활성화됨
- `Board` 컴포넌트가 `useAutoRefresh()` 훅을 호출하며, 이 훅에 WebSocket + popstate 리스너가 있음
- `popstate` 리스너는 Board 언마운트 후 이벤트가 발생하므로 실제 동작하지 않음
- Board는 `initialTasks` props를 `useState`로 관리하며, `useEffect`로 변경 감지하여 동기화

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/hooks/useAutoRefresh.ts` | 자동 갱신 훅 (WebSocket + popstate) | Modify |
| `src/components/Board.tsx` | 칸반 보드 메인 컴포넌트 | Reference |
| `src/app/[locale]/page.tsx` | 홈페이지 서버 컴포넌트 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 주석 언어 | CODE_PRINCIPLES.md | 한국어로 작성 |
| 함수 단일 책임 | CODE_PRINCIPLES.md | 하나의 함수는 한 가지 일만 수행 |
| YAGNI | CODE_PRINCIPLES.md | 현재 필요한 것만 구현 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 재마운트 감지 방식 | 모듈 레벨 플래그 | 간단하고 확실, SPA 내비게이션에서 동작 | pageshow 이벤트(bfcache만), layout 레벨 리스너(과도한 변경) |
| popstate 리스너 | 제거 | Board 언마운트 시 해제되어 타이밍 문제로 동작 불가 | 유지(무해하지만 dead code) |

## Implementation Todos

### Todo 1: useAutoRefresh 훅 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 뒤로가기로 Board가 재마운트될 때 `router.refresh()`를 호출하여 최신 데이터를 로드한다
- **Work**:
  - `src/hooks/useAutoRefresh.ts`에 모듈 레벨 변수 `boardHasMountedBefore` 추가 (초기값 `false`)
  - 마운트 시 `boardHasMountedBefore`가 `true`이면 `router.refresh()` 호출
  - 마운트 후 `boardHasMountedBefore = true` 설정
  - 기존 `popstate` useEffect 블록 제거 (동작하지 않는 dead code)
  - 기존 WebSocket useEffect 블록은 유지
- **Convention Notes**: 주석은 한국어, JSDoc 스타일 준수
- **Verification**: `pnpm build` 성공 확인
- **Exit Criteria**: `useAutoRefresh.ts`에 모듈 레벨 플래그 기반 refresh 로직이 존재하고, popstate 리스너가 제거됨
- **Status**: completed

## Verification Strategy
- `pnpm build` 성공
- 코드 리뷰: 모듈 레벨 변수와 useEffect 로직 확인

## Progress Tracking
- Total Todos: 1
- Completed: 1
- Status: Execution complete

## Change Log
- 2026-02-16: Plan created
- 2026-02-16: Todo 1 completed — useAutoRefresh 훅 수정
