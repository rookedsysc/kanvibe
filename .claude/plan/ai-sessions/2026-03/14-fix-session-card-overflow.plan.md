# Fix AI Session Card Overflow

## Business Goal
AI 세션 목록 카드에서 긴 제목이나 미리보기 문자열이 카드 밖으로 튀어나오지 않게 만들어, 세션 목록을 안정적으로 읽을 수 있게 한다.

## Scope
- **In Scope**: 세션 카드의 제목/미리보기 overflow 스타일 수정, 긴 문자열 회귀 테스트 추가
- **Out of Scope**: 세션 제목 생성 규칙 변경, provider 파서 수정

## Codebase Analysis Summary
overflow는 `SessionList` 카드에서 제목과 미리보기 텍스트에 폭 제한 및 줄바꿈 제어가 부족해 발생한다. 데이터는 그대로 두고 렌더링 레벨에서 `min-w-0`, `overflow-hidden`, `break-words`, `line-clamp`를 적용하면 최소 범위로 해결 가능하다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/AiSessionsDialog.tsx` | AI 세션 목록 카드 렌더링 | Modify |
| `src/components/__tests__/AiSessionsDialog.test.tsx` | 세션 카드 UI 회귀 테스트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 최소 범위 수정 | `.claude/core/CODE_PRINCIPLES.md` | 데이터 계약은 건드리지 않고 UI 클래스만 조정 |
| 프론트 컴포넌트 구조 유지 | 기존 `AiSessionsDialog.tsx` | 기존 구조를 유지하면서 Tailwind 클래스만 보강 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| overflow 처리 위치 | `SessionList` 렌더링 컴포넌트 | 문제 원인과 가장 가까운 지점에서 해결 가능 | reader 단계 텍스트 축약 |
| 텍스트 표시 방식 | 제목/본문에 clamp와 break-words 적용 | 긴 한 줄 텍스트와 긴 토큰 문자열 모두 대응 가능 | 단일 truncate만 사용 |

## Implementation Todos

### Todo 1: 세션 카드 레이아웃 안정화
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 카드 내부 텍스트가 부모 너비를 넘지 않게 만든다.
- **Work**:
  - `src/components/AiSessionsDialog.tsx`의 `SessionList` 카드에 `overflow-hidden`, `min-w-0` 적용
  - 제목과 미리보기에 줄바꿈 및 clamp 스타일 추가
- **Convention Notes**: 구조 변경 없이 기존 컴포넌트 클래스만 조정한다.
- **Verification**: `AiSessionsDialog` 테스트
- **Exit Criteria**: 긴 문자열이 카드 밖으로 넘치지 않는다.
- **Status**: completed

### Todo 2: 긴 문자열 회귀 테스트 추가
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 긴 제목/미리보기 문자열이 있어도 카드가 안전한 클래스 구성을 유지하는지 보장한다.
- **Work**:
  - `src/components/__tests__/AiSessionsDialog.test.tsx`에 긴 세션 제목/미리보기 렌더 테스트 추가
- **Convention Notes**: 실제 사용자 증상을 반영하는 fixture를 사용한다.
- **Verification**: `pnpm test -- --run src/components/__tests__/AiSessionsDialog.test.tsx`
- **Exit Criteria**: 회귀 테스트가 통과한다.
- **Status**: completed

## Verification Strategy
- `pnpm test -- --run src/components/__tests__/AiSessionsDialog.test.tsx`
- 필요 시 `pnpm check`

## Progress Tracking
- Total Todos: 2
- Completed: 2
- Status: Execution complete

## Change Log
- 2026-03-14: Plan created
- 2026-03-14: Todo 1 completed — 세션 카드에 overflow-hidden과 줄바꿈/clamp 스타일 적용
- 2026-03-14: Todo 2 completed — 긴 제목/미리보기 문자열 회귀 테스트 추가
- 2026-03-14: Execution complete
