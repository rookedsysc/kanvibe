# KanVibe

AI 에이전트(Claude Code 등) 작업을 실시간으로 모니터링하고 관리하는 Kanban 웹 애플리케이션.

## 기능

- **Kanban 보드**: todo / progress / review / done 4단계 드래그 앤 드롭
- **터미널 연결**: 작업 상세 페이지에서 tmux/zellij 세션을 브라우저로 직접 확인 (xterm.js)
- **Worktree 자동 생성**: branch 기반 todo 생성 시 git worktree + 터미널 세션 자동 생성
- **Hook API**: Claude Code 등 AI 에이전트가 REST API로 작업 상태 자동 업데이트
- **SSH 원격 터미널**: `~/.ssh/config` 읽어서 원격 서버의 tmux/zellij 세션에 접속
- **Docker Compose**: PostgreSQL + Next.js 단일 명령으로 배포

## 빠른 시작

### 환경변수 설정

```bash
cp .env.example .env
# .env 파일을 편집하여 설정 변경
```

| 변수 | 설명 | 기본값 |
|------|------|--------|
| `PORT` | 웹 서버 포트 번호 | `4885` |
| `KANVIBE_USER` | 로그인 사용자명 | `admin` |
| `KANVIBE_PASSWORD` | 로그인 비밀번호 | (필수 변경) |

### Docker Compose 실행

```bash
docker compose up -d
```

브라우저에서 `http://localhost:4885` 접속.

### 로컬 개발

```bash
# PostgreSQL 실행 (docker-compose의 db만)
docker compose up db -d

# 개발 서버 실행
npm install
npm run dev
```

## Hook API (Claude Code 연동)

### 동작 흐름

```
사용자가 prompt 입력 (UserPromptSubmit)
  → kanvibe-prompt-hook.sh 실행
  → POST /api/hooks/status { branchName, projectName, status: "progress" }
  → Kanban 보드에서 작업이 PROGRESS로 이동

AI가 사용자에게 질문 (PreToolUse: AskUserQuestion)
  → kanvibe-question-hook.sh 실행
  → POST /api/hooks/status { branchName, projectName, status: "review" }
  → Kanban 보드에서 작업이 REVIEW로 이동

사용자가 질문에 답변 (PostToolUse: AskUserQuestion)
  → kanvibe-prompt-hook.sh 실행
  → POST /api/hooks/status { branchName, projectName, status: "progress" }
  → Kanban 보드에서 작업이 PROGRESS로 이동

AI 응답 완료 (Stop)
  → kanvibe-stop-hook.sh 실행
  → POST /api/hooks/status { branchName, projectName, status: "review" }
  → Kanban 보드에서 작업이 REVIEW로 이동
```

### API 엔드포인트

#### 작업 생성 — `POST /api/hooks/start`

```bash
curl -X POST http://localhost:4885/api/hooks/start \
  -H "Content-Type: application/json" \
  -d '{
    "title": "feature/user-auth 구현",
    "branchName": "feature/user-auth",
    "agentType": "claude",
    "sessionType": "tmux",
    "projectId": "project-uuid"
  }'
```

#### 상태 변경 — `POST /api/hooks/status`

`branchName` + `projectName`으로 작업을 식별하여 상태를 변경한다.

```bash
curl -X POST http://localhost:4885/api/hooks/status \
  -H "Content-Type: application/json" \
  -d '{
    "branchName": "feature/user-auth",
    "projectName": "kanvibe",
    "status": "review"
  }'
```

| 파라미터 | 타입 | 설명 |
|----------|------|------|
| `branchName` | string | git 브랜치 이름 |
| `projectName` | string | KanVibe에 등록된 프로젝트 이름 |
| `status` | string | `todo`, `progress`, `review`, `done` 중 하나 |

### Claude Code Hooks 설정

#### 자동 설정 (권장)

KanVibe 웹 UI의 **프로젝트 설정 > 디렉토리 스캔**으로 프로젝트를 등록하면, 탐지된 로컬 git 저장소의 `.claude/` 폴더에 hooks가 자동으로 설치된다.

자동 설치되는 파일:
- `.claude/hooks/kanvibe-prompt-hook.sh` — prompt 입력 시 PROGRESS 전환
- `.claude/hooks/kanvibe-question-hook.sh` — AI 질문 시 REVIEW 전환
- `.claude/hooks/kanvibe-stop-hook.sh` — AI 응답 완료 시 REVIEW 전환
- `.claude/settings.json` — hooks 이벤트 등록 (기존 설정이 있으면 병합)

#### 수동 설정

1. 프로젝트 루트에 `.claude/hooks/` 디렉토리를 생성한다.

2. `kanvibe-prompt-hook.sh`를 작성한다:

```bash
#!/bin/bash
KANVIBE_URL="http://localhost:4885"
PROJECT_NAME="your-project-name"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  exit 0
fi

curl -s -X POST "${KANVIBE_URL}/api/hooks/status" \
  -H "Content-Type: application/json" \
  -d "{\"branchName\": \"${BRANCH_NAME}\", \"projectName\": \"${PROJECT_NAME}\", \"status\": \"progress\"}" \
  > /dev/null 2>&1

exit 0
```

3. `kanvibe-stop-hook.sh`를 작성한다 (status를 `"review"`로 변경).

4. 실행 권한을 부여한다:

```bash
chmod +x .claude/hooks/kanvibe-*.sh
```

5. `.claude/settings.json`에 hooks를 등록한다:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/kanvibe-prompt-hook.sh",
            "timeout": 10
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "AskUserQuestion",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/kanvibe-question-hook.sh",
            "timeout": 10
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "AskUserQuestion",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/kanvibe-prompt-hook.sh",
            "timeout": 10
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/kanvibe-stop-hook.sh",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

## 기술 스택

- **Frontend/Backend**: Next.js 16 (App Router, Server Actions, Custom Server)
- **Database**: PostgreSQL 16 + TypeORM
- **Terminal**: xterm.js + WebSocket + node-pty
- **SSH**: ssh2 (Node.js)
- **Styling**: Tailwind CSS
- **Drag & Drop**: @hello-pangea/dnd
- **Container**: Docker Compose

## 프로젝트 구조

```
src/
├── app/
│   ├── actions/       # Server Actions (auth, kanban CRUD)
│   ├── api/hooks/     # Hook REST API (start, status)
│   ├── login/         # 로그인 페이지
│   ├── task/[id]/     # 작업 상세 + 터미널 페이지
│   ├── layout.tsx     # 루트 레이아웃
│   └── page.tsx       # Kanban 보드 메인
├── components/        # React 컴포넌트 (Board, Column, TaskCard, Terminal 등)
├── entities/          # TypeORM 엔티티
└── lib/               # 유틸리티 (auth, database, terminal, worktree, sshConfig)
server.ts              # Custom Server (HTTP + WebSocket)
docker-compose.yml     # 서비스 오케스트레이션
Dockerfile             # 앱 컨테이너
```
