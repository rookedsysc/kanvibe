# Tmux Pane Layout Configuration

## Business Goal
tmux session에 최초 연결할 때 미리 설정한 pane 레이아웃(1~4분할)으로 window를 자동 분할하고, 각 pane에 지정된 시작 명령어를 실행하여 개발 환경 셋업을 자동화한다. 글로벌 기본값과 프로젝트별 오버라이드를 지원하여 프로젝트마다 최적화된 터미널 환경을 제공한다.

## Scope
- **In Scope**:
  - PaneLayoutConfig 엔티티 + TypeORM 마이그레이션
  - 6가지 레이아웃 타입 (SINGLE, HORIZONTAL_2, VERTICAL_2, LEFT_RIGHT_TB, LEFT_TB_RIGHT, QUAD)
  - 각 pane별 시작 명령어 저장 (단일 문자열)
  - `createWorktreeWithSession()` 수정 - 윈도우 생성 후 pane 분할 + 명령어 실행 (로컬 tmux만)
  - CRUD Server Actions
  - Pane Layout 설정 페이지 (`/[locale]/pane-layout/page.tsx`)
  - ProjectSettings에서 레이아웃 설정 페이지 링크 추가
  - i18n 번역 키 추가 (ko, en, zh)
- **Out of Scope**:
  - Remote(SSH) 환경의 pane 분할
  - Zellij pane 분할
  - 실시간 레이아웃 미리보기
  - 드래그앤드롭 pane 리사이징

