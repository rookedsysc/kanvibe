# 프로젝트 이름 색상 변경 및 브랜치 작업 네비게이션

## Business Goal
Task Detail 페이지에서 프로젝트 이름을 더 진하고 가시성 높은 색상(#202632)으로 표시하고, 브랜치 이름을 클릭할 수 있게 만들어 같은 프로젝트의 다른 작업들을 빠르게 탐색할 수 있도록 개선한다.

## Scope
- **In Scope**:
  - 설계 시스템 토큰에 새로운 프로젝트 배경색 추가 (#202632)
  - Task Detail 페이지 메타데이터에 브랜치 이름 섹션 추가
  - 브랜치 이름 옆에 오른쪽 화살표(→) 아이콘 버튼 추가
  - 클릭 시 같은 프로젝트의 모든 작업을 브랜치별로 그룹화한 칸반 다이얼로그 표시
  - 다이얼로그에서 브랜치/작업 선택 후 해당 작업으로 이동

- **Out of Scope**:
  - 기존 브랜치 이름 태그의 스타일 변경 (현재 gray.100 유지)
  - 보드 페이지의 TaskCard 컴포넌트 수정
  - 새로운 API 엔드포인트 추가

## Codebase Analysis Summary

### 설계 시스템
- 토큰: `prd/design-system.json`에서 정의, `src/app/globals.css`의 CSS 변수로 구현
- 프로젝트 태그: 현재 `yellow.50` 배경(`#FEF7E0`), `gray.800` 텍스트(`#424242`)
- 새로운 색상 #202632는 text.primary와 유사한 진한 회색

### Task Detail Page
- 파일: `src/app/[locale]/task/[id]/page.tsx` (서버 컴포넌트)
- 메타데이터 섹션: line 126-206, `<dl>` 구조로 정보 표시
- 현재 표시 정보: project, priority, prUrl, agentType, sessionType, sshHost, createdAt, updatedAt
- `task.branchName` 필드 존재하지만 아직 UI에 표시되지 않음

### 기존 Dialog 패턴
- BranchTaskModal: 브랜치 분기 모달 사용 (`src/components/BranchTaskModal.tsx`)
- 고정 오버레이(`z-[400]`), 중앙 정렬, shadow/border 스타일
- form 기반으로 구현되어 있음

### Task 데이터 구조
- `KanbanTask` 엔티티: `branchName`, `projectId`, `baseBranch`, `project` (관계) 필드
- Board에서 같은 프로젝트의 작업들을 필터링하는 로직 이미 존재 (projectNameMap, projectFilterSet)

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `prd/design-system.json` | 설계 토큰 정의 | Modify - 프로젝트 배경색 토큰 추가 |
| `src/app/globals.css` | CSS 변수 선언 | Modify - 새 색상 변수 추가 |
| `src/app/[locale]/task/[id]/page.tsx` | Task Detail 페이지 | Modify - 메타데이터에 브랜치 섹션 추가, 모달 상태 관리 추가 |
| `src/components/ProjectBranchTasksModal.tsx` | 새 모달 컴포넌트 | Create - 같은 프로젝트 작업 브랜치별 표시 |
| `src/app/actions/kanban.ts` | 작업 조회 로직 | Reference - getTasksByProjectId 함수 확인 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 색상 토큰 네이밍 | design-system.json | `tags.project.background` 형식으로 정의 |
| 컴포넌트 파일명 | 기존 컴포넌트들 | PascalCase, .tsx 확장자 |
| 클라이언트 컴포넌트 | 기존 모달들 | `"use client"` 지시문 필수 |
| CSS 클래스 | tailwind + design-system | `bg-tag-project-bg` 같은 시맨틱 클래스 사용 |
| 주석 | CODE_PRINCIPLES.md | 한국어로 작성, JSDoc 형식 |
| 상태 관리 | Task Detail | useState/useTransition으로 모달 열기/닫기 |
| i18n | next-intl | `useTranslations()` 훅 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 색상 저장 방식 | 설계 토큰 추가 후 Tailwind arbitrary로 적용 | 일관성 유지, 다른 토큰과 동일 방식 | 직접 hex 색상 사용 (권장 안 함) |
| 모달 컴포넌트 위치 | `src/components/ProjectBranchTasksModal.tsx` | 기존 모달들과 같은 위치 | ProjectBranchTasksDialog.tsx (naming) |
| 모달 표시 방식 | 고정 오버레이 + 중앙 정렬 | 기존 BranchTaskModal과 동일 패턴 | Drawer/Sidebar (다른 UX) |
| 브랜치별 그룹화 | Map<branchName, KanbanTask[]> 구조 | 프론트엔드에서 처리, 간단하고 효율적 | 백엔드 정렬 (불필요한 복잡성) |
| 작업 조회 | 클라이언트에서 same projectId 필터링 | 기존 Board 로직과 동일 패턴 | API 엔드포인트 추가 (과설계) |

## Implementation Todos

### Todo 1: 설계 시스템 토큰 및 CSS 변수 업데이트
- **Priority**: 1 (독립적)
- **Dependencies**: none
- **Goal**: 프로젝트 배경색을 #202632로 변경하여 설계 시스템에 반영
- **Work**:
  - `prd/design-system.json` 열기
  - `colors.tags.project` 객체의 `background` 값을 `"#202632"`로 변경 (현재: `"{yellow.50}"`)
  - `src/app/globals.css`의 `:root` 섹션에서 `--color-tag-project-bg: var(--color-yellow-50);` 줄을 `--color-tag-project-bg: #202632;`로 변경
  - Tailwind @theme inline 블록 확인 (자동 생성되므로 수정 불필요)
- **Convention Notes**:
  - design-system.json은 시맨틱 참조 형식 지원 → primitive 색상 참조 가능하지만 직접 hex도 가능
  - CSS 변수명은 기존 규칙 유지: `--color-tag-project-bg`
- **Verification**:
  - design-system.json 문법 검증 (JSON 유효성)
  - globals.css 문법 검증 (CSS 유효성)
  - 빌드 성공 확인: `pnpm build`
  - 색상 변경 시각 확인 (브라우저에서 프로젝트 태그 색상이 #202632로 표시되는지)
- **Exit Criteria**:
  - 파일 수정 완료
  - 빌드 성공
  - task detail 페이지 프로젝트 태그가 어두운 회색(#202632) 배경으로 표시됨

### Todo 2: ProjectBranchTasksModal 컴포넌트 생성
- **Priority**: 1 (독립적)
- **Dependencies**: none
- **Goal**: 같은 프로젝트의 모든 작업을 브랜치별로 그룹화하여 표시하는 모달 컴포넌트 구현
- **Work**:
  - 새 파일 `src/components/ProjectBranchTasksModal.tsx` 생성
  - `"use client"` 지시문 추가
  - Props 타입: `{ projectId: string; currentBranchName: string | null; tasks: KanbanTask[]; onClose: () => void; onSelectTask: (taskId: string) => void; }`
  - 모달 구조:
    - 고정 오버레이: `z-[400]`, `bg-bg-overlay`, 클릭 시 닫기
    - 모달 본체: `max-w-2xl`, `bg-bg-surface`, `rounded-xl`, `border-border-default`, `shadow-lg`
    - 헤더: 제목 "프로젝트 작업 목록" (i18n), 닫기 버튼
    - 본문: 브랜치별 섹션으로 그룹화
      - `Map<branchName, KanbanTask[]>` 구조로 그룹화
      - 각 브랜치마다 `<details>` 또는 `<div className="mb-4">` 섹션
      - 브랜치명 표시: `<span className="text-sm font-semibold text-text-primary">`
      - 작업 목록: TaskCard 컴포넌트 재사용 또는 간단한 리스트 (링크 클릭 시 `onSelectTask(taskId)` 호출)
  - 국제화: `useTranslations("projectBranchTasks")` 사용
  - 스타일: 기존 BranchTaskModal 스타일과 일관성
- **Convention Notes**:
  - 컴포넌트 네이밍: `ProjectBranchTasksModal` (모달 타입 명시)
  - Props는 명확하게 named parameters
  - 브랜치별 그룹화는 프론트엔드에서 수행 (단순성)
  - TaskCard 재사용이 과할 수 있으니, 간단한 리스트 구조 추천 (projectName 표시 불필요)
- **Verification**:
  - TypeScript 컴파일 성공
  - 모달이 열릴 때 정상 렌더링 (console 에러 없음)
  - 같은 프로젝트의 작업들이 브랜치별로 그룹화되어 표시됨
  - 작업 클릭 시 `onSelectTask` 호출 확인 (개발자 도구)
- **Exit Criteria**:
  - 파일 생성 완료
  - 컴포넌트 컴파일 성공
  - 모달 구조 시각 확인 (브라우저에서)

### Todo 3: Task Detail 페이지 메타데이터에 브랜치 섹션 추가
- **Priority**: 2 (depends on Todo 1)
- **Dependencies**: Todo 1
- **Goal**: Task Detail 페이지의 메타데이터 카드에 브랜치 이름 정보를 추가하고 모달 열기 기능 구현
- **Work**:
  - `src/app/[locale]/task/[id]/page.tsx` 파일 수정
  - **서버 컴포넌트 부분** (기존):
    - 변경 없음 (task.branchName은 이미 필드로 존재)
  - **메타데이터 섹션** (line 126-206):
    - `{task.prUrl && ...}` 블록 뒤에, 새로운 섹션 추가:
    ```tsx
    {task.branchName && (
      <div className="flex items-center justify-between gap-2">
        <dt className="text-xs text-text-muted">{t("branch")}</dt>
        <dd className="flex items-center gap-2">
          <span className="text-xs px-2 py-0.5 rounded-full bg-tag-branch-bg text-tag-branch-text font-medium">
            {task.branchName}
          </span>
          <button
            onClick={() => setShowBranchTasksModal(true)}
            className="text-text-secondary hover:text-text-primary transition-colors"
            aria-label="View other tasks in this project"
            title="다른 작업 보기"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M6 12L10 8L6 4" strokeLinecap="round" strokeLinejoin="round"/>
            </svg>
          </button>
        </dd>
      </div>
    )}
    ```
  - 메타데이터 섹션 상단에 상태 선언 필요: `const [showBranchTasksModal, setShowBranchTasksModal] = useState(false);`
  - 메타데이터 카드 하단에 모달 렌더링: `{showBranchTasksModal && <ProjectBranchTasksModal ... />}`
  - 이를 위해 "use client" 지시문이 필요하므로 클라이언트 래퍼 컴포넌트 고려
- **Convention Notes**:
  - 서버/클라이언트 컴포넌트 분리 고려: 기존 page.tsx는 서버 컴포넌트이므로, TaskDetailSidebar 같은 클라이언트 컴포넌트로 메타데이터 섹션 분리 가능
  - 또는 상태 관리를 위해 기존 page.tsx를 클라이언트 컴포넌트로 변경 (이미 form/state 사용 중이므로 가능)
  - 화살표 아이콘: SVG 직접 구현 (기존 TaskCard의 PR 아이콘 스타일 참고)
  - 국제화: `t("branch")`, `t("viewOtherTasks")` 등 필요
- **Verification**:
  - Task Detail 페이지 열기
  - branchName이 있는 작업에서 "Branch" 메타데이터 섹션 표시 확인
  - 화살표 버튼 클릭 가능성 확인
  - console 에러 없음
- **Exit Criteria**:
  - 메타데이터 섹션에 브랜치 이름이 tag 형식으로 표시됨
  - 오른쪽 화살표(→) 아이콘이 브랜치명 옆에 표시됨
  - 버튼이 클릭 가능한 상태 (hover 효과 포함)

### Todo 4: ProjectBranchTasksModal 통합 및 네비게이션 완성
- **Priority**: 2 (depends on Todo 2, 3)
- **Dependencies**: Todo 2, 3
- **Goal**: 모달이 정상 작동하며 작업 선택 시 해당 페이지로 이동
- **Work**:
  - Task Detail page에 ProjectBranchTasksModal import: `import ProjectBranchTasksModal from "@/components/ProjectBranchTasksModal";`
  - 모달 렌더링 위치: 메타데이터 카드 하단 또는 사이드바 끝
  - Props 전달:
    - `projectId={task.projectId || ""}`
    - `currentBranchName={task.branchName}`
    - `tasks={initialTasks}` (page에서 받은 initialTasks 사용) 또는 API 호출로 다시 조회
    - `onClose={() => setShowBranchTasksModal(false)}`
    - `onSelectTask={(taskId) => { router.push(`/task/${taskId}`); }}`
  - 라우터 import: `import { useRouter } from "@/i18n/navigation";`
  - initialTasks 필터링: 같은 projectId를 가진 작업만 전달
  - 모달 열기: `{showBranchTasksModal && task.projectId && <ProjectBranchTasksModal ... />}`
- **Convention Notes**:
  - 클라이언트/서버 경계: 필요 시 기존 page.tsx를 클라이언트 컴포넌트로 변경 또는 래퍼 생성
  - 네비게이션: `useRouter().push()` 사용 (next/navigation 대신 @/i18n/navigation)
  - 작업 목록: initialTasks에서 필터링하여 사용 (효율성)
- **Verification**:
  - 모달 열기 성공
  - 같은 프로젝트의 모든 작업이 모달에 표시됨
  - 작업 클릭 시 해당 Task Detail 페이지로 이동
  - 뒤로가기 버튼으로 이전 페이지 돌아오기 가능
  - 화살표 아이콘 호버 시 커서 변경
- **Exit Criteria**:
  - 모달이 완전히 작동함
  - 작업 선택 후 해당 페이지로 이동 성공
  - UI 일관성 확인 (색상, 텍스트, 레이아웃)

## Verification Strategy
전체 구현 완료 후 검증:
1. **빌드 성공**: `pnpm build` - TypeScript 컴파일 에러 없음
2. **색상 변경**: Task Detail 페이지에서 프로젝트 태그 배경색이 #202632(진한 회색)로 표시됨
3. **브랜치 표시**: branchName이 있는 작업의 메타데이터에 "Branch" 섹션 표시
4. **모달 열기**: 화살표 아이콘 클릭 시 모달 오픈
5. **모달 콘텐츠**: 같은 프로젝트의 작업들이 브랜치별로 그룹화되어 표시됨
6. **네비게이션**: 모달 내 작업 클릭 시 해당 Task Detail 페이지로 이동
7. **UI 일관성**: 기존 모달/컴포넌트와 스타일 일치
8. **i18n**: 한국어/영어/중국어 모두 표시 확인 (번역 추가 필요)

## Progress Tracking
- Total Todos: 4
- Completed: 4
- Status: Execution complete

## Change Log
- 2026-02-17: Plan created
- 2026-02-17: All todos executed successfully
  - Todo 1: Design system color token updated (#202632)
  - Todo 2: ProjectBranchTasksModal component created
  - Todo 3: TaskDetailInfoCard created with branch section
  - Todo 4: Modal integration, navigation, and i18n translations added
