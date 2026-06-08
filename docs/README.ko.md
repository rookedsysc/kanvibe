<div align="center">

# KanVibe

**AI 코딩 에이전트를 위한 키보드 중심 칸반 워크스페이스**

KanVibe는 AI 코딩 작업이 여러 터미널 탭에 흩어지지 않도록 도와줍니다. 브랜치 기반 태스크를 실시간 칸반 보드에서 추적하고, 각 태스크의 tmux/zellij 세션을 브라우저나 데스크톱 앱에서 열며, Claude Code, Gemini CLI, Codex CLI, OpenCode hooks가 워크플로우에 맞춰 태스크 상태를 자동으로 이동하게 할 수 있습니다.

터미널 포커스를 잃지 않고 프로젝트 필터, 태스크 검색, 알림, 태스크 상세 패널, 자주 쓰는 태스크 액션을 단축키로 실행하세요.

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/rookedsysc)

> 커피도 좋지만, 솔직히 Contribution 해주시면 더 좋습니다. :)

[EN](../README.md) | [ZH](./README.zh.md)

</div>

<div align="center">

<table>
  <tr>
    <td width="50%">
      <img src="./images/readme/kanvibe-main.png" alt="KanVibe 칸반 보드" width="100%">
      <br>
      <strong>메인 칸반 보드</strong>
    </td>
    <td width="50%">
      <img src="./images/readme/kanvibe-detail.png" alt="태스크 상세 터미널 워크스페이스" width="100%">
      <br>
      <strong>태스크 상세 워크스페이스</strong>
    </td>
  </tr>
</table>

<a href="https://www.youtube.com/watch?v=8JTrvd3T_Z0">
  <img src="https://img.youtube.com/vi/8JTrvd3T_Z0/maxresdefault.jpg" alt="KanVibe 데모 영상 썸네일" width="100%">
</a>

<strong><a href="https://www.youtube.com/watch?v=8JTrvd3T_Z0">YouTube에서 데모 보기</a></strong>

</div>

---

## 주요 워크플로우

### 1. 빠른 태스크 검색

어디서든 태스크 검색을 열고 프로젝트나 브랜치명으로 필터링한 뒤, 보드로 돌아가지 않고 바로 태스크 워크스페이스로 이동합니다.

<img src="./images/readme/kanvibe-search-shortcut.png" alt="빠른 태스크 검색 단축키" width="100%">

### 2. 태스크 상세 단축키

태스크 상세 페이지의 번호형 dock 단축키로, 키 입력이 임베디드 터미널에 도달하기 전에 태스크 메타데이터, hook 상태, AI 채팅, PR 액션을 열 수 있습니다.

<img src="./images/readme/kanvibe-detail-shortcut.png" alt="태스크 상세 단축키 패널" width="100%">

### 3. 프로젝트 필터

관심 있는 활성 프로젝트만 보이도록 보드를 좁히고, 키보드 네비게이션으로 저장소 사이를 빠르게 전환합니다.

<img src="./images/readme/kanvibe-project-search-shortcut.png" alt="프로젝트 필터 단축키" width="100%">

### 4. 알림

알림 패널을 열어 AI 에이전트 상태 변경, 백그라운드 동기화 결과, 태스크 이벤트를 확인하고 관련 태스크로 이동합니다.

<img src="./images/readme/kanvibe-notification-shortcut.png" alt="알림 단축키 패널" width="100%">

### 5. 빠른 태스크 액션

검색 결과에서 하이라이트된 태스크를 기준으로 후속 브랜치 TODO를 바로 만들고, 다음 작업에 필요한 프로젝트와 브랜치 컨텍스트를 유지합니다.

<img src="./images/readme/kanvibe-quick-action-shortcut.png" alt="빠른 태스크 액션 단축키" width="100%">

### 6. Vim 스타일 보드 조작

**설정 → 키보드**에서 Vim 스타일 보드 조작을 켠 뒤 `h/j/k/l`로 태스크 카드를 이동하고, `/`로 보이는 태스크 텍스트를 검색하며, `n`으로 새 태스크 모달을 열고, `:move progress`로 상태를 이동하고, 포커스된 태스크를 `dd`로 삭제합니다.

<video src="./images/readme/kanvibe-vim-controls.mp4" poster="./images/readme/kanvibe-vim-controls.png" controls muted playsinline width="100%"></video>

