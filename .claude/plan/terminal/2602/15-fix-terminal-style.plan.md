# tmux 웹 터미널 렌더링 문제 수정

## Business Goal

웹 터미널에서 tmux 사용 시 발생하는 시각적 문제를 해결하여 사용자 경험을 개선한다.
1. pane 구분선이 삐뚤삐뚤하게 렌더링되는 문제 수정
2. tmux가 터미널 영역을 꽉 채우지 않아 하단에 빈 공간이 생기는 문제 수정

## Scope
- **In Scope**: xterm.js Unicode 폭 처리 개선, tmux window-size 옵션 설정
- **Out of Scope**: tmux 테마/스타일 변경, 터미널 폰트 변경, 새 기능 추가

## Codebase Analysis Summary

xterm.js v6 + FitAddon + WebLinksAddon 구성. `GeistMono Nerd Font Mono` 웹폰트 사용.
서버 측에서 node-pty로 `tmux attach-session -t sessionName:windowName` 실행.
Unicode11Addon은 이전 커밋(c274a2e)에서 제거됨.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Terminal.tsx` | xterm.js 터미널 컴포넌트 | Modify - Unicode graphemes addon 추가 |
| `src/lib/terminal.ts` | PTY 생성 및 tmux attach 로직 | Modify - tmux window-size 옵션 설정 |
| `package.json` | 의존성 관리 | Modify - addon 패키지 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어로 작성 |
| Dynamic import | Terminal.tsx 기존 패턴 | xterm 애드온은 dynamic import |
| async/await | terminal.ts 기존 패턴 | child_process는 promisify 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Unicode 폭 처리 | `@xterm/addon-unicode-graphemes` | xterm v6 공식 권장, Unicode 15+ 지원 | `@xterm/addon-unicode11` (이전에 제거됨) |
| tmux 크기 조절 | `window-size latest` 글로벌 옵션 | 가장 최근 클라이언트 크기 사용, 다른 클라이언트 미영향 | `aggressive-resize on` (window별 동작), `-d` 플래그 (다른 클라이언트 강제 해제) |

## Implementation Todos

### Todo 1: @xterm/addon-unicode-graphemes 패키지 설치
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Unicode graphemes 애드온 패키지 설치
- **Work**:
  - `npm install @xterm/addon-unicode-graphemes` 실행
- **Convention Notes**: package.json의 dependencies에 추가됨을 확인
- **Verification**: `npm ls @xterm/addon-unicode-graphemes`로 설치 확인
- **Exit Criteria**: 패키지가 node_modules에 설치되고 package.json에 기록됨
- **Status**: pending

### Todo 2: Terminal.tsx에 Unicode graphemes 애드온 적용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: xterm.js에 Unicode graphemes 처리를 활성화하여 문자 폭 계산을 tmux와 일치시킴
- **Work**:
  - `src/components/Terminal.tsx`의 `connect` 함수 내에서:
  - 기존 `FitAddon`, `WebLinksAddon` import 이후에 `UnicodeGraphemesAddon` dynamic import 추가
  - `term.loadAddon(new UnicodeGraphemesAddon())` 호출
  - `term.unicode.activeVersion = "15"` 설정으로 Unicode 15 폭 테이블 활성화
  - 위 코드는 `term.open()` 호출 전에 배치
- **Convention Notes**: 기존 dynamic import 패턴 (`const { X } = await import(...)`) 따름, 한국어 주석
- **Verification**: 빌드 성공 (`npm run build`), 웹 터미널에서 tmux pane 구분선이 일직선으로 표시됨
- **Exit Criteria**: Unicode graphemes 애드온이 로드되고 활성화됨
- **Status**: pending

### Todo 3: tmux window-size 옵션 설정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: tmux가 가장 최근 활성 클라이언트의 크기를 사용하도록 설정하여 웹 터미널이 영역을 꽉 채우게 함
- **Work**:
  - `src/lib/terminal.ts`의 `attachLocalSession` 함수 내에서:
  - tmux `attach-session` 실행 전에 `execSync`로 `tmux set-option -g window-size latest` 실행
  - 에러 발생 시 무시 (tmux 구버전 호환)
  - `child_process`의 `execSync` import 추가
- **Convention Notes**: 기존 코드에서 `child_process` dynamic import 패턴 사용 중, try-catch로 에러 무시
- **Verification**: 웹 터미널에서 tmux가 전체 영역을 채움, 빌드 성공
- **Exit Criteria**: tmux attach 전에 window-size latest가 설정됨
- **Status**: pending

## Verification Strategy
- `npm run build`로 빌드 에러 없음 확인
- 웹 터미널에서 tmux 접속 후:
  1. pane 구분선이 일직선으로 표시되는지 확인
  2. tmux가 터미널 영역을 꽉 채우는지 확인 (하단 빈 공간 없음)
- 로컬 터미널과 웹 터미널 동시 접속 시에도 웹 터미널이 정상 크기로 표시되는지 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-02-15: Plan created
- 2026-02-15: All todos completed, build verified
