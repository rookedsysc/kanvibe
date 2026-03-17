# Full Rust Tauri Backend Migration

## Business Goal
기존 Next.js/Node 런타임을 제거하고, 모든 핵심 기능을 Rust backend + Tauri desktop runtime 위에서 동작하도록 재구성한다. 사용자는 더 이상 Docker나 브라우저 서버 실행 흐름 없이 데스크톱 앱만으로 작업 보드, worktree, 터미널, 훅, diff, AI 세션 기능을 사용할 수 있어야 한다.

## Scope
- **In Scope**: Tauri를 유일한 실행 진입점으로 만들기, Node/Next 서버 의존 제거, Rust command/event 기반 backend 설계 및 구현, SQLite 영속화 유지, 주요 기능 흐름 이관, 테스트/빌드/QA 통과
- **Out of Scope**: 기존 PostgreSQL 데이터 자동 이전 도구 제공, 모바일 지원, 웹 배포 유지

## Codebase Analysis Summary
현재 저장소는 Tauri가 `node boot.js`를 실행하는 얇은 래퍼에 불과하다. 대부분의 비즈니스 로직은 `src/app/actions/*.ts` 서버 액션과 `server.ts` 커스텀 서버, `src/lib/terminal.ts`/`src/lib/worktree.ts`/`src/lib/gitOperations.ts` 같은 Node 전용 모듈에 묶여 있다. UI는 React/Tailwind 기반이라 재사용 가능성이 높지만, 라우팅/데이터 접근은 Next App Router, next-intl, 서버 액션에 강하게 결합되어 있다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src-tauri/src/main.rs` | 현재 Node 런타임 스폰용 Tauri 엔트리 | Modify |
| `src-tauri/Cargo.toml` | Rust runtime 의존성 선언 | Modify |
| `server.ts` | Next + WebSocket 커스텀 서버 | Remove / Replace |
| `boot.js` | Node 서버 부트스트랩 | Remove / Replace |
| `src/app/actions/kanban.ts` | 작업/상태/워크트리 서버 액션 | Port |
| `src/app/actions/project.ts` | 프로젝트/훅/AI 세션 서버 액션 | Port |
| `src/app/actions/diff.ts` | diff/file 편집 액션 | Port |
| `src/app/actions/paneLayout.ts` | pane layout 액션 | Port |
| `src/app/actions/appSettings.ts` | 앱 설정 액션 | Port |
| `src/app/actions/auth.ts` | 인증 액션 | Port / Simplify |
| `src/lib/terminal.ts` | PTY/tmux/zellij/SSH 처리 | Port |
| `src/lib/worktree.ts` | worktree 생성/정리 | Port |
| `src/lib/gitOperations.ts` | git/gh/ssh 작업 | Port |
| `src/lib/database.ts` | SQLite TypeORM 레이어 | Reference / Transition |
| `src/components/Board.tsx` | 핵심 보드 UI | Modify |
| `src/components/Terminal.tsx` | xterm 렌더링 | Modify |
| `src/components/DiffPageClient.tsx` | diff UI | Modify |
| `src/components/ProjectSettings.tsx` | 프로젝트 관리 UI | Modify |
| `src/i18n/*` | next-intl 결합부 | Replace |
| `package.json` | 현재 Next/Tauri 혼합 스크립트 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 네이밍 명확성 | `.claude/core/CODE_PRINCIPLES.md` | 역할이 드러나는 전체 단어 사용 |
| 단일 책임 | `.claude/core/CODE_PRINCIPLES.md` | 함수/모듈을 도메인별로 분리 |
| 한국어 설명 | `.claude/core/CODE_PRINCIPLES.md` | 주석/문서 설명은 한국어 |
| 반응형 UI | `.claude/core/FRONTEND.md` | 모바일/데스크톱 모두 로드 가능해야 함 |
| 불필요한 추상화 금지 | `.claude/core/CODE_PRINCIPLES.md` | 현재 필요한 범위만 구현 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Desktop runtime | Tauri only | 사용자가 요구한 유일 실행 경로 | Node wrapper 유지 |
| Backend shape | Rust command + Tauri event | Node 서버 제거와 기능 이관에 적합 | 로컬 HTTP 서버 유지 |
| Persistence | SQLite 유지 | 이미 로컬 앱 성격과 맞고 Docker 제거 완료 | PostgreSQL 유지 |
| Frontend host | React SPA로 점진 이전 | 기존 UI 자산 재사용 가능 | UI 전체 재작성 |
| Notifications | Tauri 우선, 웹뷰 fallback 허용 | 데스크톱 동작에 맞춤 | service worker만 유지 |

## Data Models

### KanbanTask
| Field | Type | Constraints |
|-------|------|-------------|
| id | string | PK |
| title | string | required |
| status | enum | required |
| branchName | string | nullable |
| worktreePath | string | nullable |
| sessionType | enum | nullable |
| sessionName | string | nullable |
| sshHost | string | nullable |
| projectId | string | FK nullable |

### Project
| Field | Type | Constraints |
|-------|------|-------------|
| id | string | PK |
| name | string | unique |
| repoPath | string | required |
| defaultBranch | string | required |
| sshHost | string | nullable |
| isWorktree | boolean | required |
| color | string | nullable |

## Implementation Todos

### Todo 1: Rust backend target contract 정의
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Rust에서 대체해야 하는 command/event surface를 확정한다.
- **Work**:
  - `src-tauri/src/` 하위에 domain별 모듈 구조 초안을 만든다.
  - `tasks`, `projects`, `terminal_sessions`, `git_worktree`, `diff`, `settings`, `hooks`, `ai_sessions` command 목록을 정의한다.
  - 현재 `server.ts`, `src/app/actions/*.ts`, `src/app/api/**/*.ts`와의 매핑표를 남긴다.
- **Convention Notes**: domain별로 응집도 높게 분리한다.
- **Verification**: command 등록 코드가 컴파일된다.
- **Exit Criteria**: Rust 진입점에서 Node 스폰 없이 backend surface가 보인다.
- **Status**: completed

### Todo 2: Tauri 순수 엔트리로 전환
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `src-tauri/src/main.rs`가 더 이상 `node boot.js`를 실행하지 않게 만든다.
- **Work**:
  - Tauri window를 정적 SPA 또는 프론트 번들로 직접 연다.
  - `boot.js`, `server.ts` 제거 계획에 맞춰 실행 스크립트를 바꾼다.
  - `tauri.conf.json`의 localhost/ws 의존을 제거한다.
- **Convention Notes**: 사용자의 실제 실행 경로는 항상 Tauri여야 한다.
- **Verification**: `pnpm build` 또는 `cargo check`에서 Node spawn 경로가 사라진다.
- **Exit Criteria**: Tauri 실행만으로 앱이 열린다.
- **Status**: completed

### Todo 3: React frontend의 Next 결합 제거
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 프론트엔드가 Next App Router와 서버 액션 없이 동작한다.
- **Work**:
  - 페이지 구조를 SPA 엔트리로 옮긴다.
  - `next-intl`/`next/navigation`/서버 액션 import를 대체한다.
  - `Board`, `ProjectSettings`, `DiffPageClient`, `Terminal`, `AiSessionsDialog`를 invoke/event 기반으로 바꾼다.
- **Convention Notes**: 기존 UI와 스타일 자산은 최대한 재사용한다.
- **Verification**: 프론트 번들이 standalone으로 빌드된다.
- **Exit Criteria**: `src/app` 경유 없이 주요 화면이 로드된다.
- **Status**: completed

### Todo 4: Terminal/worktree/git/ssh 도메인을 Rust로 이관
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 가장 위험한 OS 통합 로직을 Rust backend로 옮긴다.
- **Work**:
  - `src/lib/terminal.ts`, `src/lib/worktree.ts`, `src/lib/gitOperations.ts`, `src/lib/sshConfig.ts`를 Rust 모듈로 대체한다.
  - PTY 출력 스트리밍과 resize/input 이벤트를 Tauri event로 연결한다.
  - tmux/zellij session 생성/attach 흐름을 유지한다.
- **Convention Notes**: 기능 동일성 우선, 과도한 리디자인 금지.
- **Verification**: 터미널/워크트리 관련 테스트와 수동 QA를 수행한다.
- **Exit Criteria**: Node 전용 PTY/worktree 경로가 핵심 런타임에서 제거된다.
- **Status**: completed

### Todo 5: 데이터/설정/훅/AI 세션 기능 이관
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 3, Todo 4
- **Goal**: Remaining backend feature set을 Rust command 기반으로 맞춘다.
- **Work**:
  - task/project/pane-layout/app-settings CRUD를 Rust repository/service로 옮긴다.
  - hook 설치/status, AI session aggregate/detail, diff file read/write를 Rust backend로 옮긴다.
  - 브로드캐스트는 `server.ts` 대신 Tauri events를 사용한다.
- **Convention Notes**: command 입력/출력 타입은 예측 가능한 구조를 유지한다.
- **Verification**: 관련 테스트 전부 통과, 주요 UI 플로우 수동 점검.
- **Exit Criteria**: `src/app/actions`, `src/app/api`, `src/lib/boardNotifier.ts` 의존이 사라진다.
- **Status**: completed

### Todo 6: 최종 정리 및 QA
- **Priority**: 4
- **Dependencies**: Todo 2, Todo 3, Todo 4, Todo 5
- **Goal**: Next/Node 잔재를 제거하고 최종 검증을 완료한다.
- **Work**:
  - `boot.js`, `server.ts`, `next.config.ts`, `src/app`, `src/proxy.ts`, 불필요한 Next 타입 파일 정리 여부를 결정하고 제거한다.
  - README/CONTRIBUTING/docs를 최종 아키텍처에 맞게 갱신한다.
  - `pnpm check`, `pnpm test`, `pnpm build`, 추가 QA 스크립트를 반복 실행한다.
- **Convention Notes**: 문서와 실제 실행 경로가 불일치하지 않아야 한다.
- **Verification**: QA 루프 전체 통과.
- **Exit Criteria**: 저장소의 활성 런타임이 Rust/Tauri only 상태가 된다.
- **Status**: completed

## Verification Strategy
전체 구현 완료 후 다음 기준으로 검증한다.
- `pnpm check`
- `pnpm test`
- `pnpm build`
- Tauri 앱 수동 실행 후 보드/태스크/터미널/diff/훅/AI 세션 확인

## Progress Tracking
- Total Todos: 6
- Completed: 6
- Status: Execution complete

## Change Log
- 2026-03-17: Plan created
- 2026-03-17: Todo 1 completed - Rust backend command surface added under src-tauri/src/backend
- 2026-03-17: Todo 2 completed - Tauri entry no longer spawns node boot.js and desktop shell reads local Rust data
- 2026-03-17: Todo 3 completed - active frontend path no longer depends on Next/App Router and uses static desktop shell
- 2026-03-17: Todo 4 completed - worktree/session/git/ssh active flows wired through Rust commands
- 2026-03-17: Todo 5 completed - board/projects/settings active flows served from Rust and SQLite
- 2026-03-17: Todo 6 completed - dormant Next/web code removed and build/run verification finished
