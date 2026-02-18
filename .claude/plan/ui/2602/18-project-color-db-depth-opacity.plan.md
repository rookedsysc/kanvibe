# Project Color DB 저장 + 자식 브랜치 명암 표현

## Business Goal
프로젝트 그룹 색상을 DB에 영속화하여 사용자가 직접 변경할 수 있게 하고, 브랜치 계층에 따른 색상 명암으로 부모-자식 관계를 시각적으로 표현한다.

## Scope
- **In Scope**:
  - Project 엔티티에 `colorIndex` 컬럼 추가 (DB 마이그레이션)
  - Board에서 DB colorIndex 우선 사용, null이면 해시 fallback
  - TaskDetail 페이지에 프로젝트 색상 인라인 편집기 추가
  - 프로젝트 그룹 내 자식 브랜치 태스크에 depth 기반 opacity 적용
- **Out of Scope**:
  - HEX 커스텀 색상 입력
  - worktree 프로젝트 독립 색상 설정

## Codebase Analysis Summary
- Project 엔티티: `src/entities/Project.ts` (id, name, repoPath, defaultBranch, sshHost, isWorktree, createdAt)
- Board의 projectColorMap: useMemo 해시 기반 (projectNameMap → 0-7 인덱스)
- Column: ProjectTaskGroup 래퍼 + BranchConnector + depth 기반 indentation
- TaskDetailInfoCard: PriorityEditor 패턴 참고 가능
- 마이그레이션: `src/migrations/` 타임스탬프 기반, `database.ts` migrations 배열에 수동 추가

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/entities/Project.ts` | Project 엔티티 | Modify - colorIndex 추가 |
| `src/migrations/` | DB 마이그레이션 | Create - AddColorIndexToProjects |
| `src/lib/database.ts` | 런타임 DataSource | Modify - migrations 배열에 추가 |
| `src/app/actions/kanban.ts` | Server actions | Modify - updateProjectColor 추가 |
| `src/components/Board.tsx` | 보드 메인 | Modify - projectColorMap DB 값 우선 사용 |
| `src/components/ProjectColorEditor.tsx` | 색상 편집기 | Create - 인라인 색상 선택 UI |
| `src/components/TaskDetailInfoCard.tsx` | Detail 사이드바 | Modify - ProjectColorEditor 통합 |
| `src/components/Column.tsx` | 컬럼 렌더링 | Modify - 자식 브랜치 opacity 적용 |
| `src/components/ProjectTaskGroup.tsx` | 그룹 래퍼 | Modify - depth 기반 스타일 전달 |
| `messages/*.json` | i18n | Modify - 번역 키 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 마이그레이션 | CLAUDE.md | 엔티티 수정 → generate → database.ts에 import 추가 |
| Server action | 기존 패턴 | `"use server"`, revalidatePath, serialize |
| 인라인 편집기 | PriorityEditor | 클릭 시 드롭다운, server action 호출 |
| i18n | CLAUDE.md | ko/en/zh 3개 파일 동시 수정 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| DB 타입 | smallint nullable | null = 해시 fallback, 0-7 범위 | varchar hex |
| 편집 UI | 8색 원형 스와치 드롭다운 | PriorityEditor 패턴 일관, 직관적 | 모달, 슬라이더 |
| 자식 opacity | inline style opacity | CSS 변수 추가 없이 간단 | HSL lightness |
| worktree 색상 | 메인 프로젝트 colorIndex 조회 | 기존 통합 로직 유지 | 개별 설정 |

## Implementation Todos

### Todo 1: Project 엔티티 + 마이그레이션
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Project 테이블에 color_index 컬럼 추가
- **Work**:
  - `src/entities/Project.ts`에 colorIndex 프로퍼티 추가: `@Column({ name: "color_index", type: "smallint", nullable: true, default: null }) colorIndex!: number | null;`
  - `pnpm migration:generate -- src/migrations/AddColorIndexToProjects` 실행
  - 생성된 마이그레이션 파일 검토
  - `src/lib/database.ts` migrations 배열에 새 마이그레이션 import 추가
- **Convention Notes**: 마이그레이션 파일은 생성 후 수정하지 않음
- **Verification**: `pnpm migration:run` 성공
- **Exit Criteria**: DB에 color_index 컬럼 존재, 기존 데이터는 null
- **Status**: pending

### Todo 2: Server action - updateProjectColor
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 프로젝트 색상 변경 API 추가
- **Work**:
  - `src/app/actions/kanban.ts`에 `updateProjectColor(projectId: string, colorIndex: number)` 추가
  - 동일 repoPath의 worktree 프로젝트도 같은 colorIndex로 업데이트
  - revalidatePath 호출
  - serialize 패턴 따름
- **Convention Notes**: 기존 server action 패턴, `"use server"`, revalidatePath
- **Verification**: TypeScript 타입 체크 통과
- **Exit Criteria**: 프로젝트 색상을 DB에 저장하고 관련 worktree도 동기화하는 server action 완성
- **Status**: pending

### Todo 3: Board - DB colorIndex 우선 사용
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: projectColorMap에서 DB값 우선, null이면 해시 fallback
- **Work**:
  - `src/components/Board.tsx`의 `projectColorMap` useMemo 수정
  - projects 배열에서 메인 프로젝트의 colorIndex를 조회
  - colorIndex가 있으면 사용, null이면 기존 해시 로직 fallback
  - worktree 프로젝트는 메인 프로젝트의 colorIndex 상속
- **Convention Notes**: 기존 projectNameMap, projectColorMap 구조 유지
- **Verification**: TypeScript 타입 체크 통과
- **Exit Criteria**: DB에 colorIndex가 저장된 프로젝트는 해당 색상 사용
- **Status**: pending

### Todo 4: ProjectColorEditor 컴포넌트
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: 프로젝트 색상을 변경할 수 있는 인라인 편집기 컴포넌트
- **Work**:
  - `src/components/ProjectColorEditor.tsx` 생성
  - Props: `projectId: string`, `currentColorIndex: number | null`
  - UI: 현재 색상 원형 표시, 클릭 시 8색 스와치 드롭다운
  - 선택 시 `updateProjectColor` server action 호출
  - PriorityEditor 패턴 참고 (useTransition, 드롭다운)
- **Convention Notes**: `"use client"`, 기존 인라인 편집기 패턴
- **Verification**: 컴포넌트 렌더링 가능, 타입 체크 통과
- **Exit Criteria**: 8색 중 선택하여 프로젝트 색상 변경 가능
- **Status**: pending

### Todo 5: TaskDetailInfoCard에 색상 편집기 통합
- **Priority**: 3
- **Dependencies**: Todo 4
- **Goal**: Detail 페이지에서 프로젝트 색상 편집 가능
- **Work**:
  - `src/components/TaskDetailInfoCard.tsx` 수정
  - 기존 project 표시 영역에 ProjectColorEditor 추가
  - task.project.colorIndex를 전달
- **Convention Notes**: 기존 PriorityEditor 배치 패턴과 동일
- **Verification**: Detail 페이지에서 색상 변경 UI 표시
- **Exit Criteria**: project 정보 옆에 색상 편집기 표시, 변경 가능
- **Status**: pending

### Todo 6: 자식 브랜치 depth 기반 opacity 적용
- **Priority**: 2
- **Dependencies**: none
- **Goal**: 프로젝트 그룹 내 자식 태스크의 테두리/배경 색상을 depth에 비례하여 연하게
- **Work**:
  - `src/components/Column.tsx` 수정
  - 그룹 내 각 태스크 렌더링 시 depth에 기반한 opacity 적용
  - depth 0: opacity 100% (원래 색상), depth 1: opacity 65%, depth 2+: opacity 40%
  - TaskCard 래퍼 div에 style={{ opacity }} 적용 (그룹 테두리 내부)
  - BranchConnector도 동일 opacity 적용
- **Convention Notes**: inline style 사용 (동적 값)
- **Verification**: 타입 체크 통과
- **Exit Criteria**: 자식 브랜치 태스크가 시각적으로 연한 색상으로 표시
- **Status**: pending

### Todo 7: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 색상 편집기 관련 번역 키 등록
- **Work**:
  - `messages/ko.json`: `"taskDetail.projectColor": "프로젝트 색상"`
  - `messages/en.json`: `"taskDetail.projectColor": "Project Color"`
  - `messages/zh.json`: `"taskDetail.projectColor": "项目颜色"`
- **Convention Notes**: 3개 언어 동시 수정
- **Verification**: 빌드 시 번역 키 누락 에러 없음
- **Exit Criteria**: 번역 키 등록 완료
- **Status**: pending

### Todo 8: 빌드 검증
- **Priority**: 4
- **Dependencies**: Todo 3, Todo 5, Todo 6, Todo 7
- **Goal**: 전체 빌드 + 테스트 통과
- **Work**:
  - `npx tsc --noEmit` TypeScript 검증
  - `pnpm test` 기존 테스트 통과
- **Verification**: 모든 검증 통과
- **Exit Criteria**: 타입 에러 0, 테스트 84/84 통과
- **Status**: pending

## Verification Strategy
- `npx tsc --noEmit`: TypeScript 타입 안전성
- `pnpm test`: 기존 테스트 회귀 없음
- `pnpm migration:run`: 마이그레이션 적용 성공

## Progress Tracking
- Total Todos: 8
- Completed: 8
- Status: Execution complete

## Change Log
- 2026-02-18: Plan created
- 2026-02-18: All todos executed and verified
