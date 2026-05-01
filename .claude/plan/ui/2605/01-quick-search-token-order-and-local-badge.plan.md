# Quick Search Token Order And Local Badge

## Business Goal
`Cmd/Ctrl+Shift+O` 빠른 검색에서 사용자가 검색어 순서를 신경 쓰지 않고 task를 찾을 수 있게 하고, 로컬 task에 불필요한 `local` 배지를 제거해 검색 결과를 더 간결하게 만든다.

## Scope
- **In Scope**: 빠른 검색의 토큰 순서 무관 매칭 지원, 결과 점수 계산 조정, 로컬 배지 제거, 관련 테스트 추가 및 수정
- **Out of Scope**: 검색 대상 필드 추가, 외부 검색 라이브러리 도입, 원격 task 표시 UX 전면 개편

## Codebase Analysis Summary
빠른 검색 기능은 `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` 안에서 완결적으로 구현되어 있다. 현재 검색은 `src/utils/fuzzySearch.ts`의 단일 query subsequence 매칭을 각 필드에 개별 적용하는 구조라, 공백으로 분리된 여러 단어의 순서가 바뀌면 의도한 결과를 안정적으로 찾기 어렵다. UI는 결과 우측에 원격/로컬 뱃지를 렌더링하며, 테스트는 `TaskQuickSearchDialog.test.tsx`에서 키보드 오픈, 검색, 네비게이션, 표시 상태를 검증한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | 빠른 검색 결과 계산과 결과 UI 렌더링 | Modify |
| `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` | 빠른 검색 동작 테스트 | Modify |
| `.claude/plan/ui/2605/01-quick-search-token-order-and-local-badge.plan.md` | 이번 작업 실행 계획 | Create / Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 검색 변경 범위 최소화 | 기존 `TaskQuickSearchDialog.tsx` 구조 | 기존 컴포넌트 내부 계산 흐름을 유지하고 필요한 로직만 국소적으로 수정한다 |
| 테스트 우선 | `superpowers:test-driven-development` | 구현 전에 실패하는 테스트를 먼저 추가하고 실패를 확인한 뒤 코드를 수정한다 |
| UI 표현 유지 | 기존 결과 렌더링 패턴 | 원격 호스트 표시는 유지하고, 요청된 로컬 배지만 제거한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 검색 규칙 | 공백 기준 토큰 분리 후 전체 토큰 AND 매칭 | 검색어 순서와 필드 분산 여부에 관계없이 원하는 task를 찾게 한다 | 전체 query 문자열 단일 fuzzy 매칭 |
| 점수 계산 | 토큰별 최고 매칭 점수 누적 + 필드 가중치 유지 | 기존 우선순위를 크게 깨지 않으면서 다중 토큰 결과를 정렬할 수 있다 | 별도 랭킹 알고리즘 도입 |
| 배지 처리 | `remote`만 노출, `local`은 미노출 | 사용자 요청을 최소 변경으로 정확히 반영한다 | 로컬/원격 모두 배지 제거 |

## API Contracts (if applicable)
해당 없음.

## Data Models (if applicable)
해당 없음.

## Implementation Todos

### Todo 1: 빠른 검색 동작 테스트 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 순서 무관 검색과 로컬 배지 제거 요구를 재현하는 실패 테스트를 만든다
- **Work**:
  - `src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx`에 다중 토큰 순서 무관 검색 케이스를 추가한다
  - 같은 테스트 파일에 로컬 task는 배지를 렌더링하지 않고 원격 task만 `remote`와 호스트를 보여주는 케이스를 추가한다
  - 기존 테스트 데이터로 요구 재현이 어렵다면 필요한 최소 fixture만 보강한다
- **Convention Notes**: 테스트 이름은 동작 중심으로 작성하고, 한 테스트는 한 행동만 검증한다
- **Verification**: `pnpm test src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx`
- **Exit Criteria**: 새 테스트가 추가되고, 구현 전 상태에서 기대한 이유로 실패한다
- **Status**: completed

### Todo 2: 검색 로직과 결과 UI 수정
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 토큰 순서 무관 검색이 동작하고 로컬 배지가 사라지도록 구현한다
- **Work**:
  - `src/desktop/renderer/components/TaskQuickSearchDialog.tsx`에서 query를 토큰화하고 각 토큰이 `branchName`, `projectName`, `title` 중 어디에서든 매칭되는지 평가하도록 검색 계산을 수정한다
  - 결과 하이라이트는 기존 `HighlightedText`를 유지하되 대표 필드에 사용할 인덱스를 계산 가능한 범위 내에서 합성한다
  - 결과 우측 렌더링에서 로컬 배지를 제거하고 원격 task에만 `remote` 배지와 `sshHost`를 유지한다
- **Convention Notes**: 새 유틸 추가보다 기존 컴포넌트 내부 로직 재구성을 우선하고, 타입은 명시적으로 유지한다
- **Verification**: `pnpm test src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx`
- **Exit Criteria**: 새 테스트와 기존 빠른 검색 테스트가 모두 통과한다
- **Status**: completed

### Todo 3: 최종 검증 및 정리
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 변경이 타입/린트/테스트 관점에서 문제 없는지 확인하고 계획 파일을 완료 상태로 갱신한다
- **Work**:
  - 빠른 검색 관련 테스트를 다시 실행해 회귀가 없는지 확인한다
  - 가능하면 `pnpm check` 또는 최소 범위 검증 명령을 실행한다
  - 결과에 맞춰 계획 파일의 진행 상태와 변경 로그를 갱신한다
- **Convention Notes**: 검증 없이 완료 주장하지 않고, 실패 시 원인을 먼저 수정한다
- **Verification**: `pnpm test src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx` and `pnpm check`
- **Exit Criteria**: 계획 파일의 모든 todo가 완료 상태가 되고 검증 결과가 기록된다
- **Status**: completed

## Verification Strategy
빠른 검색 컴포넌트의 단위 테스트를 우선 검증 기준으로 사용하고, 타입 체크로 런타임 전 문제를 확인한다.
- `pnpm test src/desktop/renderer/components/__tests__/TaskQuickSearchDialog.test.tsx`
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-05-01: Plan created
- 2026-05-01: Todo 1 completed - 순서 무관 검색과 로컬 배지 제거를 재현하는 실패 테스트 추가
- 2026-05-01: Todo 2 completed - 다중 토큰 검색 집계와 원격 전용 배지 렌더링 구현
- 2026-05-01: Todo 3 completed - 빠른 검색 테스트와 타입 체크로 최종 검증 완료
