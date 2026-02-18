# KanVibe CLI Script (kanvibe start/stop)

## Business Goal
기존 start.sh를 `bash kanvibe start` / `bash kanvibe stop` 형태의 통합 CLI 스크립트로 교체한다. 의존성 자동 체크 및 설치 프롬프트를 제공하고, 시스템 로케일 기반 i18n(ko/en/zh)을 적용하여 사용자 경험을 개선한다.

## Scope
- **In Scope**:
  - `kanvibe` bash 스크립트 생성 (start/stop 서브커맨드)
  - 의존성 체크 (node 24+, pnpm, docker, git, tmux) + 선택 의존성 (zellij, gh)
  - 미설치 의존성에 대한 설치 프롬프트 (Homebrew 기반)
  - 시스템 로케일 자동 감지 i18n (ko/en/zh)
  - 깔끔한 터미널 UI (색상, 아이콘, 진행 표시)
  - PID 파일 기반 프로세스 관리
  - 기존 `start.sh` 삭제
- **Out of Scope**:
  - Linux 패키지 매니저 지원 (macOS Homebrew만)
  - 자동 업데이트 기능
  - 데몬 모드

## Codebase Analysis Summary
프로젝트는 Next.js 16 커스텀 서버(server.ts)를 boot.js를 통해 실행한다. Docker로 PostgreSQL을 관리하며 TypeORM 마이그레이션을 사용한다. tmux와 zellij는 터미널 세션 관리에 사용되고, gh는 PR URL 자동 감지에 사용된다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `kanvibe` | 통합 CLI 스크립트 | Create |
| `start.sh` | 기존 시작 스크립트 | Delete |
| `package.json` | pnpm scripts | Reference |
| `docker-compose.yml` | DB 컨테이너 설정 | Reference |
| `.env.example` | 환경변수 참조 | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 한국어 주석 | CLAUDE.md | 스크립트 내 주석은 한국어로 작성 |
| 환경변수 기본값 | .env.example, docker-compose.yml | KANVIBE_USER=admin, KANVIBE_PASSWORD=changeme, DB_PORT=4886 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 스크립트 형식 | bash 단일 파일 | 외부 의존성 없이 실행 | Python, Node.js |
| PID 관리 | `.kanvibe.pid` 파일 | 정확한 프로세스 종료 | pkill |
| i18n 구현 | bash 함수 + case 문 | bash 4+ 호환 | 별도 JSON 파싱 |
| 패키지 매니저 | Homebrew | macOS darwin 전용 | apt, yum |

## Implementation Todos

### Todo 1: kanvibe 스크립트 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `bash kanvibe start` / `bash kanvibe stop` 으로 실행 가능한 통합 CLI 스크립트 작성
- **Work**:
  - 프로젝트 루트에 `kanvibe` 파일 생성
  - i18n 함수: `detect_locale()` — $LANG에서 ko/en/zh 감지, 기본 en
  - i18n 메시지: `msg()` 함수 — 키 기반으로 현재 로케일 메시지 반환
  - 의존성 체크 함수: `check_deps()` — node(24+), pnpm, docker, git, tmux 필수 체크 + zellij, gh 선택
  - 의존성 설치 함수: `install_dep()` — Homebrew로 개별 설치, 설치 전 확인 프롬프트
  - `start` 서브커맨드: 의존성 체크 → pnpm install → docker compose up -d db → DB 대기 → 마이그레이션 → 빌드 → PID 기록 → 앱 시작
  - `stop` 서브커맨드: PID 파일로 앱 종료 → docker compose down
  - 터미널 UI: 색상(ANSI), 체크마크/엑스 아이콘, 단계 표시
  - `chmod +x kanvibe`
- **Convention Notes**: 주석 한국어, 환경변수 기본값은 .env.example과 동일
- **Verification**: `bash kanvibe` 실행 시 사용법 출력 확인
- **Exit Criteria**: `bash kanvibe start`가 의존성 체크 후 서버 시작, `bash kanvibe stop`이 서버+DB 종료
- **Status**: pending

### Todo 2: start.sh 삭제
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 기존 start.sh 파일 제거
- **Work**:
  - `start.sh` 파일 삭제
- **Convention Notes**: 없음
- **Verification**: start.sh 파일이 존재하지 않음
- **Exit Criteria**: 파일 삭제 완료
- **Status**: pending

## Verification Strategy
- `bash kanvibe` 실행 시 사용법 메시지 출력
- `bash kanvibe start` 실행 시 의존성 체크 UI 표시
- `bash kanvibe stop` 실행 시 프로세스 종료 동작
- shellcheck 또는 bash -n 문법 검증

## Progress Tracking
- Total Todos: 2
- Completed: 2
- Status: Execution complete

## Change Log
- 2026-02-18: Plan created
- 2026-02-18: Todo 1 completed — kanvibe script created with i18n, dep check, start/stop
- 2026-02-18: Todo 2 completed — start.sh deleted
- 2026-02-18: Additional — .kanvibe.pid added to .gitignore
