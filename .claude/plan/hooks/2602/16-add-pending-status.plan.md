# Add PENDING Status Between Progress and Review

## Business Goal
Claude Code hooks에서 AskUserQuestion 발생 시 사용자 의사결정 대기 상태를 별도 칸반 컬럼으로 시각화하여, progress(작업 중)와 review(검토 대기)를 명확히 구분한다.

## Scope
- **In Scope**: TaskStatus enum에 PENDING 추가, DB migration, Hook 스크립트 수정, API STATUS_MAP 갱신, Board UI 컬럼 추가, TaskStatusBadge 스타일, i18n 번역, CSS/디자인 토큰 추가
- **Out of Scope**: Done cleanup 로직, 기타 hook 동작 변경, Stop hook 동작 변경

## Codebase Analysis Summary
- `TaskStatus` enum (src/entities/KanbanTask.ts:12-17): TODO, PROGRESS, REVIEW, DONE
- Hook 스크립트 생성 (src/lib/claudeHooksSetup.ts): generatePromptHookScript(→progress), generateStopHookScript(→review), generateQuestionHookScript(→review)
- API route (src/app/api/hooks/status/route.ts): STATUS_MAP으로 string→TaskStatus 매핑
- Board UI (src/components/Board.tsx:26-31): COLUMNS 배열로 컬럼 정의
- TaskStatusBadge (src/components/TaskStatusBadge.tsx): 상태별 스타일 설정
- CSS (src/app/globals.css:78-81): status 색상 변수
- i18n (messages/*.json): columns 번역 키

### Relevant Files
| File | Role | Action |
|------|------|--------|
| src/entities/KanbanTask.ts | TaskStatus enum 정의 | Modify |
| src/migrations/*.ts | DB migration | Create |
| src/lib/database.ts | migration 등록 | Modify |
| src/lib/claudeHooksSetup.ts | Hook 스크립트 생성 | Modify |
| src/app/api/hooks/status/route.ts | Hook API endpoint | Modify |
| src/components/Board.tsx | 칸반 보드 UI | Modify |
| src/components/TaskStatusBadge.tsx | 상태 뱃지 스타일 | Modify |
| messages/ko.json | 한국어 번역 | Modify |
| messages/en.json | 영어 번역 | Modify |
| messages/zh.json | 중국어 번역 | Modify |
| src/app/globals.css | CSS 변수 | Modify |
| prd/design-system.json | 디자인 토큰 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Enum은 파일 분리 | BACKEND.md | enum은 별도 파일이지만, 기존 KanbanTask.ts에 이미 정의됨 → 유지 |
| Migration 후 database.ts에 등록 | CLAUDE.md | migrations 배열에 새 migration 클래스 import 추가 |
| CSS 변수 네이밍 | design-system.json | --color-status-{name} 패턴 |
| i18n 3개 파일 동시 수정 | CLAUDE.md | ko, en, zh 모두 동일 키로 번역 추가 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|------------|
| 새 상태 이름 | PENDING | 사용자 선택 | ASKING, DECISION |
| 컬럼 색상 | Purple (#8B5CF6) | 사용자 선택 | Orange, Red |
| Hook 변경 범위 | PreToolUse(AskUserQuestion)만 pending으로 변경 | Stop hook은 기존대로 review 유지 | - |

## Data Models

### TaskStatus Enum (변경)
| Value | DB Value | Description |
|-------|----------|-------------|
| TODO | "todo" | 할 일 |
| PROGRESS | "progress" | 진행 중 |
| **PENDING** | **"pending"** | **사용자 의사결정 대기** |
| REVIEW | "review" | 검토 대기 |
| DONE | "done" | 완료 |

## Implementation Todos

### Todo 1: TaskStatus enum에 PENDING 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: TaskStatus enum에 PENDING 값을 PROGRESS와 REVIEW 사이에 추가
- **Work**:
  - `src/entities/KanbanTask.ts`의 `TaskStatus` enum에 `PENDING = "pending"` 추가 (PROGRESS 다음, REVIEW 이전)
- **Convention Notes**: enum 값은 소문자 문자열
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: TaskStatus.PENDING이 정상적으로 참조 가능
- **Status**: pending

### Todo 2: DB migration 생성
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: PostgreSQL의 kanban_tasks.status enum 타입에 'pending' 값을 추가
- **Work**:
  - `npm run migration:generate` 또는 수동으로 migration 파일 작성
  - PostgreSQL ALTER TYPE ... ADD VALUE 'pending' BEFORE 'review' 사용
  - `src/lib/database.ts`의 migrations 배열에 새 migration 클래스 import 추가
- **Convention Notes**: migration 파일은 타임스탬프 기반 정렬, database.ts에 반드시 등록
- **Verification**: `npm run migration:run` 성공
- **Exit Criteria**: DB에 pending enum 값이 존재
- **Status**: pending

### Todo 3: Hook 스크립트 및 API 수정
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: AskUserQuestion hook이 pending 상태를 전송하도록 변경
- **Work**:
  - `src/lib/claudeHooksSetup.ts`의 `generateQuestionHookScript`: status를 "review" → "pending"으로 변경
  - `src/app/api/hooks/status/route.ts`의 `STATUS_MAP`에 `pending: TaskStatus.PENDING` 추가
- **Convention Notes**: 기존 코드 스타일 유지
- **Verification**: API에 `{"status": "pending"}` 전송 시 정상 처리
- **Exit Criteria**: PreToolUse(AskUserQuestion) hook이 pending 상태를 전송하고, API가 이를 처리
- **Status**: pending

### Todo 4: UI 업데이트 (Board, TaskStatusBadge, CSS, 디자인 토큰)
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 칸반 보드에 Pending 컬럼을 추가하고 시각적 스타일 적용
- **Work**:
  - `src/app/globals.css`:
    - `:root`에 `--color-status-pending: #8B5CF6` 추가
    - `@theme inline`에 `--color-status-pending` 등록
  - `prd/design-system.json`에 status-pending 토큰 추가
  - `src/components/Board.tsx`의 `COLUMNS` 배열에 PENDING 컬럼 추가 (PROGRESS 다음, REVIEW 이전)
  - `src/components/Board.tsx`의 `filteredTasks`에 `[TaskStatus.PENDING]: []` 추가
  - `src/components/TaskStatusBadge.tsx`의 `statusConfig`에 PENDING 스타일 추가 (purple 계열)
- **Convention Notes**: Tailwind CSS 변수 사용, bg-status-pending 클래스 사용
- **Verification**: 보드에 Pending 컬럼이 표시되고 올바른 색상 적용
- **Exit Criteria**: 5개 컬럼이 Todo → Progress → Pending → Review → Done 순서로 표시
- **Status**: pending

### Todo 5: i18n 번역 추가
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 3개 언어 파일에 pending 컬럼 번역 추가
- **Work**:
  - `messages/ko.json`의 `board.columns`에 `"pending": "Pending"` 추가
  - `messages/en.json`의 `board.columns`에 `"pending": "Pending"` 추가
  - `messages/zh.json`의 `board.columns`에 `"pending": "待决定"` 추가
- **Convention Notes**: 동일 키로 3개 파일 모두 업데이트
- **Verification**: 각 locale에서 Pending 컬럼 이름이 올바르게 표시
- **Exit Criteria**: ko, en, zh 번역 파일에 pending 키 존재
- **Status**: pending

## Verification Strategy
- TypeScript 컴파일: `npx tsc --noEmit`
- 빌드: `npm run build`
- DB migration 검토: 생성된 migration SQL 확인

## Progress Tracking
- Total Todos: 5
- Completed: 5
- Status: Execution complete

## Change Log
- 2026-02-16: Plan created
- 2026-02-16: All todos completed, build verification passed
