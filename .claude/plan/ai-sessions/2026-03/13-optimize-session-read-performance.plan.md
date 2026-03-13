# AI Session Read Performance Optimization

## Business Goal
readClaudeSessions, readCodexSessions, readOpenCodeSessions에서 세션 목록 및 메시지 로딩이 느린 문제를 해결한다.
주요 원인은 (1) 파일 순차 처리, (2) 전체 파일 메모리 로드, (3) 반복 호출 시 캐시 없음이다.

## Scope
- **In Scope**: `shared.ts` 캐시 유틸 추가, `readJsonLinesHead` 추가, `readClaudeSessions`/`readCodexSessions` 병렬화 + 조기 종료, mtime 기반 캐시 적용
- **Out of Scope**: 파일 watch 실시간 캐시, DB 도입, Gemini 파싱 구현

## Codebase Analysis Summary

Claude/Codex는 JSONL 파일 기반(파일 하나 = 세션 하나), OpenCode는 SQLite 기반.
`shared.ts`의 `readJsonLines`는 파일 전체를 메모리에 올리고 split/parse.
`readClaudeSessions`/`readCodexSessions`는 `for...of`로 파일 순차 처리.
세션 목록 조회 시 tool call 이벤트(progress, response_item 등)까지 전부 파싱.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/lib/aiSessions/shared.ts` | 공통 유틸 (readJsonLines, extractPlainText 등) | Modify |
| `src/lib/aiSessions/readClaudeSessions.ts` | Claude JSONL 읽기 | Modify |
| `src/lib/aiSessions/readCodexSessions.ts` | Codex JSONL 읽기 | Modify |
| `src/lib/aiSessions/readOpenCodeSessions.ts` | OpenCode SQLite 읽기 | Reference (이미 단일 쿼리) |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| async 함수 | 기존 코드 | 모든 I/O는 async/await |
| 타입 안전 | 기존 코드 | unknown으로 받아 타입 가드로 좁힘 |
| export 방식 | 기존 코드 | named export, 파일 하단에 private 함수 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 캐시 위치 | 모듈 레벨 Map (`shared.ts`) | Next.js 서버는 long-lived process | Redis, 파일 캐시 |
| 캐시 키 | `filePath + ':' + mtime` | 파일 변경 시 자동 무효화 | 시간 TTL |
| 조기 종료 방식 | `readline` 기반 `readJsonLinesHead(path, maxLines)` | 기존 `readJsonLines` 시그니처 유지 | 스트림 없이 전체 읽기 |
| 병렬화 단위 | 파일 단위 `Promise.all` | 각 파일이 독립적 | Worker thread |

## Implementation Todos

### Todo 1: shared.ts에 mtime 캐시 + readJsonLinesHead 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 전체 파일 로드 없이 앞 N줄만 읽는 유틸 + 파싱 결과 캐시 인프라 제공
- **Work**:
  - `shared.ts`에 `import { createReadStream } from "fs"` 및 `import { createInterface } from "readline"` 추가
  - `FileParseCache<T>` 타입 정의: `{ mtime: number; result: T }`
  - `fileParseCache` Map 전역 선언: `Map<string, FileParseCache<unknown>>`
  - `getCachedOrParse<T>(filePath, parseFn)` 함수 추가:
    - `stat(filePath)`로 mtime 조회
    - 캐시 hit (mtime 동일) → 캐시 반환
    - 캐시 miss → `parseFn()` 실행 → 결과 캐시 저장 후 반환
  - `readJsonLinesHead(filePath, maxLines)` 함수 추가:
    - `readline.createInterface` + `createReadStream`으로 라인 스트리밍
    - `maxLines`개 읽으면 스트림 destroy 후 반환
    - 반환 타입: `Promise<unknown[]>`
- **Convention Notes**: `async function`, named export, 기존 `readJsonLines` 시그니처 변경 없음
- **Verification**: TypeScript 빌드 통과 (`pnpm tsc --noEmit`)
- **Exit Criteria**: `getCachedOrParse`, `readJsonLinesHead`가 export되고 빌드 에러 없음
- **Status**: completed

### Todo 2: readClaudeSessions 병렬화 + 조기 종료 + 캐시 적용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 세션 목록 조회 시 파일 병렬 처리 + 앞 50줄만 읽어 메타데이터 추출
- **Work**:
  - `readClaudeSessions`: `for...of` 루프 → `Promise.all(projectFiles.map(...))`로 교체
  - `consumeClaudeListEvent` 호출 로직을 `parseClaudeSessionFromFile(filePath, context)` 함수로 분리
  - `parseClaudeSessionFromFile` 내부에서 `getCachedOrParse(filePath, () => readJsonLinesHead(filePath, 60))` 사용
  - 60줄 이내에 sessionId, cwd, 첫 user message가 없으면 fallback으로 `readJsonLines` 사용
  - `readClaudeSessionDetail`: `sourceRef` 있을 때 파일 직접 읽으므로 현재 구조 유지, 단 `getCachedOrParse` 적용
- **Convention Notes**: private 함수는 파일 하단, 타입은 기존 `ClaudeProjectEvent` 재사용
- **Verification**: TypeScript 빌드 통과, `NODE_ENV=test pnpm test` 통과
- **Exit Criteria**: 빌드 에러 없음, 기존 테스트 통과
- **Status**: completed

### Todo 3: readCodexSessions 병렬화 + 캐시 적용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Codex 세션 목록 조회 시 파일 병렬 처리 + 캐시 적용
- **Work**:
  - `readCodexSessions`: `for...of` 루프 → `Promise.all(rolloutFiles.map(...))` 교체
  - `parseCodexSessionSummary`에 `getCachedOrParse` 적용:
    - 캐시 키는 filePath (mtime으로 자동 무효화)
    - 파싱 결과(`AggregatedAiSession | null`)를 캐시
  - `readCodexSessionDetail`: sourceRef 있으면 단일 파일만 읽으므로 `getCachedOrParse` 적용
- **Convention Notes**: 기존 `parseCodexSessionSummary` 시그니처 유지
- **Verification**: TypeScript 빌드 통과, `NODE_ENV=test pnpm test` 통과
- **Exit Criteria**: 빌드 에러 없음, 기존 테스트 통과
- **Status**: completed

## Verification Strategy
- `pnpm tsc --noEmit` — 타입 에러 없음
- `NODE_ENV=test pnpm test` — 기존 테스트 전체 통과
- 실제 실행 시 AiSessionsCard 로딩 속도 체감 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-03-13: Plan created
- 2026-03-13: All todos completed
