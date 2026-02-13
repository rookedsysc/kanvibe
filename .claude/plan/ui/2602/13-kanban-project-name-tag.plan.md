# Kanban TaskCard에 프로젝트 이름 태그 표시

## Business Goal
칸반 보드의 TaskCard에 프로젝트 이름을 표시하여 어떤 프로젝트의 작업인지 한눈에 파악할 수 있게 한다. Worktree 프로젝트인 경우 메인 프로젝트 이름을 표시한다.

## Scope
- **In Scope**: TaskCard에 프로젝트 이름 태그 추가, worktree→main project name 해석 로직, 프로젝트 태그 디자인 토큰, Board→Column→TaskCard props 전달
- **Out of Scope**: 백엔드 쿼리 변경, 엔티티 수정, 프로젝트 정렬/필터링, Task Detail 페이지 변경

## Codebase Analysis Summary
Board 컴포넌트는 `projects: Project[]`를 이미 props로 받고 있으나 Column/TaskCard로 전달하지 않음. getTasksByStatus()는 project relation을 로드하지 않으며, TaskCard는 task.projectId만 접근 가능. 기존 태그 패턴은 `text-xs px-2 py-0.5 rounded-full bg-tag-*-bg text-tag-*-text` 형식.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `prd/design-system.json` | 디자인 토큰 정의 | Modify |
| `src/app/globals.css` | CSS 변수 선언 + Tailwind 등록 | Modify |
| `src/components/Board.tsx` | 메인 보드 컨테이너 | Modify |
| `src/components/Column.tsx` | 상태별 컬럼 | Modify |
| `src/components/TaskCard.tsx` | 작업 카드 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 태그 스타일 | TaskCard.tsx | `text-xs px-2 py-0.5 rounded-full` 패턴 |
| 디자인 토큰 절차 | CLAUDE.md | design-system.json → globals.css :root → @theme inline |
| Props 타입 | Column.tsx, TaskCard.tsx | interface로 명시적 타입 정의 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 데이터 전달 | Board에서 projectNameMap 구성 후 props drilling | 기존 projects 데이터 활용, 백엔드 변경 없음 | getTasksByStatus에 relations 추가 |
| Worktree 해석 | repoPath에서 `__worktrees` 패턴 파싱 | 기존 디렉토리 컨벤션 활용 | isWorktree 플래그 + parentProjectId 컬럼 |
| 태그 색상 | yellow-50 bg + gray-800 text | 따뜻한 톤으로 시각적 구분, 기존 primitive 활용, 충분한 대비 | purple 계열 (비 Google 브랜드) |

## Implementation Todos

### Todo 1: 프로젝트 태그 디자인 토큰 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: `tag-project-bg/text` 디자인 토큰을 추가하여 프로젝트 태그에 사용할 색상을 정의한다
- **Work**:
  - `prd/design-system.json`의 `colors.tags` 객체에 `"project": { "background": "{yellow.50}", "text": "{gray.800}" }` 추가
  - `src/app/globals.css`의 `:root`에 `--color-tag-project-bg: var(--yellow-50)`, `--color-tag-project-text: var(--gray-800)` 추가
  - `src/app/globals.css`의 `@theme inline` 블록에 `--color-tag-project-bg`, `--color-tag-project-text` 등록
- **Convention Notes**: 기존 tag 토큰 패턴(bg + text 쌍)과 동일한 구조 유지
- **Verification**: CSS 파싱 오류 없는지 빌드 확인
- **Exit Criteria**: `bg-tag-project-bg text-tag-project-text` Tailwind 클래스 사용 가능
- **Status**: pending

### Todo 2: Board → Column → TaskCard 프로젝트 이름 전달 및 표시
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Board에서 projectNameMap을 구성하고 Column을 거쳐 TaskCard까지 전달, 카드에 프로젝트 이름 태그 렌더링
- **Work**:
  - `Board.tsx`: `projects` 배열로 `projectNameMap: Record<string, string>` 생성 (`useMemo`)
    - worktree 프로젝트(`repoPath`에 `__worktrees` 포함)는 메인 프로젝트 이름으로 resolve
    - 메인 프로젝트 찾기: repoPath에서 `__worktrees` 이전 경로 추출 → 동일 repoPath의 프로젝트 name 사용, 없으면 경로의 basename 사용
  - `Column.tsx`: `ColumnProps`에 `projectNameMap: Record<string, string>` 추가, TaskCard로 전달
  - `TaskCard.tsx`: `TaskCardProps`에 `projectName?: string` 추가, 브랜치 태그 앞에 프로젝트 이름 태그 렌더링
  - Board에서 Column 호출 시 `projectNameMap` 전달
  - Column에서 TaskCard 호출 시 `projectNameMap[task.projectId]`를 `projectName`으로 전달
- **Convention Notes**: 기존 태그 스타일(`text-xs px-2 py-0.5 rounded-full`) 재사용, truncate로 긴 이름 처리
- **Verification**: `npm run build` 성공, 타입 에러 없음
- **Exit Criteria**: TaskCard에 프로젝트 이름 태그가 브랜치 태그 앞에 표시됨, worktree 프로젝트는 메인 프로젝트 이름 표시
- **Status**: pending

## Verification Strategy
- `npm run build` 성공 확인
- TypeScript 타입 에러 없음 확인

## Progress Tracking
- Total Todos: 2
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-13: Plan created