[Vim 조작 데모 영상 열기](./images/readme/kanvibe-vim-controls.mp4)

---

## 사전 요구사항

| 의존성 | 버전 | 필수 | 설치 |
|--------|------|------|------|
| [git](https://git-scm.com/) | 최신 | Yes | `brew install git` |
| [tmux](https://github.com/tmux/tmux) | 최신 | Yes | `brew install tmux` |
| [gh](https://cli.github.com/) | 최신 | Yes | `brew install gh` (`gh auth login` 필요) |
| [zellij](https://github.com/zellij-org/zellij) | 최신 | No | `brew install zellij` |

---

## 빠른 시작

### Homebrew로 설치

KanVibe가 공식 Homebrew Cask 저장소에 등록되기 전까지는 KanVibe Homebrew tap에서 설치합니다:

```bash
brew install --cask rookedsysc/kanvibe/kanvibe
open -a KanVibe
```

공식 Homebrew Cask 등록 후에는 아래 한 줄로 설치합니다:

```bash
brew install --cask kanvibe
```

### 업데이트 또는 제거

```bash
brew update
brew upgrade --cask kanvibe
```

```bash
brew uninstall --cask kanvibe
brew untap rookedsysc/kanvibe
```

---

## 사용 흐름

### 1. 프로젝트 등록

메인 보드 상단에서 프로젝트 필터와 **+ 새 작업** 사이의 **돋보기 디렉터리 스캔 버튼**을 엽니다. Dialog에서 fzf 스타일 폴더 검색으로 로컬/원격 git 저장소를 선택하면 KanVibe가 디렉터리를 스캔하여 기존 worktree 브랜치를 자동 감지합니다.

프로젝트 관리를 중단하려면 디렉터리 스캔 dialog에서 삭제하세요. 이 동작은 내장 SQLite 데이터베이스에서 해당 프로젝트와 KanVibe 태스크만 삭제하며, 디스크의 git 브랜치, worktree, 파일은 보존합니다.

### 2. 태스크 생성

칸반 보드에서 TODO 태스크를 추가합니다. 브랜치명으로 태스크를 생성하면 KanVibe가 자동으로:
- 해당 브랜치의 **git worktree**를 생성합니다
- 세션용 **tmux window** 또는 **zellij tab**을 생성합니다
- 터미널 세션을 태스크에 연결합니다

### 3. 칸반 보드에서 작업

태스크는 5단계 상태로 관리됩니다: **TODO** → **PROGRESS** → **PENDING** → **REVIEW** → **DONE**

드래그 앤 드롭으로 상태를 변경하거나 [AI 에이전트 Hooks](#ai-에이전트-hooks---자동-상태-추적)가 자동으로 상태를 전환하게 할 수 있습니다. 태스크가 **DONE**으로 이동하면 브랜치, worktree, 터미널 세션이 **자동으로 삭제**됩니다.

### 4. Pane 레이아웃 선택

각 태스크의 터미널 페이지에서 다양한 pane 레이아웃을 지원합니다:

| 레이아웃 | 설명 |
|----------|------|
| **Single** | 전체 화면 단일 pane |
| **Horizontal 2** | 좌우 2분할 |
| **Vertical 2** | 상하 2분할 |
| **Left + Right TB** | 왼쪽 pane + 오른쪽 상하 분할 |
| **Left TB + Right** | 왼쪽 상하 분할 + 오른쪽 pane |
| **Quad** | 4등분 |

각 pane에 커스텀 명령어를 설정할 수 있습니다(예: `vim`, `htop`, `lazygit`, 테스트 러너 등). 레이아웃은 설정 다이얼로그에서 전역 또는 프로젝트별로 설정할 수 있습니다.

---

## 기능

### 실시간 칸반 보드
- 5단계 태스크 관리(TODO / PROGRESS / PENDING / REVIEW / DONE)
- 프로젝트 색상, 우선순위 표시, PR 배지, 세션 라벨을 포함한 드래그 앤 드롭 태스크 정렬
- 보이는 프로젝트와 태스크 텍스트를 키보드로 검색할 수 있는 다중 프로젝트 필터링
- 장기 프로젝트를 위한 Done 컬럼 페이지네이션
- 브라우저와 데스크톱 창 전체에 WebSocket 기반 실시간 업데이트

### 브랜치 기반 태스크 워크스페이스
- git worktree와 터미널 세션을 자동으로 준비하는 브랜치 TODO 생성
- 보드 toolbar의 돋보기 스캔 dialog로 저장소를 등록하고 기존 worktree 브랜치를 TODO 태스크로 감지
- 각 태스크를 전용 터미널 워크스페이스로 열고, 사이드 dock에서 태스크 메타데이터, hook 컨트롤, 채팅, PR 액션 사용
- 태스크를 DONE으로 이동하여 브랜치, worktree, 터미널 세션 자동 정리
- 기존 git 브랜치, worktree, 디스크 파일을 건드리지 않고 디렉터리 스캔 dialog에서 프로젝트 삭제

### 터미널 세션 (tmux / zellij)
- **tmux**와 **zellij** 모두 터미널 멀티플렉서로 지원
- xterm.js와 WebSocket 기반 브라우저 터미널 스트리밍
- `~/.ssh/config`를 읽는 SSH 원격 터미널 지원
- 비대화형 원격 SSH 명령은 `~/.kanvibe` 아래의 앱 전용 ControlMaster 소켓 풀을 재사용하며 host별 동시성을 사용 가능한 CPU 코어 수의 4배까지 제한합니다
- 원격 터미널 attach는 SSH에서 tmux/zellij를 직접 실행하며, 로컬 `DISPLAY`, 원격 `X11Forwarding`, `xauth`가 준비된 경우에만 trusted X11 forwarding(`ssh -Y`)을 요청합니다
- Nerd Font 렌더링 지원

### 키보드 중심 조작
- 어디서든 브랜치명이나 프로젝트명으로 빠른 태스크 검색 열기
- 보드를 떠나지 않고 프로젝트 필터링, 알림 확인, 태스크 액션 실행
- 키 입력이 터미널에 도달하기 전에 번호형 상세 단축키로 태스크 정보, 상태/hooks, AI 채팅, PR 및 다른 dock 패널 전환
- 설정된 단축키로 빠른 태스크 검색에서 브랜치 TODO 바로 생성

### 키보드 단축키

| 단축키 | 범위 | 동작 |
|--------|------|------|
| `Cmd/Ctrl+F` 또는 Vim 스타일 조작이 켜진 상태의 `/` | 보드 | 현재 보이는 프로젝트/태스크 텍스트를 보드 안에서 검색. `Enter`로 다음, `Shift+Enter`로 이전 결과 이동 |
| `h / j / k / l` 또는 `← / ↓ / ↑ / →` | 보드 태스크 카드 | 보이는 태스크 카드 포커스를 좌/하/상/우로 이동. 포커스된 태스크가 없으면 첫 번째 보이는 태스크로 진입 |
| `n` | 보드 | 새 태스크 모달 즉시 열기 |
| `:move todo\|progress\|pending\|review\|done` | 포커스된 보드 태스크 카드 | 드래그 없이 포커스된 태스크를 대상 상태로 이동. 명령 입력창에서 `Tab`을 누르면 유일한 상태 prefix를 자동 완성 |
| `dd` | 포커스된 보드 태스크 카드 | 확인 후 포커스된 태스크 삭제 |
| `Cmd/Ctrl+Shift+O` | 전역 | 브랜치명/프로젝트명 기준 빠른 태스크 검색 열기(기본값, 변경 가능) |
| `Cmd/Ctrl+Shift+P` | 보드 | 프로젝트 필터 드롭다운 열기 |
| `Cmd/Ctrl+Shift+I` | 보드 | 알림 드롭다운 열기 |
| `Cmd+[` / `Cmd+]` (macOS), `Alt+[` / `Alt+]` (Linux) | 전역 | 앱 히스토리 뒤로/앞으로 이동. 더 뒤로 갈 곳이 없으면 보드 홈으로 이동 |
| `Cmd+1/2/3` (macOS), `Alt+1/2/3` (Linux) | 태스크 상세 | 번호형 상세 dock 항목인 정보, 상태/hooks, AI 채팅을 활성화. 이 단축키는 터미널 입력보다 먼저 가로챕니다 |
| `Cmd+4` (macOS), `Alt+4` (Linux) | 태스크 상세 | PR이 있는 태스크에서는 브라우저로 태스크 PR을 열고, PR이 없으면 보이는 네 번째 dock 항목에 할당됩니다 |
| `Cmd/Ctrl+N` | 빠른 태스크 검색 | 현재 하이라이트된 태스크 기준으로 새 브랜치 TODO 만들기 |
| `↑ / ↓ / Enter / Shift+Enter / Esc` | 빠른 태스크 검색 | 선택 이동, 태스크 열기, 태스크 새 창 열기, 다이얼로그 닫기 |
| `↑ / ↓ / Enter / Esc` | 프로젝트 필터 드롭다운 | 선택 이동, 프로젝트 필터 토글, 드롭다운 닫기 |
| `↑ / ↓ / Enter / Esc` | 알림 드롭다운 | 선택 이동, 알림 대상 열기, 드롭다운 닫기 |

태스크 상세 dock 번호는 보드로 돌아가기 버튼을 제외하고 보이는 dock 항목 순서를 따릅니다. 태스크에 PR URL이 있으면 PR이 4번 슬롯을 차지하고 이후 dock 항목은 5번 이후로 밀립니다. PR이 없으면 다음 dock 항목이 4번 슬롯을 사용합니다.

Vim 스타일 보드 조작(`h/j/k/l`, `/`, `n`, `dd`, `:move ...`)은 **설정 → 키보드**에서 켜고 끌 수 있습니다. 꺼도 방향키 태스크 이동과 `Cmd/Ctrl+F` 페이지 검색은 계속 사용할 수 있습니다.

### AI 에이전트 Hooks - 자동 상태 추적
KanVibe는 **Claude Code Hooks**, **Gemini CLI Hooks**, **Codex CLI**, **OpenCode**와 연동하여 태스크 상태를 자동 추적합니다. 태스크는 5가지 상태로 관리됩니다:

| 상태 | 설명 |
|------|------|
| **TODO** | 태스크 생성 시 초기 상태 |
| **PROGRESS** | AI가 태스크를 적극적으로 작업 중인 상태 |
| **PENDING** | AI가 후속 질문을 하여 사용자 응답을 기다리는 상태(Claude Code만 지원) |
| **REVIEW** | AI 작업이 완료되어 리뷰 대기 중인 상태 |
| **DONE** | 작업 완료 — 브랜치, worktree, 터미널 세션이 **자동 삭제**됩니다 |

#### Claude Code
```
사용자가 프롬프트 전송     → PROGRESS
AI가 재질문 (AskUser)     → PENDING
사용자가 답변             → PROGRESS
AI 응답 완료              → REVIEW
```

#### Gemini CLI
```
BeforeAgent (사용자 프롬프트) → PROGRESS
AfterAgent (에이전트 완료)   → REVIEW
```

> Gemini CLI에는 Claude Code의 `AskUserQuestion`에 대응하는 이벤트가 없어 PENDING 상태는 지원되지 않습니다.

#### Codex CLI
```
UserPromptSubmit              → PROGRESS
PermissionRequest (Bash 전용) → PENDING
PreToolUse (Bash 전용)        → PROGRESS
Stop                          → REVIEW
```

KanVibe는 이제 Codex 최신 lifecycle hooks 방식인 `.codex/hooks.json`과 `.codex/config.toml`의 `[features].hooks = true` 조합을 사용합니다:

- https://developers.openai.com/codex/hooks
- https://developers.openai.com/codex/config-reference

Codex는 신뢰된 프로젝트/worktree 경로에서만 project-local `.codex/` hooks를 로드합니다. 태스크가 생성된 worktree에서 실행된다면 local hook 파일이 실행되기 전에 해당 worktree를 Codex에서 신뢰 상태로 설정해야 합니다.

> 현재 Codex의 `PermissionRequest`와 `PreToolUse` 매처는 Bash 범위에 한정되므로, `PENDING`은 모든 대화형 재질문이 아니라 승인 대기 상태를 의미합니다.

#### OpenCode
```
사용자 메시지 전송 (message.updated, role=user) → PROGRESS
AI 질문 대기 (question.asked)                   → PENDING
사용자 질문 답변 (question.replied)             → PROGRESS
세션 대기 (session.idle)                        → REVIEW
```

OpenCode는 셸 스크립트 hooks 대신 자체 [플러그인 시스템](https://opencode.ai/docs/plugins/)을 사용합니다. KanVibe는 `.opencode/plugins/kanvibe-plugin.ts`에 TypeScript 플러그인을 생성하여 `@opencode-ai/plugin` SDK를 통해 OpenCode의 네이티브 이벤트 hooks(`message.updated`, `question.asked`, `question.replied`, `session.idle`)를 구독합니다. 외부 셸 명령을 실행하지 않고 프로세스 내에서 상태 업데이트를 처리합니다.

모든 에이전트 Hook은 KanVibe 디렉터리 스캔으로 프로젝트를 등록하거나 worktree와 함께 태스크를 생성하면 **자동 설치**됩니다. 태스크 상세 페이지에서 개별 설치도 가능합니다.

| 에이전트 | Hook 디렉터리 | 설정 파일 |
|---------|--------------|----------|
| Claude Code | `.claude/hooks/` | `.claude/settings.json` |
| Gemini CLI | `.gemini/hooks/` | `.gemini/settings.json` |
| Codex CLI | `.codex/hooks/` | `.codex/config.toml`, `.codex/hooks.json` |
| OpenCode | `.opencode/plugins/` | 플러그인 자동 탐색 |

#### 브라우저 알림

AI 에이전트 Hooks를 통한 태스크 상태 변경이 **브라우저 알림**으로 전달됩니다. 프로젝트명, 브랜치명, 변경된 상태를 표시하며, **알림을 클릭하면 해당 태스크 상세 페이지로 이동**합니다.

- **실시간 알림** — 태스크 상태 변경 시 즉시 알림 수신
- **백그라운드 모드** — KanVibe가 포커스되지 않아도 알림 수신
- **스마트 네비게이션** — 알림 클릭 → 태스크 상세 페이지(현재 언어 유지)
- **커스터마이징 가능** — 프로젝트별 알림 활성화/비활성화 및 상태별 필터링 지원(PROGRESS, PENDING, REVIEW, DONE)

설정: 최초 접속 시 브라우저 알림 권한을 허용해 주세요. **프로젝트 설정** → **알림**에서 상태별 필터를 조정할 수 있습니다.

#### Hook API 엔드포인트

| 엔드포인트 | 메서드 | 설명 |
|-----------|--------|------|
| `/api/hooks/start` | POST | 새 태스크 생성 |
| `/api/hooks/status` | POST | `branchName` + `projectName`으로 태스크 상태를 변경하며, 대상을 못 찾으면 상태는 바꾸지 않고 브라우저 알림만 보낸 뒤 `404`를 반환 |

### GitHub 스타일 Diff 뷰

브라우저에서 GitHub 스타일의 diff 뷰어로 코드 변경사항을 바로 확인할 수 있습니다. 태스크 상세 페이지의 **Diff** 뱃지를 클릭하면 base 브랜치 대비 변경된 모든 파일을 확인할 수 있습니다.

- 변경 파일 수가 표시되는 파일 트리 사이드바
- Monaco Editor 기반 인라인 diff 뷰어
- 브라우저에서 바로 수정 가능한 에딧 모드
- 체크박스로 확인한 파일 추적

### Pane 레이아웃 에디터
- 6가지 레이아웃 프리셋(Single, Horizontal 2, Vertical 2, Left+Right TB, Left TB+Right, Quad)
- pane별 커스텀 명령어 설정
- 전역 및 프로젝트별 레이아웃 설정

### 국제화 (i18n)
- 지원 언어: 한국어(ko), 영어(en), 중국어(zh)
- next-intl 기반

---

## 기술 스택

| 카테고리 | 기술 |
|----------|------|
| Frontend/Backend | Next.js 16 (App Router) + React 19 + TypeScript |
| Database | SQLite + TypeORM + better-sqlite3 |
| Styling | Tailwind CSS v4 |
| Terminal | xterm.js + WebSocket + node-pty |
| SSH | system ssh binary |
| Drag & Drop | @hello-pangea/dnd |
| i18n | next-intl |
| Desktop Packaging | Electron + Electron Builder |

---

## 라이선스

이 프로젝트는 **AGPL-3.0** 라이선스를 따릅니다. 오픈소스 목적으로 자유롭게 사용, 수정, 확장할 수 있습니다. 상업적 SaaS 배포는 이 라이선스에서 허용되지 않습니다. 자세한 내용은 [LICENSE](../LICENSE)를 참조하세요.

---

## 기여하기

[CONTRIBUTING.ko.md](./CONTRIBUTING.ko.md)를 참조하세요.

---

## Inspired By

- [workmux](https://github.com/raine/workmux) — tmux workspace manager
- [vibe-kanban](https://github.com/BloopAI/vibe-kanban) — AI-powered Kanban board
