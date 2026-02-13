# 칸반 보드 프로젝트 필터

## Business Goal
칸반 보드에 프로젝트 필터를 추가하여 특정 프로젝트의 태스크만 볼 수 있게 한다. 필터는 localStorage에 저장되어 세션 간 유지된다. 프로젝트 선택 시 해당 프로젝트 + worktree 프로젝트의 태스크를 함께 표시한다. 태스크 생성 시 필터된 프로젝트가 자동 설정된다.

## Scope
- **In Scope**: 프로젝트 필터 드롭다운 UI, localStorage 저장/복원, 클라이언트 필터링, worktree 프로젝트 포함 필터, CreateTaskModal에 기본 프로젝트 전달, 필터 적용 시 드래그 핸들러 보정
- **Out of Scope**: 서버사이드 필터링, DB 변경, 프로젝트 간 드래그 이동

## Relevant Files
| File | Role | Action |
|------|------|--------|
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |
| `src/components/Board.tsx` | 메인 보드 컨테이너 | Modify |
| `src/components/CreateTaskModal.tsx` | 태스크 생성 모달 | Modify |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 필터 방식 | 클라이언트 필터링 | 데이터 구조/API 변경 없음 | 서버사이드 필터 |
| 저장소 | localStorage | 단순, 서버 불필요 | cookie, URL param |
| Worktree 포함 | repoPath 기반 매칭 | 기존 __worktrees 패턴 활용 | isWorktree 플래그 + parentId |
| 드래그 핸들러 | 필터 인덱스→전체 인덱스 매핑 | 정확한 위치 삽입 | 단순 append (부정확) |

## Implementation Todos

### Todo 1: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 프로젝트 필터 UI에 사용할 번역 키 추가
- **Work**:
  - `messages/ko.json`의 `board` 객체에 `"allProjects": "전체 프로젝트"` 추가
  - `messages/en.json`의 `board` 객체에 `"allProjects": "All Projects"` 추가
  - `messages/zh.json`의 `board` 객체에 `"allProjects": "所有项目"` 추가
- **Verification**: JSON 파싱 오류 없음
- **Exit Criteria**: `t("board.allProjects")` 사용 가능
- **Status**: pending

### Todo 2: Board에 프로젝트 필터 구현
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Board 컴포넌트에 프로젝트 필터 state, localStorage 저장/복원, 필터 UI, 필터링 로직, 드래그 핸들러 보정을 구현
- **Work**:
  - `selectedProjectId` state 추가 (초기값: "")
  - localStorage 복원 useEffect (`kanvibe:projectFilter` 키)
  - localStorage 저장 useEffect
  - `projectFilterSet: Set<string>` useMemo: 선택된 프로젝트 + repoPath 기반 worktree 프로젝트 ID 집합
  - `filteredTasks: TasksByStatus` useMemo: projectFilterSet으로 필터링
  - `insertAtFilteredIndex` 헬퍼 함수: 필터된 인덱스를 전체 배열 위치로 매핑하여 삽입
  - `handleDragEnd` 수정: `draggableId`로 태스크 찾기, `insertAtFilteredIndex`로 정확한 위치 삽입
  - 헤더에 `<select>` 드롭다운 추가: "전체 프로젝트" + 개별 프로젝트 (isWorktree 제외)
  - Column에 `filteredTasks[col.status]` 전달
  - CreateTaskModal에 `defaultProjectId={selectedProjectId}` 전달
- **Convention Notes**: 기존 디자인 토큰 사용, useCallback/useMemo 패턴 유지
- **Verification**: TypeScript 타입 에러 없음
- **Exit Criteria**: 필터 선택 시 해당 프로젝트+worktree 태스크만 표시, localStorage 저장/복원 동작, 드래그 정상 동작
- **Status**: pending

### Todo 3: CreateTaskModal 기본 프로젝트 설정
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: Board에서 전달된 defaultProjectId로 프로젝트를 자동 선택
- **Work**:
  - `CreateTaskModalProps`에 `defaultProjectId?: string` 추가
  - `selectedProjectId` useState 초기값을 `defaultProjectId || ""` 사용
  - 모달이 열릴 때(isOpen 변경 시) defaultProjectId로 리셋하는 useEffect 추가
- **Convention Notes**: 기존 props 패턴 유지
- **Verification**: TypeScript 타입 에러 없음
- **Exit Criteria**: 필터 선택 후 "새 작업" 클릭 시 해당 프로젝트가 자동 선택됨
- **Status**: pending

## Verification Strategy
- TypeScript 타입 에러 없음 (`npx tsc --noEmit`)
- 3개 언어 JSON 파싱 오류 없음

## Progress Tracking
- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-13: Plan created