## Codebase Analysis Summary
현재 시스템은 `createWorktreeWithSession()`에서 tmux window를 생성하고, `attachLocalSession()`에서 xterm.js를 통해 접속. 하나의 tmux window = 하나의 pane(분할 없음). xterm.js가 전체 tmux window를 렌더링하므로, tmux 내에서 pane을 분할하면 클라이언트에서 자동으로 보임.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/entities/PaneLayoutConfig.ts` | Pane 레이아웃 설정 엔티티 | Create |
| `src/entities/enums/PaneLayoutType.ts` | 레이아웃 타입 enum | Create |
| `src/migrations/1770854400003-AddPaneLayoutConfig.ts` | DB 마이그레이션 | Create |
| `src/lib/database.ts` | DataSource 설정 (엔티티, 마이그레이션 등록) | Modify |
| `src/lib/worktree.ts` | worktree + tmux window 생성 | Modify |
| `src/app/actions/paneLayout.ts` | CRUD Server Actions | Create |
| `src/app/[locale]/pane-layout/page.tsx` | 레이아웃 설정 페이지 | Create |
| `src/components/PaneLayoutEditor.tsx` | 레이아웃 편집 UI 컴포넌트 | Create |
| `src/components/ProjectSettings.tsx` | 프로젝트 설정 패널 | Modify |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Entity UUID | CLAUDE.md / BACKEND.md | `uuid.v7()` 사용 |
| Enum 파일 분리 | BACKEND.md | Enum은 별도 파일로 분리, DB에는 varchar 저장 |
| Migration | CLAUDE.md | generate 후 database.ts migrations 배열에 추가 |
| Server Actions | 기존 패턴 | `src/app/actions/` 디렉토리, `"use server"` 지시자 |
| i18n | CLAUDE.md | ko, en, zh 3개 언어 모두 번역 키 추가 |
| CSS | CLAUDE.md | 기존 디자인 토큰 CSS 변수 + Tailwind 클래스 |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석/설명은 한국어 |
| "use client" | 기존 패턴 | 클라이언트 컴포넌트에 지시자 필수 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 데이터 모델 | 별도 PaneLayoutConfig 엔티티 | Project 엔티티 오염 방지, 명확한 관심사 분리 | Project에 JSON 컬럼 |
| 레이아웃 타입 | DB varchar + TypeScript enum (6종) | 타입 안전성, 확장 용이 | 문자열 자유 입력 |
| Pane 명령어 저장 | JSONB `[{position, command}]` | 유연한 구조, 레이아웃별 pane 수 가변 | 별도 컬럼 4개 |
| 글로벌/프로젝트 구분 | projectId nullable (null=글로벌) | 단일 테이블로 통합 관리 | 별도 테이블 |
| Pane 분할 시점 | worktree 생성 시 | 터미널 접속 시 이미 준비, 일관된 상태 | 터미널 접속 시 |
| Pane 분할 대상 | 로컬 tmux만 | 복잡도 최소화, 우선 tmux 지원 | 원격 SSH + Zellij 포함 |

## Data Models

### PaneLayoutType (Enum)
| Value | Description | Pane Count |
|-------|-------------|------------|
| SINGLE | 분할 없음 | 1 |
| HORIZONTAL_2 | 상하 2분할 | 2 |
| VERTICAL_2 | 좌우 2분할 | 2 |
| LEFT_RIGHT_TB | 좌 + 우상하 3분할 | 3 |
| LEFT_TB_RIGHT | 좌상하 + 우 3분할 | 3 |
| QUAD | 4분할 (2x2) | 4 |

### PaneLayoutConfig (Entity)
| Field | Type | Constraints |
|-------|------|-------------|
| id | uuid | PK, uuid.v7() |
| layoutType | varchar | NOT NULL, PaneLayoutType enum |
| panes | jsonb | NOT NULL, `[{position: number, command: string}]` |
| projectId | uuid | FK → projects.id, nullable, UNIQUE |
| isGlobal | boolean | NOT NULL, default false |
| createdAt | timestamp | NOT NULL, UTC |
| updatedAt | timestamp | NOT NULL, UTC |

**Constraint**: `isGlobal=true` 레코드는 최대 1개 (projectId=null, isGlobal=true). projectId가 있으면 isGlobal=false.

### Pane Position Mapping
```
SINGLE:         [0]
HORIZONTAL_2:   [0(top), 1(bottom)]
VERTICAL_2:     [0(left), 1(right)]
LEFT_RIGHT_TB:  [0(left), 1(right-top), 2(right-bottom)]
LEFT_TB_RIGHT:  [0(left-top), 1(left-bottom), 2(right)]
QUAD:           [0(top-left), 1(top-right), 2(bottom-left), 3(bottom-right)]
```

## Implementation Todos

### Todo 1: PaneLayoutType enum + PaneLayoutConfig 엔티티 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: DB 스키마의 기반이 되는 TypeScript 엔티티와 enum 정의
- **Work**:
  - `src/entities/enums/PaneLayoutType.ts` 생성 - 6가지 레이아웃 타입 enum
  - `src/entities/PaneLayoutConfig.ts` 생성 - TypeORM 엔티티 (id, layoutType, panes, projectId, isGlobal, createdAt, updatedAt)
  - panes 컬럼은 `jsonb` 타입, `[{position: number, command: string}]` 구조
  - Project와 ManyToOne 관계 (nullable)
  - isGlobal + projectId unique constraint
- **Convention Notes**: UUID는 `uuid.v7()`, Enum은 별도 파일, DB에는 varchar 저장
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: 엔티티 파일과 enum 파일이 생성되고 타입 에러 없음
- **Status**: pending

### Todo 2: TypeORM 마이그레이션 생성 + database.ts 등록
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: DB에 pane_layout_configs 테이블 생성
- **Work**:
  - `src/lib/database.ts`에 PaneLayoutConfig 엔티티를 entities 배열에 추가
  - `npm run migration:generate -- src/migrations/AddPaneLayoutConfig` 실행
  - 생성된 마이그레이션 SQL 검토
  - `src/lib/database.ts`의 migrations 배열에 새 마이그레이션 클래스 import 추가
  - `npm run migration:run` 실행
- **Convention Notes**: migration 파일 생성 후 수정 금지, database.ts migrations 배열에 반드시 추가
- **Verification**: `npm run migration:run` 성공, DB에 테이블 생성 확인
- **Exit Criteria**: pane_layout_configs 테이블이 DB에 존재하고 마이그레이션 정상 실행
- **Status**: pending

### Todo 3: CRUD Server Actions 생성
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Pane 레이아웃 설정을 생성/조회/수정/삭제하는 서버 액션 제공
- **Work**:
  - `src/app/actions/paneLayout.ts` 생성
  - `getGlobalPaneLayout()`: 글로벌 기본 레이아웃 조회
  - `getProjectPaneLayout(projectId)`: 프로젝트별 레이아웃 조회 (없으면 글로벌 fallback)
  - `getEffectivePaneLayout(projectId?)`: 프로젝트 → 글로벌 → null 순서로 조회
  - `savePaneLayout(input)`: 생성 또는 업데이트 (upsert 패턴)
  - `deletePaneLayout(id)`: 삭제
- **Convention Notes**: `"use server"` 지시자, revalidatePath 호출, 에러 핸들링
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: 모든 CRUD 함수가 정의되고 타입 안전
- **Status**: pending

### Todo 4: worktree.ts에 pane 분할 로직 통합
- **Priority**: 3
- **Dependencies**: Todo 1, Todo 3
- **Goal**: tmux window 생성 후 레이아웃에 맞게 pane을 분할하고 각 pane에 명령어 실행
- **Work**:
  - `src/lib/worktree.ts`에 `applyPaneLayout(sessionName, windowName, layoutConfig, worktreePath)` 함수 추가
  - 레이아웃 타입별 tmux split-pane 명령어 시퀀스 구현:
    - SINGLE: 분할 없음, pane 0에 command만 send
    - HORIZONTAL_2: `split-window -v`
    - VERTICAL_2: `split-window -h`
    - LEFT_RIGHT_TB: `split-window -h` → 우측 pane 선택 → `split-window -v`
    - LEFT_TB_RIGHT: `split-window -h` → 좌측 pane 선택 → `split-window -v`
    - QUAD: `split-window -h` → `split-window -v` → 좌측 상단 선택 → `split-window -v`
  - 각 pane에 `tmux send-keys -t "{session}:{window}.{paneIndex}" "{command}" Enter` 실행
  - `createWorktreeWithSession()` 수정: window 생성 후 `getEffectivePaneLayout(projectId)` 조회 → `applyPaneLayout()` 호출
  - `createWorktreeWithSession()` 함수 시그니처에 `projectId` 파라미터 추가 (이미 호출부에서 projectId를 가지고 있음)
- **Convention Notes**: 한국어 주석, execGit 유틸 활용, 에러 시 graceful fallback (분할 실패해도 기본 window는 유지)
- **Verification**: 로컬에서 tmux session 생성 후 pane 분할 확인
- **Exit Criteria**: worktree 생성 시 설정된 레이아웃대로 pane이 분할되고 명령어가 실행됨
- **Status**: pending

### Todo 5: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Pane 레이아웃 관련 UI 텍스트의 다국어 지원
- **Work**:
  - `messages/ko.json`에 `paneLayout` 네임스페이스 추가:
    - title, description, layoutType labels, pane position labels, command placeholder, save/delete/reset buttons, global default label, project override label, success/error messages
  - `messages/en.json`에 동일 키 영어 번역
  - `messages/zh.json`에 동일 키 중국어 번역
  - `settings` 네임스페이스에 "paneLayoutLink" 키 추가 (ProjectSettings 내 링크 텍스트)
- **Convention Notes**: 번역 키 구조는 기존 패턴 참조 (flat namespace)
- **Verification**: JSON 파싱 오류 없음, 3개 언어 키 동일
- **Exit Criteria**: ko, en, zh 3개 파일에 동일한 키 구조로 번역 추가됨
- **Status**: pending

### Todo 6: PaneLayoutEditor 컴포넌트 생성
- **Priority**: 3
- **Dependencies**: Todo 3, Todo 5
- **Goal**: Pane 레이아웃을 시각적으로 선택하고 각 pane의 명령어를 입력하는 UI
- **Work**:
  - `src/components/PaneLayoutEditor.tsx` 생성 ("use client")
  - Props: `{ projectId?: string; initialConfig?: PaneLayoutConfig }`
  - 레이아웃 타입 선택 UI: 6가지 레이아웃을 아이콘/미니 프리뷰로 표시 (CSS grid로 시각화)
  - 선택된 레이아웃에 따라 pane 수만큼 명령어 입력 필드 동적 렌더링
  - 각 입력 필드에 pane 위치 라벨 표시 (상, 하, 좌, 우 등)
  - 저장 버튼 → `savePaneLayout()` server action 호출
  - 삭제 버튼 → `deletePaneLayout()` 호출 (프로젝트 설정만, 글로벌은 삭제 대신 리셋)
  - 기존 디자인 토큰 활용 (bg-bg-surface, border-border-default 등)
- **Convention Notes**: Tailwind 디자인 토큰 사용, useTranslations("paneLayout"), useTransition으로 pending 상태 관리
- **Verification**: 컴포넌트 렌더링 오류 없음
- **Exit Criteria**: 레이아웃 선택 + 명령어 입력 + 저장/삭제가 동작하는 UI 완성
- **Status**: pending

### Todo 7: Pane Layout 설정 페이지 생성
- **Priority**: 4
- **Dependencies**: Todo 6
- **Goal**: 글로벌 기본값과 프로젝트별 오버라이드를 관리하는 전용 페이지
- **Work**:
  - `src/app/[locale]/pane-layout/page.tsx` 생성 (서버 컴포넌트)
  - 글로벌 기본 레이아웃 섹션: PaneLayoutEditor (projectId 없이)
  - 프로젝트별 오버라이드 섹션: 프로젝트 목록 + 각 프로젝트의 PaneLayoutEditor
  - `getGlobalPaneLayout()`, `getAllProjects()` 서버에서 데이터 fetch
  - 뒤로가기 링크 (메인 보드로)
  - 프로젝트별 override 여부 표시 (설정됨/글로벌 기본값 사용 중)
- **Convention Notes**: next-intl getTranslations, 서버 컴포넌트에서 데이터 로딩
- **Verification**: 페이지 접속 시 정상 렌더링
- **Exit Criteria**: `/pane-layout` 페이지에서 글로벌 + 프로젝트별 레이아웃 설정 가능
- **Status**: pending

### Todo 8: ProjectSettings에 레이아웃 설정 링크 추가
- **Priority**: 4
- **Dependencies**: Todo 5, Todo 7
- **Goal**: 기존 ProjectSettings 패널에서 레이아웃 설정 페이지로 이동할 수 있는 링크 제공
- **Work**:
  - `src/components/ProjectSettings.tsx` 수정
  - 스캔 영역 아래 또는 프로젝트 목록 상단에 "Pane Layout 설정" 링크/버튼 추가
  - `@/i18n/navigation`의 `Link` 사용 → `/pane-layout` 경로
  - 기존 UI 스타일과 일관된 디자인 (text-sm, brand-primary 색상)
- **Convention Notes**: locale-aware Link 사용 (`import { Link } from "@/i18n/navigation"`), Tailwind 디자인 토큰
- **Verification**: ProjectSettings에서 링크 클릭 시 설정 페이지로 이동
- **Exit Criteria**: ProjectSettings에 링크가 표시되고 클릭 시 정상 네비게이션
- **Status**: pending

## Verification Strategy
- TypeScript 빌드: `npm run build` 성공
- DB 마이그레이션: `npm run migration:run` 성공
- 페이지 접근: `/ko/pane-layout` 접속 시 정상 렌더링
- 기능 테스트: 글로벌 레이아웃 저장 → 프로젝트 오버라이드 저장 → worktree 생성 시 pane 분할 확인

## Progress Tracking
- Total Todos: 8
- Completed: 8
- Status: Execution complete

## Change Log
- 2026-02-14: Plan created
- 2026-02-14: All 8 todos completed. TypeScript build + Next.js build verified.
