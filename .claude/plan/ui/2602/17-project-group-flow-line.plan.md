# Project Group & Branch Flow Line

## Business Goal
칸반 보드에서 동일 프로젝트의 태스크를 파스텔톤 테두리로 시각적으로 그룹화하고, 그룹 내에서 baseBranch → childBranch 브랜치 계층 관계를 git-like 트리 라인으로 표시하여 프로젝트별 작업 현황과 브랜치 흐름을 한눈에 파악할 수 있게 한다.

## Scope
- **In Scope**:
  - 컬럼 내 태스크를 프로젝트별로 정렬/그룹화 (같은 프로젝트끼리 인접 배치)
  - 프로젝트 그룹에 파스텔톤 테두리 + 프로젝트명 라벨 표시
  - 그룹 내 baseBranch → childBranch 트리 라인/화살표 (같은 컬럼 내에서만)
  - 8개 파스텔 색상 자동 할당 (CSS 변수, 프로젝트명 해시 기반)
  - worktree 프로젝트는 메인 프로젝트 그룹으로 통합 (기존 projectNameMap 로직 활용)
  - 프로젝트 없는 태스크는 그룹 없이 하단에 표시
- **Out of Scope**:
  - 크로스-컬럼 화살표
  - 프로젝트별 색상 수동 설정 (DB 필드)
  - 그룹 접기/펼치기 기능

