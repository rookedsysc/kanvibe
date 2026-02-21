# AI Hooks Git Exclude 설정

## Business Goal

worktree에 자동 생성되는 AI 코딩 도구(Claude, Gemini, Codex, OpenCode)의 hooks 파일들이 git tracking에 포함되지 않도록 `.git/info/excluded`에 패턴을 추가한다. `.gitignore`는 커밋되므로 로컬 전용 제외에 적합한 `info/excluded`를 사용한다.

## Scope

- **In Scope**: `gitExclude.ts` 유틸리티 생성, 4개 hooks setup 함수에서 호출 추가
- **Out of Scope**: 기존 테스트 수정 (유틸은 best-effort로 실패해도 hooks 설치에 영향 없음)

## Codebase Analysis Summary

4개 AI 도구의 hooks setup 함수가 각각 worktree 경로에 설정 파일을 생성한다. 현재 이 파일들은 git exclude 처리가 없어 `git status`에 untracked로 노출된다.

### Relevant Files

| File | Role | Action |
|------|------|--------|
| `src/lib/gitExclude.ts` | git exclude 패턴 추가 유틸리티 | Create |
| `src/lib/claudeHooksSetup.ts` | Claude hooks 설치 | Modify |
| `src/lib/geminiHooksSetup.ts` | Gemini hooks 설치 | Modify |
| `src/lib/codexHooksSetup.ts` | Codex hooks 설치 | Modify |
| `src/lib/openCodeHooksSetup.ts` | OpenCode hooks 설치 | Modify |

### 제외 대상 파일/디렉토리

| Tool | 생성 파일 | Exclude 패턴 |
|------|----------|-------------|
| Claude | `.claude/hooks/*.sh`, `.claude/settings.json` | `.claude/hooks/`, `.claude/settings.json` |
| Gemini | `.gemini/hooks/*.sh`, `.gemini/settings.json` | `.gemini/hooks/`, `.gemini/settings.json` |
| Codex | `.codex/hooks/*.sh`, `.codex/config.toml` | `.codex/hooks/`, `.codex/config.toml` |
| OpenCode | `.opencode/plugins/kanvibe-plugin.ts` | `.opencode/plugins/` |

### Conventions to Follow

| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | JSDoc 주석은 한국어로 |
| KISS | CODE_PRINCIPLES.md | 함수 하나가 한 가지 일 |
| 에러 처리 | 기존 hooks setup 패턴 | try-catch로 감싸되 실패해도 메인 로직 중단 안 함 |

## Architecture Decisions

| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| exclude 위치 | `.git/info/excluded` | 로컬 전용, 커밋 불필요 | `.gitignore` (커밋됨), `~/.config/git/ignore` (전역) |
| git dir 탐색 | `git rev-parse --git-dir` | worktree에서도 정확한 git dir 반환 | path 직접 조합 (worktree 구조 가정 필요) |
| 호출 위치 | 각 setup 함수 내부 | 설치와 exclude가 항상 쌍으로 실행됨 | 외부 호출자에서 별도 호출 (누락 가능) |
| 패턴 범위 | 4개 도구 전부 한번에 추가 | 어떤 도구든 먼저 설치되면 전체 exclude 보장 | 각 도구별 자기 패턴만 (불완전) |

## Implementation Todos

### Todo 1: gitExclude 유틸리티 생성

- **Priority**: 1
- **Dependencies**: none
- **Goal**: `addAiToolPatternsToGitExclude(repoPath)` 함수를 생성하여 `.git/info/excluded`에 AI 도구 패턴을 멱등적으로 추가한다
- **Work**:
  - `src/lib/gitExclude.ts` 파일 생성
  - `addAiToolPatternsToGitExclude(repoPath: string): Promise<void>` 함수 구현
  - `execAsync("git -C <repoPath> rev-parse --git-dir")`로 git 디렉토리 경로 획득
  - `<gitDir>/info/excluded` 파일을 읽고 (없으면 빈 문자열)
  - `# KanVibe AI hooks (auto-generated)` 마커 블록이 없으면 패턴 블록 전체를 append
  - 이미 마커가 있으면 skip (멱등성)
  - `info/` 디렉토리가 없으면 `mkdir -p`로 생성
- **Convention Notes**: JSDoc 한국어, export async function
- **Verification**: 유틸리티 파일 생성 확인, TypeScript 컴파일 오류 없음
- **Exit Criteria**: `src/lib/gitExclude.ts`가 존재하고 export 함수가 올바르게 정의됨
- **Status**: pending

### Todo 2: 4개 hooks setup 함수에 gitExclude 호출 추가

- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 각 setup 함수가 hooks 파일 생성 후 git exclude 패턴을 자동 추가하도록 한다
- **Work**:
  - `src/lib/claudeHooksSetup.ts`의 `setupClaudeHooks` 함수 마지막에 `await addAiToolPatternsToGitExclude(repoPath)` 호출 추가 (try-catch로 감싸서 실패 시 console.error만)
  - `src/lib/geminiHooksSetup.ts`의 `setupGeminiHooks` 함수 마지막에 동일하게 추가
  - `src/lib/codexHooksSetup.ts`의 `setupCodexHooks` 함수 마지막에 동일하게 추가
  - `src/lib/openCodeHooksSetup.ts`의 `setupOpenCodeHooks` 함수 마지막에 동일하게 추가
  - 각 파일 상단에 `import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude"` 추가
- **Convention Notes**: 에러가 발생해도 hooks 설치 자체는 성공한 상태이므로 throw하지 않음
- **Verification**: TypeScript 컴파일 오류 없음, 기존 테스트 통과
- **Exit Criteria**: 4개 setup 함수 모두에 gitExclude 호출이 추가됨
- **Status**: pending

### Todo 3: 빌드 및 테스트 검증

- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 변경 사항이 기존 코드를 깨뜨리지 않음을 확인한다
- **Work**:
  - `NODE_ENV=test pnpm test` 실행하여 기존 테스트 통과 확인
  - TypeScript 컴파일 검증
- **Verification**: 테스트 통과, 빌드 성공
- **Exit Criteria**: 모든 테스트 green, 컴파일 에러 없음
- **Status**: pending

## Verification Strategy

- TypeScript 컴파일: `pnpm build` 또는 `npx tsc --noEmit`
- 기존 테스트: `NODE_ENV=test pnpm test`
- 통합 확인: setup 함수 호출 시 `.git/info/excluded`에 패턴이 추가되는지 수동 확인

## Progress Tracking

- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log

- 2026-02-21: Plan created
