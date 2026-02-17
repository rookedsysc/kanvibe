# Add Priority Field to Kanban Tasks

## Business Goal
칸반 태스크에 우선순위(Priority) 필드를 추가하여 시각적 가시성을 높인다. `!` (파란색), `!!` (주황색), `!!!` (빨간색)으로 표현하며 null 허용. 칸반 보드 카드, 태스크 상세 페이지, 태스크 생성 모달에서 표시/설정 가능하게 한다.

## Scope
- **In Scope**: TaskPriority Enum, KanbanTask Entity 수정, DB Migration, Server Action 수정, 디자인 토큰 추가, TaskCard UI, Detail Page UI, CreateTaskModal UI, i18n 번역
- **Out of Scope**: 우선순위 기반 자동 정렬, 우선순위 필터링, Context Menu에서 우선순위 변경

## Codebase Analysis Summary
- Entity: `src/entities/KanbanTask.ts` — TypeORM 엔티티, Enum은 별도 파일 분리 패턴
- Server Actions: `src/app/actions/kanban.ts` — createTask, updateTask 함수
- TaskCard: `src/components/TaskCard.tsx` — 태그 스타일 기반 뱃지 표시
- Detail Page: `src/app/[locale]/task/[id]/page.tsx` — 메타데이터 카드에 정보 표시
- CreateTaskModal: `src/components/CreateTaskModal.tsx` — 태스크 생성 폼
- Migration: `src/lib/database.ts`에 마이그레이션 import 배열 관리
- Design: `prd/design-system.json` + `src/app/globals.css` CSS 변수

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/entities/TaskPriority.ts` | Priority Enum 정의 | Create |
| `src/entities/KanbanTask.ts` | Task 엔티티 | Modify (priority 컬럼 추가) |
| `src/migrations/TIMESTAMP-AddPriorityToKanbanTasks.ts` | DB 마이그레이션 | Create |
| `src/lib/database.ts` | 마이그레이션 import 배열 | Modify |
| `src/app/actions/kanban.ts` | createTask, updateTask 서버 액션 | Modify |
| `prd/design-system.json` | 디자인 토큰 | Modify |
| `src/app/globals.css` | CSS 변수 | Modify |
| `src/components/TaskCard.tsx` | 칸반 카드 UI | Modify |
| `src/components/CreateTaskModal.tsx` | 생성 모달 UI | Modify |
| `src/app/[locale]/task/[id]/page.tsx` | Detail 페이지 UI | Modify |
| `src/components/PrioritySelector.tsx` | 우선순위 선택 컴포넌트 | Create |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Enum 분리 | BACKEND.md | Enum은 별도 파일로 분리 |
| DB varchar 저장 | BACKEND.md | Enum은 varchar로 저장, Enum class로 변환 |
| 디자인 토큰 | CLAUDE.md | design-system.json → globals.css → @theme inline |
| i18n 3개 언어 | CLAUDE.md | ko, en, zh 동시 수정 |
| Migration import | CLAUDE.md | database.ts migrations 배열에 추가 |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Enum 값 | `low`/`medium`/`high` | 의미 명확, 기존 TaskStatus 패턴 일치 | 숫자 1/2/3 |
| 색상 매핑 | low=파란, medium=주황, high=빨강 | 사용자 요구 (!=파란, !!=주황, !!!=빨강) | - |
| 표시 기호 | `!`/`!!`/`!!!` 텍스트 | 사용자 요구 사항 그대로 | 아이콘, dot |
| 편집 UI | PrioritySelector 공용 컴포넌트 | CreateTaskModal + Detail 페이지 양쪽에서 재사용 | 각각 인라인 |
| nullable | yes | 사용자 요구 — 우선순위 미지정 가능 | default low |

## Data Models

### TaskPriority Enum
| Value | Display | Color |
|-------|---------|-------|
| `low` | `!` | 파란색 (#4285F4) |
| `medium` | `!!` | 주황색 (#F5A623) |
| `high` | `!!!` | 빨간색 (#EA4335) |
| `null` | - | 표시 안 함 |

### KanbanTask (수정)
| Field | Type | Constraints |
|-------|------|-------------|
| `priority` | `varchar` (enum TaskPriority) | nullable, default null |

## Implementation Todos

### Todo 1: TaskPriority Enum 생성 + Entity 수정
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 우선순위 데이터 모델 정의
- **Work**:
  - `src/entities/TaskPriority.ts` 생성 — `export enum TaskPriority { LOW = "low", MEDIUM = "medium", HIGH = "high" }`
  - `src/entities/KanbanTask.ts`에 `priority` 컬럼 추가 — `@Column({ type: "enum", enum: TaskPriority, nullable: true, default: null })`
- **Convention Notes**: Enum은 별도 파일 분리, varchar 저장
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: KanbanTask 엔티티에 priority 필드 존재
- **Status**: pending

### Todo 2: DB Migration 생성
- **Priority**: 1
- **Dependencies**: none (수동 작성)
- **Goal**: priority 컬럼을 DB에 추가하는 마이그레이션 생성
- **Work**:
  - `src/migrations/TIMESTAMP-AddPriorityToKanbanTasks.ts` 수동 작성
  - `ALTER TABLE kanban_tasks ADD COLUMN priority varchar DEFAULT NULL`
  - TypeORM enum type 생성: `CREATE TYPE kanban_tasks_priority_enum AS ENUM('low', 'medium', 'high')`
  - `src/lib/database.ts`의 migrations 배열에 import 추가
- **Convention Notes**: 기존 마이그레이션 파일 패턴 참조, timestamp 기반 이름
- **Verification**: migration 파일 문법 확인
- **Exit Criteria**: 마이그레이션 파일 존재 + database.ts에 등록
- **Status**: pending

### Todo 3: Design Tokens + CSS 변수 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 우선순위 색상 디자인 토큰 정의
- **Work**:
  - `prd/design-system.json`에 priority 색상 토큰 추가 (bg + text)
    - `priority-low-bg`, `priority-low-text` (파란 계열)
    - `priority-medium-bg`, `priority-medium-text` (주황 계열)
    - `priority-high-bg`, `priority-high-text` (빨간 계열)
  - `src/app/globals.css`의 `:root`에 CSS 변수 추가
  - `@theme inline` 블록에 Tailwind 등록
- **Convention Notes**: 기존 tag 색상 토큰 패턴 (bg + text) 따르기
- **Verification**: CSS 변수 선언 확인
- **Exit Criteria**: Tailwind에서 `bg-priority-low-bg`, `text-priority-low-text` 등 사용 가능
- **Status**: pending

### Todo 4: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 우선순위 관련 번역 키 추가
- **Work**:
  - `messages/ko.json` — task 네임스페이스에 `priority`, `priorityLow`, `priorityMedium`, `priorityHigh`, `priorityNone` 추가; taskDetail에 `priority` 추가
  - `messages/en.json` — 동일 키 영어 번역
  - `messages/zh.json` — 동일 키 중국어 번역
- **Convention Notes**: 3개 언어 동시 수정
- **Verification**: JSON 문법 에러 없음
- **Exit Criteria**: 모든 번역 키가 3개 언어 파일에 존재
- **Status**: pending

### Todo 5: Server Action 수정 (createTask, updateTask)
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 서버 액션에서 priority 필드 처리
- **Work**:
  - `src/app/actions/kanban.ts`의 `CreateTaskInput` 인터페이스에 `priority?: TaskPriority` 추가
  - `createTask` 함수에서 `priority` 필드 설정 (`input.priority || null`)
  - `updateTask` 함수의 `updates` 타입에 `priority` 추가 + 처리 로직
- **Convention Notes**: 기존 nullable 필드 패턴 따르기 (agentType 등과 동일)
- **Verification**: TypeScript 컴파일 에러 없음
- **Exit Criteria**: createTask, updateTask 모두 priority 처리 가능
- **Status**: pending

### Todo 6: PrioritySelector 공용 컴포넌트 생성
- **Priority**: 2
- **Dependencies**: Todo 3
- **Goal**: CreateTaskModal과 Detail 페이지에서 재사용할 우선순위 선택 컴포넌트
- **Work**:
  - `src/components/PrioritySelector.tsx` 생성
  - Props: `value: TaskPriority | null`, `onChange: (priority: TaskPriority | null) => void`
  - 각 우선순위 옵션을 색상 태그 버튼으로 표시 (none / ! / !! / !!!)
  - 선택된 옵션 하이라이트
- **Convention Notes**: "use client" 디렉티브, 기존 태그 스타일 패턴
- **Verification**: 컴포넌트 렌더링 확인
- **Exit Criteria**: PrioritySelector가 priority 선택/해제 가능
- **Status**: pending

### Todo 7: TaskCard UI에 우선순위 표시
- **Priority**: 2
- **Dependencies**: Todo 3
- **Goal**: 칸반 보드 카드에 우선순위 뱃지 표시
- **Work**:
  - `src/components/TaskCard.tsx` 수정
  - task.priority가 존재할 때 제목 앞 또는 태그 영역에 `!`/`!!`/`!!!` 뱃지 추가
  - 색상: priority-{level}-bg / priority-{level}-text 토큰 사용
- **Convention Notes**: 기존 agentType 태그와 동일한 스타일 패턴
- **Verification**: 시각적 확인
- **Exit Criteria**: 우선순위 있는 태스크에 색상 뱃지 표시
- **Status**: pending

### Todo 8: CreateTaskModal에 우선순위 선택 추가
- **Priority**: 3
- **Dependencies**: Todo 5, Todo 6
- **Goal**: 태스크 생성 시 우선순위 설정 가능
- **Work**:
  - `src/components/CreateTaskModal.tsx` 수정
  - PrioritySelector 컴포넌트 추가
  - handleSubmit에서 priority 값을 createTask에 전달
- **Convention Notes**: 기존 폼 필드 스타일과 일관성
- **Verification**: 모달에서 우선순위 선택 후 생성 시 DB에 저장 확인
- **Exit Criteria**: 생성 시 priority 값 전달 및 저장
- **Status**: pending

### Todo 9: Detail 페이지에 우선순위 표시 + 변경 기능
- **Priority**: 3
- **Dependencies**: Todo 5, Todo 6
- **Goal**: 상세 페이지에서 우선순위 표시 및 변경
- **Work**:
  - `src/app/[locale]/task/[id]/page.tsx` 수정
  - 메타데이터 카드에 우선순위 항목 추가 (PrioritySelector 사용)
  - 클라이언트 컴포넌트로 우선순위 변경 핸들러 구현 (updateTask 호출)
  - 필요 시 `PriorityEditor.tsx` 클라이언트 컴포넌트 별도 생성
- **Convention Notes**: 서버 컴포넌트 + 클라이언트 컴포넌트 분리 패턴
- **Verification**: Detail 페이지에서 우선순위 변경 시 DB 반영 확인
- **Exit Criteria**: 우선순위 표시 + 변경 + revalidate 동작
- **Status**: completed

## Verification Strategy
- `pnpm build` 성공 (TypeScript 컴파일 + Next.js 빌드)
- 칸반 보드에서 우선순위 뱃지가 올바른 색상으로 표시
- Detail 페이지에서 우선순위 표시 및 변경 가능
- CreateTaskModal에서 우선순위 선택 후 생성 가능
- null priority 태스크에서 뱃지 미표시 확인

## Progress Tracking
- Total Todos: 9
- Completed: 9
- Status: Execution complete

## Change Log
- 2026-02-17: Plan created
- 2026-02-17: All 9 todos executed and completed