## Codebase Analysis Summary
- 칸반 보드 구조: Board → Column → TaskCard (DragDropContext/Droppable/Draggable)
- `KanbanTask` 엔티티에 `projectId`, `baseBranch`, `branchName` 필드 존재
- `Project` 엔티티에 `defaultBranch`, `isWorktree` 필드 존재
- `Board.tsx`에서 `projectNameMap` (worktree → 메인 프로젝트명 resolve), `projectFilterSet` 이미 구현
- Column은 flat list로 TaskCard 렌더링, DnD index는 순서 기반
- 디자인 토큰: `:root`에 CSS 변수 → `@theme inline`에 Tailwind 등록 패턴

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/globals.css` | 디자인 토큰 정의 | Modify - 파스텔 그룹 색상 토큰 추가 |
| `prd/design-system.json` | 디자인 시스템 원본 | Modify - 그룹 색상 토큰 추가 |
| `src/components/Board.tsx` | 메인 보드 컴포넌트 | Modify - projectColorMap 계산, Column에 전달 |
| `src/components/Column.tsx` | 컬럼 컴포넌트 | Modify - 태스크 그룹핑 로직, ProjectTaskGroup 렌더링 |
| `src/components/ProjectTaskGroup.tsx` | 프로젝트 그룹 래퍼 | Create - 파스텔 테두리 + 트리 라인 렌더링 |
| `src/components/TaskCard.tsx` | 태스크 카드 | Reference - 기존 구조 유지 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| CSS 변수 네이밍 | globals.css | `--color-{category}-{name}` 패턴 |
| Tailwind 등록 | globals.css | `:root` 정의 후 `@theme inline`에 등록 |
| 컴포넌트 | 기존 패턴 | `"use client"` 지시자, default export |
| 주석 | CODE_PRINCIPLES | 한국어 서술형 |
| 네이밍 | CODE_PRINCIPLES | 명확한 전체 단어, 비즈니스 역할 표현 |
| i18n | CLAUDE.md | 새 UI 텍스트는 messages/{locale}.json에 번역 키 추가 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 그룹핑 방식 | 컬럼 내 정렬 후 wrapper div | DnD와 호환, 시각적으로 깔끔 | Overlay, Virtual grouping |
| 색상 할당 | 프로젝트명 해시 → 파스텔 팔레트 인덱스 | 세션 간 일관성, DB 변경 불필요 | 순서 기반, DB 저장 |
| 트리 라인 | CSS border-left + 작은 화살표 SVG | 경량, 추가 라이브러리 불필요 | Full SVG, Canvas |
| 새 컴포넌트 | ProjectTaskGroup.tsx | 그룹 렌더링 책임 분리 (SRP) | Column에 직접 구현 |
| DnD 호환 | Draggable index를 전체 컬럼 기준 유지 | 그룹핑 후에도 DnD 순서 유지 | 그룹 내 local index |

## Implementation Todos

### Todo 1: 파스텔 색상 디자인 토큰 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 8개 프로젝트 그룹 파스텔 색상을 CSS 변수로 정의
- **Work**:
  - `src/app/globals.css`의 `:root`에 8개 파스텔 색상 변수 추가:
    - `--color-group-0-border`, `--color-group-0-bg` ~ `--color-group-7-border`, `--color-group-7-bg`
    - border: 파스텔 중간 톤 (테두리용), bg: 매우 연한 파스텔 (배경용)
    - 색상: 핑크, 스카이블루, 민트, 라벤더, 피치, 레몬, 로즈, 세이지
  - `@theme inline` 블록에 동일 변수 Tailwind 등록
  - `--color-group-line`: 트리 라인 색상 (gray-300 수준)
  - `prd/design-system.json`에 해당 토큰 추가
- **Convention Notes**: `--color-{category}-{name}` 패턴, `:root` 정의 → `@theme inline` 등록
- **Verification**: CSS 파일 문법 오류 없음, `pnpm build` 성공
- **Exit Criteria**: 8개 그룹 색상 + 트리 라인 색상이 CSS 변수로 정의되고 Tailwind에서 사용 가능
- **Status**: pending

### Todo 2: Board에서 projectColorMap 계산 로직 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 프로젝트명 → 색상 인덱스(0-7) 매핑 계산 후 Column에 전달
- **Work**:
  - `src/components/Board.tsx`에 `projectColorMap` useMemo 추가:
    - `projectNameMap`에서 unique한 프로젝트명 추출
    - 프로젝트명의 간단한 해시값 → 0-7 인덱스 매핑 (일관성 보장)
    - `Record<string, number>` 타입 (resolvedProjectName → colorIndex)
  - `ColumnProps`에 `projectColorMap: Record<string, number>` 추가
  - Column 렌더링 시 `projectColorMap` prop 전달
- **Convention Notes**: useMemo로 계산, 기존 `projectNameMap` 의존
- **Verification**: TypeScript 타입 체크 통과
- **Exit Criteria**: Board가 Column에 프로젝트별 색상 인덱스를 전달하는 구조 완성
- **Status**: pending

### Todo 3: ProjectTaskGroup 컴포넌트 생성
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: 프로젝트 그룹의 파스텔 테두리 + 프로젝트명 라벨 + 브랜치 트리 라인을 렌더링하는 컴포넌트 생성
- **Work**:
  - `src/components/ProjectTaskGroup.tsx` 생성
  - Props: `projectName: string`, `colorIndex: number`, `tasks: KanbanTask[]`, `children: React.ReactNode`
  - 렌더링 구조:
    - 외부 div: `border-2 rounded-lg p-2 mb-3` + 동적 파스텔 border/bg 색상
    - 상단: 프로젝트명 라벨 (작은 텍스트)
    - 내부: children (TaskCard들)
  - 브랜치 트리 라인 렌더링:
    - `tasks` 배열에서 baseBranch → branchName 관계를 분석하여 트리 구조 계산
    - 루트 노드: baseBranch가 없거나 프로젝트 defaultBranch와 일치하는 태스크
    - 자식 노드: baseBranch가 다른 태스크의 branchName과 일치하는 태스크
    - 각 태스크 카드 왼쪽에 트리 라인 표시 (CSS border-left + pseudo-element)
    - 루트: 세로선 시작, 자식: 세로선에서 가로선으로 분기하는 L자 라인
  - 브랜치 관계가 없는 태스크(branchName 없음)는 라인 없이 일반 표시
- **Convention Notes**: `"use client"`, default export, 한국어 주석
- **Verification**: 컴포넌트 단독 렌더링 가능, 타입 체크 통과
- **Exit Criteria**: 파스텔 테두리 + 프로젝트명 + 트리 라인이 포함된 그룹 컴포넌트 완성
- **Status**: pending

### Todo 4: Column에서 태스크 그룹핑 및 ProjectTaskGroup 렌더링
- **Priority**: 3
- **Dependencies**: Todo 2, Todo 3
- **Goal**: Column 내 태스크를 프로젝트별로 그룹화하여 ProjectTaskGroup으로 렌더링
- **Work**:
  - `src/components/Column.tsx` 수정
  - `ColumnProps`에 `projectColorMap: Record<string, number>` 추가
  - 태스크 그룹핑 로직:
    - `tasks`를 projectName 기준으로 정렬 (같은 프로젝트끼리 인접)
    - 프로젝트 있는 태스크: 그룹별로 `ProjectTaskGroup` 래퍼로 감싸기
    - 프로젝트 없는 태스크: 그룹 없이 하단에 렌더링
  - DnD 호환:
    - Draggable `index`는 전체 컬럼 기준 연속 번호 유지
    - 그룹 wrapper div 안에 Draggable TaskCard 배치 (DnD와 호환)
  - `Board.tsx`에서 전달하는 `projectColorMap` 활용
- **Convention Notes**: DnD index 연속성 필수, 그룹 내/외 순서 일관성
- **Verification**: `pnpm build` 성공, DnD가 그룹핑 후에도 정상 동작
- **Exit Criteria**: 컬럼 내 태스크가 프로젝트별 파스텔 테두리 그룹으로 표시되고 DnD 동작 유지
- **Status**: pending

### Todo 5: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 새로 추가되는 UI 텍스트의 번역 키 등록
- **Work**:
  - `messages/ko.json`에 추가: `"board.noProject": "프로젝트 없음"` (그룹 없는 태스크 영역 라벨, 필요 시)
  - `messages/en.json`에 추가: `"board.noProject": "No Project"`
  - `messages/zh.json`에 추가: `"board.noProject": "无项目"`
  - Assumption: 현재 UI에서 별도 라벨이 필요 없으면 이 Todo는 skip 가능
- **Convention Notes**: 3개 언어 파일 동시 수정
- **Verification**: 빌드 시 번역 키 누락 에러 없음
- **Exit Criteria**: 새 UI에 필요한 번역 키가 3개 언어로 등록됨
- **Status**: pending

### Todo 6: 빌드 검증 및 최종 테스트
- **Priority**: 4
- **Dependencies**: Todo 4, Todo 5
- **Goal**: 전체 기능 통합 후 빌드 성공 및 타입 안전성 확인
- **Work**:
  - `pnpm build` 실행하여 전체 빌드 성공 확인
  - TypeScript 타입 에러 없음 확인
  - 기존 테스트 (`pnpm test`) 통과 확인
- **Convention Notes**: N/A
- **Verification**: `pnpm build && pnpm test` 성공
- **Exit Criteria**: 빌드 성공, 기존 테스트 통과, 타입 에러 없음
- **Status**: pending

## Verification Strategy
- `pnpm build`: 전체 빌드 성공
- `pnpm test`: 기존 테스트 회귀 없음
- 시각적 확인: 프로젝트 그룹 파스텔 테두리, 트리 라인 렌더링, DnD 동작

## Progress Tracking
- Total Todos: 6
- Completed: 6
- Status: Execution complete

## Change Log
- 2026-02-17: Plan created
- 2026-02-17: All todos executed and verified
