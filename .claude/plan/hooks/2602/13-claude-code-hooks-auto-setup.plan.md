# Claude Code Hooks 자동 설정 시스템

## Business Goal
Claude Code 사용 시 작업 상태를 칸반 보드에 자동 반영하여 AI 에이전트의 작업 진행 상황을 실시간으로 모니터링할 수 있도록 한다. 프로젝트 스캔 시 hooks를 자동으로 설치하여 수동 설정 부담을 제거한다.

## Scope
- **In Scope**: 
  - branchName + projectName 기반 상태 업데이트 API 엔드포인트
  - UserPromptSubmit / Stop hook 스크립트 2개
  - scanAndRegisterProjects 확장 (자동 hooks 설치)
  - 기존 미사용 엔드포인트 제거 (/api/hooks/update, /api/hooks/complete)
  - README.md Hook 가이드 업데이트
- **Out of Scope**: SSH remote repo 자동 설정, UI 변경

## Codebase Analysis Summary
- 기존 Hook API: `/api/hooks/start`, `/api/hooks/update`, `/api/hooks/complete` — 내부에서 호출하는 코드 없음
- Task 엔티티: `branchName` 필드 존재 (unique), `projectId` FK 존재
- Project 엔티티: `name` 필드 (unique), `repoPath` 필드
- 프로젝트 스캔: `scanAndRegisterProjects()` in `src/app/actions/project.ts`
- 기존 hooks 설정: `.claude/settings.json` — UserPromptSubmit, Notification, Stop 이벤트 사용 중
- Hook 스크립트 패턴: `.claude/hooks/skill-forced-eval-hook.sh` — bash, stdin JSON 파싱, cat heredoc 출력

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/api/hooks/status/route.ts` | branch+project 기반 상태 업데이트 API | Create |
| `src/app/api/hooks/update/route.ts` | 기존 id 기반 업데이트 API | Delete |
| `src/app/api/hooks/complete/route.ts` | 기존 id 기반 완료 API | Delete |
| `src/lib/claudeHooksSetup.ts` | hooks 자동 설정 유틸리티 | Create |
| `src/app/actions/project.ts` | 프로젝트 스캔/등록 서버 액션 | Modify |
| `README.md` | 프로젝트 문서 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| API route 패턴 | `/api/hooks/start/route.ts` | NextRequest/NextResponse, try-catch, JSON body 파싱 |
| 한국어 에러 메시지 | 기존 API 전체 | 에러 응답은 한국어로 |
| Entity import | 기존 코드 | `@/entities/KanbanTask`, `@/lib/database` |
| Hook 스크립트 | `skill-forced-eval-hook.sh` | bash, stdin JSON, jq 파싱 |
| Server action | `project.ts` | async function, serialize, revalidatePath |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Task 식별 | branchName + projectName | 복수 프로젝트에서 동일 브랜치명 가능, projectName으로 구분 | branchName only |
| 미사용 API 정리 | update/complete 삭제 | status 엔드포인트로 대체, 코드 정리 | 유지 |
| hooks 설치 위치 | 각 repo의 .claude/ | 프로젝트별 독립 설정 | 글로벌 ~/.claude/ |
| 설정 병합 전략 | JSON deep merge | 기존 settings.json 보존 | 덮어쓰기 |

## Implementation Todos

### Todo 1: branchName + projectName 기반 상태 업데이트 API 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Hook 스크립트가 호출할 API 엔드포인트를 생성한다
- **Work**:
  - `src/app/api/hooks/status/route.ts` 생성
  - POST 핸들러: `{ branchName: string, projectName: string, status: "progress" | "review" | "done" }` 수신
  - Project를 name으로 조회 → Task를 projectId + branchName으로 조회 → status 업데이트
  - 프로젝트/태스크 미발견 시 404 응답
- **Convention Notes**: 기존 `/api/hooks/start/route.ts`와 동일한 패턴 (try-catch, 한국어 에러)
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: `curl -X POST /api/hooks/status` 호출 시 정상 응답
- **Status**: pending

### Todo 2: 미사용 API 엔드포인트 제거
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `/api/hooks/update`와 `/api/hooks/complete`를 삭제하여 코드를 정리한다
- **Work**:
  - `src/app/api/hooks/update/route.ts` 삭제
  - `src/app/api/hooks/complete/route.ts` 삭제
  - `src/app/api/hooks/update/` 디렉토리 삭제
  - `src/app/api/hooks/complete/` 디렉토리 삭제
- **Convention Notes**: 내부 코드에서 참조하는 곳 없음 확인 완료
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 삭제된 엔드포인트 경로에 404 반환
- **Status**: pending

### Todo 3: Hook 스크립트 템플릿 및 자동 설정 유틸리티 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 프로젝트 스캔 시 자동으로 설치할 hook 스크립트와 settings.json 병합 로직을 구현한다
- **Work**:
  - `src/lib/claudeHooksSetup.ts` 생성
  - `generatePromptHookScript(kanvibeUrl, projectName)` — UserPromptSubmit hook bash 스크립트 생성
  - `generateStopHookScript(kanvibeUrl, projectName)` — Stop hook bash 스크립트 생성
  - `setupClaudeHooks(repoPath, projectName, kanvibeUrl)` — 메인 설정 함수
    - `.claude/hooks/` 디렉토리 생성
    - 스크립트 파일 생성 + chmod +x
    - 기존 `.claude/settings.json` 읽기 → hooks 섹션 deep merge → 저장
  - 병합 로직: 기존 UserPromptSubmit/Stop hooks 배열에 kanvibe hook 추가 (중복 방지)
- **Convention Notes**: 
  - hook 스크립트에서 `jq`로 stdin JSON 파싱, `git rev-parse --abbrev-ref HEAD`로 브랜치 획득
  - KANVIBE_URL은 스크립트 상단 변수로 주입
  - PROJECT_NAME도 스크립트 상단 변수로 주입
- **Verification**: 유닛 레벨에서 함수 호출 시 파일 생성 확인
- **Exit Criteria**: setupClaudeHooks 호출 시 .claude/hooks/ + settings.json 정상 생성
- **Status**: pending

### Todo 4: scanAndRegisterProjects에 자동 hooks 설정 통합
- **Priority**: 2
- **Dependencies**: Todo 3
- **Goal**: 프로젝트 스캔/등록 후 자동으로 Claude Code hooks를 설치한다
- **Work**:
  - `src/app/actions/project.ts`의 `scanAndRegisterProjects` 수정
  - 프로젝트 등록 성공 후 `setupClaudeHooks(repoPath, projectName, kanvibeUrl)` 호출
  - `kanvibeUrl`은 `process.env.PORT`에서 추출: `http://localhost:${PORT || 4885}`
  - ScanResult 타입에 `hooksSetup: string[]` 필드 추가
  - hooks 설정 실패 시 에러를 `result.errors`에 추가하되 등록은 유지
  - 로컬 repo만 대상 (sshHost가 있으면 hooks 설정 건너뜀)
