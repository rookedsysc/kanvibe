# Fix Duplicated Branch Name Display

## Business Goal
칸반 보드에서 브랜치 이름과 태스크 제목이 동일할 때 중복 표시되는 문제를 해결하여 UI의 가독성을 높인다. 카드에 description 미리보기(2줄)를 추가하여 태스크 내용을 한눈에 파악할 수 있게 한다.

## Scope
- **In Scope**: TaskCard 브랜치 태그 조건부 숨김, 상세 페이지 브랜치 메타데이터 조건부 숨김, 카드 description 2줄 미리보기
- **Out of Scope**: 데이터 모델 변경, CreateTaskModal 수정, 번역 파일 추가

## Codebase Analysis Summary
TaskCard.tsx에서 `task.title`과 `task.branchName`이 항상 동일 값(CreateTaskModal에서 branchName을 title로 설정)으로 카드에 제목+태그 두 번 표시. 상세 페이지(page.tsx)도 동일하게 제목 영역과 메타데이터 섹션에 중복.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/TaskCard.tsx` | 칸반 카드 렌더링 | Modify |
| `src/app/[locale]/task/[id]/page.tsx` | 태스크 상세 페이지 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Tailwind CSS 변수 토큰 | CLAUDE.md | `text-text-secondary` 등 디자인 토큰 사용 |
| line-clamp | Tailwind v4 | `line-clamp-2` 유틸리티 클래스 사용 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 숨김 방식 | 조건부 (`branchName === title`) | title과 branchName이 다른 경우에도 올바르게 동작 | 항상 숨김 (유연성 부족) |
| Description 줄 제한 | CSS `line-clamp-2` | Tailwind 기본 지원, 추가 JS 불필요 | JS 기반 truncation |

## Implementation Todos

### Todo 1: TaskCard 브랜치 태그 조건부 숨김 + description 미리보기 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 카드에서 브랜치 태그 중복 제거, description 2줄 미리보기 추가
- **Work**:
  - `src/components/TaskCard.tsx` 47-51행: `task.branchName` 렌더링 조건에 `task.branchName !== task.title` 추가
  - `task.title` h3 태그 아래, 태그 영역 위에 `task.description`을 `line-clamp-2`로 표시하는 p 태그 추가
- **Convention Notes**: 텍스트 색상은 `text-text-secondary`, 폰트 크기는 `text-xs`
- **Verification**: `npm run build`로 빌드 성공 확인
- **Exit Criteria**: 카드에서 branchName === title일 때 브랜치 태그 미표시, description 2줄 표시
- **Status**: pending

### Todo 2: 상세 페이지 브랜치 메타데이터 조건부 숨김
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 상세 페이지에서 브랜치 메타데이터 중복 제거
- **Work**:
  - `src/app/[locale]/task/[id]/page.tsx` 129-138행: `task.branchName` 렌더링 조건에 `task.branchName !== task.title` 추가
- **Convention Notes**: 기존 코드 패턴 유지, 조건만 추가
- **Verification**: `npm run build`로 빌드 성공 확인
- **Exit Criteria**: 상세 페이지에서 branchName === title일 때 브랜치 메타데이터 미표시
- **Status**: pending

## Verification Strategy
- `npm run build`로 전체 빌드 성공 확인

## Progress Tracking
- Total Todos: 2
- Completed: 2
- Status: Execution complete

## Change Log
- 2026-02-14: Plan created
- 2026-02-14: All todos completed, TypeScript check passed
