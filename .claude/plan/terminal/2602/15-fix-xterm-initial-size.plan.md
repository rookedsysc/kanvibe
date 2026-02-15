# Fix xterm 초기 터미널 크기 불일치

## Business Goal

xterm 터미널에 최초 접속 시 tmux 윈도우가 브라우저 터미널의 전체 크기를 채우지 않는 문제를 해결한다. PTY 생성 시 클라이언트의 실제 터미널 크기를 사용하여 초기 크기 불일치를 제거한다.

## Scope
- **In Scope**: PTY 초기 크기를 클라이언트 실제 크기와 일치시키기 (로컬 세션 + 원격 SSH 세션)
- **Out of Scope**: tmux 설정 변경, 기타 터미널 기능, CSS 레이아웃 변경

## Codebase Analysis Summary

현재 흐름:
1. 클라이언트(`Terminal.tsx`): `fitAddon.fit()`으로 브라우저 크기 계산 → WebSocket 연결 → `ws.onopen`에서 resize 메시지 전송
2. 서버(`server.ts`): WebSocket 연결 수신 → `attachLocalSession`/`attachRemoteSession` 호출
3. 서버(`terminal.ts`): PTY를 하드코딩된 `cols: 120, rows: 30`으로 생성 → resize 메시지 수신 시 `ptyProcess.resize()` 호출

문제: PTY가 120x30으로 생성된 후 tmux attach → 이후에야 클라이언트 크기로 resize. 이 초기 불일치로 tmux가 전체 영역을 사용하지 못함.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Terminal.tsx` | 클라이언트 xterm 터미널 컴포넌트 | Modify |
| `server.ts` | WebSocket 서버, 터미널 연결 라우팅 | Modify |
| `src/lib/terminal.ts` | PTY 생성 및 세션 관리 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석/답변은 한국어 |
| 폴백 값 유지 | 기존 코드 패턴 | 쿼리 파라미터 없을 때 기존 120x30 유지 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 크기 전달 방식 | WebSocket URL 쿼리 파라미터 | 가장 간단, PTY 생성 전에 크기를 알 수 있음 | 첫 메시지 대기 후 PTY 생성 (복잡도 증가) |
| fit 타이밍 | fitAddon.fit()을 await하고 WebSocket을 그 이후에 생성 | 정확한 크기 측정 후 연결 보장 | 동기 fit (부정확할 수 있음) |
| 폴백 크기 | 기존 120x30 유지 | 하위 호환성 보장 | 80x24 (xterm 기본값) |

## Implementation Todos

### Todo 1: Terminal.tsx — fitAddon.fit() 이후 WebSocket 생성 및 URL에 크기 전달
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `fitAddon.fit()` 실행 완료 후 정확한 cols/rows를 WebSocket URL 쿼리 파라미터로 전달
- **Work**:
  - `requestAnimationFrame(() => fitAddon.fit())` 호출을 Promise로 감싸서 await
  - fit 이후 `term.cols`, `term.rows`를 WebSocket URL에 `?cols=${term.cols}&rows=${term.rows}` 형태로 추가
  - WebSocket 생성 코드를 fit 완료 이후로 이동
- **Convention Notes**: 기존 코드 스타일 유지 (async/await 패턴)
- **Verification**: 브라우저에서 터미널 접속 시 WebSocket URL에 cols/rows 파라미터가 포함되는지 확인
- **Exit Criteria**: WebSocket URL에 실제 터미널 크기가 쿼리 파라미터로 포함됨
- **Status**: pending

### Todo 2: server.ts — URL에서 cols/rows 파싱하여 attach 함수에 전달
- **Priority**: 1
- **Dependencies**: none
- **Goal**: WebSocket 연결 URL에서 초기 터미널 크기를 추출하여 attach 함수에 전달
- **Work**:
  - `parse(request.url)` 결과에서 `query` 파라미터 추출
  - `cols`, `rows`를 정수로 파싱 (폴백: 120, 30)
  - `attachLocalSession`, `attachRemoteSession` 호출 시 `cols`, `rows` 인자 추가
- **Convention Notes**: `url.parse`의 query 파싱 사용 또는 `URLSearchParams` 활용
- **Verification**: 서버 로그에서 올바른 cols/rows 값이 파싱되는지 확인
- **Exit Criteria**: attach 함수에 클라이언트 크기가 정확히 전달됨
- **Status**: pending

### Todo 3: terminal.ts — attachLocalSession/attachRemoteSession에서 동적 크기 사용
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: PTY 생성 및 SSH shell 시 하드코딩된 120x30 대신 전달받은 크기 사용
- **Work**:
  - `attachLocalSession` 시그니처에 `cols?: number`, `rows?: number` 파라미터 추가
  - `pty.spawn()` 호출 시 `cols: cols ?? 120`, `rows: rows ?? 30` 사용
  - `attachRemoteSession` 시그니처에도 동일하게 추가
  - `conn.shell()` 호출 시 `cols: cols ?? 120`, `rows: rows ?? 30` 사용
- **Convention Notes**: 옵셔널 파라미터로 하위 호환성 유지
- **Verification**: PTY 생성 시 전달된 크기가 사용되는지 확인
- **Exit Criteria**: PTY가 클라이언트 실제 크기로 생성됨
- **Status**: pending

## Verification Strategy
- 브라우저에서 터미널 접속 시 tmux가 전체 브라우저 터미널 영역을 채우는지 시각적 확인
- 다양한 브라우저 창 크기에서 테스트
- `npm run build`로 빌드 오류 없음 확인

## Progress Tracking
- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-15: Plan created