- **Convention Notes**: 기존 scanAndRegisterProjects 패턴 유지, try-catch로 에러 격리
- **Verification**: TypeScript 컴파일 확인
- **Exit Criteria**: 프로젝트 스캔 시 등록된 repo에 .claude/hooks/ 자동 생성
- **Status**: pending

### Todo 5: README.md Hook 가이드 업데이트
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: Claude Code Hooks 설정 방법을 README에 정확하게 문서화한다
- **Work**:
  - README.md의 "Hook API (Claude Code 연동)" 섹션 전체 재작성
  - 삭제된 update/complete 엔드포인트 예시 제거
  - 새 `/api/hooks/status` 엔드포인트 문서화
  - hooks 동작 흐름 설명 (UserPromptSubmit → PROGRESS, Stop → REVIEW)
  - 수동 설정 방법 (settings.json + 스크립트 직접 작성)
  - 자동 설정 방법 (프로젝트 스캔 시 자동 설치)
  - 기술 스택 섹션의 Next.js 버전 16으로 업데이트
- **Convention Notes**: 기존 README 스타일 유지, 한국어, 코드 블록 사용
- **Verification**: 마크다운 렌더링 확인
- **Exit Criteria**: README에 정확한 hook 설정 가이드가 포함됨
- **Status**: pending

## Verification Strategy
- TypeScript 컴파일: `npx tsc --noEmit`
- 삭제된 엔드포인트 참조 없음 확인
- 생성된 hook 스크립트의 bash syntax 확인: `bash -n script.sh`

## Progress Tracking
- Total Todos: 5
- Completed: 5
- Status: Execution complete

## Change Log
- 2026-02-13: Plan created
- 2026-02-13: All 5 todos completed. TypeScript 컴파일 성공.
