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

### 작업 시작

```bash
curl -X POST http://localhost:4885/api/hooks/start \
  -H "Content-Type: application/json" \
  -d '{
    "title": "feature/user-auth 구현",
    "branchName": "feature/user-auth",
    "agentType": "claude",
    "sessionType": "tmux"
  }'
```

### 작업 업데이트

```bash
curl -X POST http://localhost:4885/api/hooks/update \
  -H "Content-Type: application/json" \
  -d '{
    "id": "task-uuid",
    "status": "review",
    "description": "JWT 인증 구현 완료, 리뷰 대기"
  }'
```

### 작업 완료

```bash
curl -X POST http://localhost:4885/api/hooks/complete \
  -H "Content-Type: application/json" \
  -d '{ "id": "task-uuid" }'
```

### Claude Code hooks 설정 예시

`.claude/hooks.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "command": "curl -s -X POST http://YOUR_KANVIBE_HOST:4885/api/hooks/start -H 'Content-Type: application/json' -d '{\"title\": \"Claude Code 작업\", \"agentType\": \"claude\"}'"
      }
    ]
  }
}
```

## 기술 스택

- **Frontend/Backend**: Next.js 15 (App Router, Server Actions, Custom Server)
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
│   ├── api/hooks/     # Hook REST API (start, update, complete)
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
