# Fix AI Session Detail Message Order

## Business Goal
메시지 상세 보기에서 최신 대화가 먼저 보여야 사용자가 최근 작업 맥락을 즉시 파악할 수 있다. worktree와 main branch 세션을 함께 보더라도 상세 메시지 순서가 일관되게 최신순을 유지하도록 보장한다.

## Scope
- **In Scope**: AI 세션 상세 reader의 메시지 정렬 로직 수정, pagination 순서 보장, 회귀 테스트 추가
- **Out of Scope**: 세션 요약 카드 정렬 규칙 변경, 새로운 provider 추가, UI 레이아웃 변경

## Codebase Analysis Summary
세션 요약 목록은 이미 `sortSessionsDescending()`으로 최신순 정렬되고 있지만, 상세 메시지는 각 provider reader가 저장 순서 그대로 반환하고 있었다. UI는 반환 순서를 그대로 렌더링하고 `nextCursor` 페이지를 뒤에 이어붙이므로, reader 단계에서 최신순 정렬 후 pagination 해야 동작이 일관된다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/aiSessions/shared.ts` | 공통 정렬/파싱 유틸리티 | Modify |
| `src/lib/aiSessions/readClaudeSessions.ts` | Claude 상세 메시지 reader | Modify |
| `src/lib/aiSessions/readCodexSessions.ts` | Codex 상세 메시지 reader | Modify |
| `src/lib/aiSessions/readOpenCodeSessions.ts` | OpenCode 상세 메시지 reader | Modify |
| `src/lib/aiSessions/__tests__/shared.test.ts` | 공통 정렬 회귀 테스트 | Create |
| `src/components/__tests__/AiSessionsDialog.test.tsx` | 상세 UI pagination 동작 검증 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 단순한 공통 유틸 추가 | `.claude/core/CODE_PRINCIPLES.md` | YAGNI를 지키며 필요한 범위만 함수로 분리 |
| 테스트로 회귀 방지 | 기존 `src/lib/aiSessions/__tests__/aggregateAiSessions.test.ts` | 정렬 계약을 명시적으로 검증 |
| 프론트 상태 흐름 유지 | `src/components/AiSessionsDialog.tsx` 기존 패턴 | UI는 server 결과를 그대로 렌더링하고 reader에서 순서를 보장 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 상세 메시지 정렬 위치 | reader 단계에서 최신순 정렬 후 pagination | provider별 원본 저장 순서 차이를 UI에서 숨기고 load-more 커서를 안정적으로 유지 | UI 렌더 직전 정렬, DB/파일 조회 단계별 개별 구현 |
| 회귀 테스트 위치 | 공통 유틸 테스트 + UI pagination 테스트 | 정렬 규칙과 실제 append 순서를 둘 다 보장 | UI 테스트만 추가, provider별 통합 테스트만 추가 |

## Implementation Todos

### Todo 1: 공통 최신순 정렬 유틸 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 상세 메시지 배열을 timestamp 기준 최신순으로 재사용 가능하게 정렬한다.
- **Work**:
  - `src/lib/aiSessions/shared.ts`에 `sortMessagesDescending()` 추가
  - invalid timestamp 처리 규칙을 기존 세션 정렬 함수와 동일하게 유지
- **Convention Notes**: 기존 `sortSessionsDescending()` 패턴을 그대로 따라 예측 가능한 정렬 규칙을 유지한다.
- **Verification**: `src/lib/aiSessions/__tests__/shared.test.ts`
- **Exit Criteria**: 공통 함수가 추가되고 최신순 정렬 결과를 테스트로 증명한다.
- **Status**: completed

### Todo 2: provider detail reader 최신순 보장
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Claude, Codex, OpenCode 상세 메시지가 모두 최신순으로 반환되도록 맞춘다.
- **Work**:
  - `src/lib/aiSessions/readClaudeSessions.ts`에서 pagination 전 정렬 적용
  - `src/lib/aiSessions/readCodexSessions.ts`에서 pagination 전 정렬 적용
  - `src/lib/aiSessions/readOpenCodeSessions.ts`에서 메시지 배열 생성 후 정렬 적용
- **Convention Notes**: provider별 parsing 로직은 유지하고, 순서 보정만 최소 범위로 적용한다.
- **Verification**: `pnpm test -- --run src/lib/aiSessions/__tests__ src/components/__tests__/AiSessionsDialog.test.tsx`
- **Exit Criteria**: 모든 local provider detail reader가 최신순 메시지를 반환한다.
- **Status**: completed

### Todo 3: pagination 회귀 테스트 보강
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 첫 페이지는 최신 메시지, 추가 로드는 더 오래된 메시지가 뒤에 붙는 동작을 고정한다.
- **Work**:
  - `src/components/__tests__/AiSessionsDialog.test.tsx`의 load-more 케이스를 최신순 기준으로 수정
  - `src/lib/aiSessions/__tests__/shared.test.ts` 추가
- **Convention Notes**: 테스트 이름과 fixture는 실제 사용자 증상인 "상세 보기 최신순"을 직접 설명해야 한다.
- **Verification**: `pnpm test -- --run src/lib/aiSessions/__tests__ src/components/__tests__/AiSessionsDialog.test.tsx` and `pnpm check`
- **Exit Criteria**: 관련 테스트와 타입 체크가 모두 통과한다.
- **Status**: completed

## Verification Strategy
- `pnpm test -- --run src/lib/aiSessions/__tests__ src/components/__tests__/AiSessionsDialog.test.tsx`
- `pnpm check`

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-03-13: Plan created
- 2026-03-13: Todo 1 completed — 공통 상세 메시지 최신순 정렬 유틸 추가
- 2026-03-13: Todo 2 completed — Claude, Codex, OpenCode detail reader에 최신순 정렬 적용
- 2026-03-13: Todo 3 completed — 최신순 pagination 회귀 테스트와 타입 체크 통과
- 2026-03-13: Execution complete
