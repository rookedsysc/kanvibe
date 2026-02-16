<div align="center">

# KanVibe

**AI 에이전트 작업 관리 칸반 보드**

AI 코딩 에이전트(Claude Code 등)의 작업을 실시간으로 관리하는 웹 기반 터미널 칸반 보드.
브라우저에서 tmux/zellij 세션을 직접 모니터링하며, 드래그 앤 드롭 칸반 보드로 작업 진행 상황을 추적합니다.
[Claude Code Hooks](#claude-code-hooks---자동-상태-추적) 기반 자동 상태 트래킹을 지원하여 수동 업데이트가 필요 없습니다.

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/rookedsysc)

> 커피도 좋지만, 솔직히 Contribution 해주시면 더 좋습니다. :)

[EN](../README.md) | [ZH](./README.zh.md)

</div>

<div align="center">

[![▶ YouTube에서 데모 보기](https://img.youtube.com/vi/8JTrvd3T_Z0/maxresdefault.jpg)](https://www.youtube.com/watch?v=8JTrvd3T_Z0)

**▶ [YouTube에서 데모 보기](https://www.youtube.com/watch?v=8JTrvd3T_Z0)**

<table>
  <tr>
    <td width="53%"><img src="./images/kanvibe1.png" alt="칸반 보드" width="100%"></td>
    <td width="47%"><img src="./images/kanvibe2.png" alt="태스크 상세 & 터미널" width="100%"></td>
  </tr>
</table>

</div>

---

## 사전 요구사항

| 의존성 | 버전 | 설치 |
|--------|------|------|
| [Node.js](https://nodejs.org/) | >= 22 | [다운로드](https://nodejs.org/en/download/) |
| [tmux](https://github.com/tmux/tmux) 또는 [zellij](https://github.com/zellij-org/zellij) | 최신 | `brew install tmux` / `apt install tmux` |
| [Docker](https://www.docker.com/) | 최신 | [다운로드](https://docs.docker.com/get-docker/) |

> Docker는 Docker Compose를 통해 PostgreSQL 데이터베이스를 실행하는 데 사용됩니다.

---

## 빠른 시작

### 1. 환경변수 설정

```bash
cp .env.example .env
```

| 변수 | 설명 | 기본값 |
|------|------|--------|
| `PORT` | 웹 서버 포트 | `4885` |
| `DB_PORT` | PostgreSQL 포트 | `4886` |
| `KANVIBE_USER` | 로그인 사용자명 | `admin` |
| `KANVIBE_PASSWORD` | 로그인 비밀번호 | `changeme` (변경 필수!) |

### 2. 실행

```bash
bash start.sh
```

이 명령어 하나로 의존성 설치, PostgreSQL 시작, 데이터베이스 마이그레이션, 빌드, 서버 실행까지 모두 처리됩니다.

브라우저에서 `http://localhost:4885` 접속.

---

## 사용 흐름

### 1. 프로젝트 등록

프로젝트 설정에서 **fzf 스타일 폴더 검색**으로 로컬 git 저장소를 검색하고 등록합니다. KanVibe가 디렉토리를 스캔하여 기존 worktree 브랜치를 자동 감지합니다.

### 2. 태스크 생성

칸반 보드에서 TODO 태스크를 추가합니다. 브랜치명으로 태스크를 생성하면 KanVibe가 자동으로:
- 해당 브랜치의 **git worktree** 생성
- **tmux window** 또는 **zellij tab**으로 터미널 세션 생성
- 터미널 세션을 태스크에 연결

### 3. 칸반 보드에서 작업

5단계 상태로 태스크를 관리합니다: **TODO** → **PROGRESS** → **PENDING** → **REVIEW** → **DONE**

드래그 앤 드롭으로 상태를 변경하거나, [Claude Code Hooks](#claude-code-hooks---자동-상태-추적)를 통해 자동으로 전환됩니다. 태스크가 **DONE**으로 이동하면 브랜치, worktree, 터미널 세션이 **자동으로 삭제**됩니다.

### 4. Pane 레이아웃 선택

각 태스크의 터미널 페이지에서 다양한 pane 레이아웃을 지원합니다:

| 레이아웃 | 설명 |
|----------|------|
| **Single** | 전체 화면 단일 pane |
| **Horizontal 2** | 좌우 2분할 |
| **Vertical 2** | 상하 2분할 |
| **Left + Right TB** | 왼쪽 + 오른쪽 상하 분할 |
| **Left TB + Right** | 왼쪽 상하 분할 + 오른쪽 |
| **Quad** | 4등분 |

각 pane에 커스텀 명령어를 설정할 수 있습니다 (예: `vim`, `htop`, `lazygit`, 테스트 러너 등). 레이아웃은 전역 또는 프로젝트별로 설정 가능합니다.

---

## 기능

### 칸반 보드
- 5단계 상태 관리 (TODO / PROGRESS / PENDING / REVIEW / DONE)
- 사용자 정의 태스크 정렬
- 다중 프로젝트 필터링
- Done 컬럼 페이지네이션
- WebSocket 기반 실시간 업데이트

### Git Worktree 연동
- 브랜치 기반 태스크 생성 시 git worktree 자동 생성
- Worktree 스캔: 기존 브랜치를 TODO 태스크로 자동 등록
- DONE 상태 전환 시 브랜치 + worktree + 세션 자동 정리

### 터미널 세션 (tmux / zellij)
- **tmux**와 **zellij** 모두 터미널 멀티플렉서로 지원
- xterm.js + WebSocket 기반 브라우저 터미널
- SSH 원격 터미널 지원 (`~/.ssh/config` 읽기)
- Nerd Font 렌더링 지원

### Claude Code Hooks - 자동 상태 추적
KanVibe는 **Claude Code Hooks**와 연동하여 태스크 상태를 자동 추적합니다. 태스크는 5가지 상태로 관리됩니다:

| 상태 | 설명 |
|------|------|
| **TODO** | 생성된 태스크의 초기 상태 |
| **PROGRESS** | AI가 작업 중인 상태 |
| **PENDING** | AI가 사용자에게 재질문하여 답변 대기 중인 상태 |
| **REVIEW** | AI 작업이 완료되어 리뷰 대기 중인 상태 |
| **DONE** | 작업 완료 — 브랜치, worktree, 터미널 세션이 **자동 삭제**됩니다 |

```
사용자가 프롬프트 전송   → 태스크가 PROGRESS로 이동
AI가 재질문 (AskUser)   → 태스크가 PENDING으로 자동 전환
사용자가 답변           → 태스크가 PROGRESS로 복귀
AI 응답 완료            → 태스크가 REVIEW로 이동
태스크를 DONE으로 이동   → 브랜치 + worktree + 터미널 세션 자동 삭제
```

프로젝트를 KanVibe 디렉토리 스캔으로 등록하면 Hook이 **자동 설치**됩니다. Hook 스크립트는 프로젝트의 `.claude/hooks/` 디렉토리에 배치됩니다.

#### Hook API 엔드포인트

| 엔드포인트 | 메서드 | 설명 |
|-----------|--------|------|
| `/api/hooks/start` | POST | 새 태스크 생성 |
| `/api/hooks/status` | POST | `branchName` + `projectName`으로 태스크 상태 변경 |

### Pane 레이아웃 에디터
- 6가지 레이아웃 프리셋 (Single, Horizontal 2, Vertical 2, Left+Right TB, Left TB+Right, Quad)
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
| Database | PostgreSQL 16 + TypeORM |
| Styling | Tailwind CSS v4 |
| Terminal | xterm.js + WebSocket + node-pty |
| SSH | ssh2 (Node.js) |
| Drag & Drop | @hello-pangea/dnd |
| i18n | next-intl |
| Container | Docker Compose |

---

## 라이센스

이 프로젝트는 **AGPL-3.0** 라이센스를 따릅니다. 오픈소스 목적으로 자유롭게 사용, 수정, 확장할 수 있습니다. 상업적 SaaS 배포는 이 라이센스에서 허용되지 않습니다. 자세한 내용은 [LICENSE](../LICENSE)를 참조하세요.

---

## 기여하기

[CONTRIBUTING.ko.md](./CONTRIBUTING.ko.md)를 참조하세요.

---

## Inspired By

- [workmux](https://github.com/raine/workmux) — tmux workspace manager
- [vibe-kanban](https://github.com/BloopAI/vibe-kanban) — AI-powered Kanban board
