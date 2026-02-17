# Done 상태 이동 시 리소스 삭제 경고

## Business Goal
태스크를 Done으로 이동하면 worktree, branch, tmux session이 삭제되는데, 사용자가 이를 인지하지 못하고 실수로 리소스를 잃는 것을 방지하기 위해 확인 다이얼로그를 표시한다.

## Scope
- **In Scope**: 칸반 D&D와 상세 페이지에서 Done 이동 시 confirm 경고 추가, 3개 언어 번역
- **Out of Scope**: 커스텀 모달 UI, 다른 상태 전환 경고

## Codebase Analysis Summary

Done 상태 전환 시 `cleanupTaskResources()`가 호출되어 session(tmux/zellij), worktree, branch를 삭제한다.
두 경로 존재: Board.tsx `handleDragEnd` → `moveTaskToColumn`, 상세 페이지 `handleStatusChange` → `updateTaskStatus`.
기존 삭제 경고는 `confirm()` 네이티브 다이얼로그 사용 (`DeleteTaskButton`, `handleDeleteFromCard`).

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/Board.tsx` | 칸반 보드 D&D 핸들러 | Modify |
| `src/app/[locale]/task/[id]/page.tsx` | 상세 페이지 상태 변경 | Modify |
| `src/components/DoneStatusButton.tsx` | Done 버튼 클라이언트 래퍼 (신규) | Create |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 확인 다이얼로그 | `DeleteTaskButton.tsx` | `confirm()` 네이티브 사용 |
| i18n 키 추가 | `messages/*.json` | 3개 언어 동시 추가 |
| 클라이언트 컴포넌트 | `DeleteTaskButton.tsx` | `"use client"` + `useTranslations` |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 다이얼로그 타입 | `confirm()` | 기존 삭제 패턴 동일 | 커스텀 모달 |
| 상세 페이지 구현 | `DoneStatusButton` 클라이언트 컴포넌트 | `DeleteTaskButton` 패턴과 동일 | 페이지 전체 client 전환 |
| 경고 조건 | 리소스(branch/session) 존재 시만 | 불필요한 UX 방해 방지 | 항상 경고 |

## Implementation Todos

### Todo 1: 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Done 이동 경고 메시지 번역 키를 3개 언어에 추가
- **Work**:
  - `messages/ko.json`의 `board` 네임스페이스에 `doneConfirm` 키 추가
  - `messages/ko.json`의 `taskDetail` 네임스페이스에 `doneConfirm` 키 추가
  - `messages/en.json`, `messages/zh.json`에 동일 키 추가
  - 메시지 내용: "Done으로 이동하면 worktree, branch, tmux session이 삭제됩니다. 계속하시겠습니까?"
- **Convention Notes**: 3개 언어 동시 수정
- **Verification**: JSON 파싱 오류 없음 확인
- **Exit Criteria**: 3개 언어 파일에 `doneConfirm` 키 존재
- **Status**: pending

### Todo 2: Board.tsx D&D 경고 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 칸반 드래그 앤 드롭으로 Done 이동 시 confirm 표시
- **Work**:
  - `Board.tsx`의 `handleDragEnd`에서 `destStatus === TaskStatus.DONE`이고 태스크에 리소스(branchName 또는 sessionType)가 있을 때 `confirm()` 호출
  - 취소 시 `return` (카드가 원래 위치로 복귀)
  - 번역은 `useTranslations("board")`의 `doneConfirm` 사용
- **Convention Notes**: 기존 `handleDragEnd` 구조 유지, 최소 변경
- **Verification**: 빌드 성공
- **Exit Criteria**: D&D로 Done 이동 시 리소스 있는 태스크에 confirm 표시
- **Status**: pending

### Todo 3: DoneStatusButton 컴포넌트 생성 + 상세 페이지 적용
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 상세 페이지 Done 버튼에 confirm 경고 추가
- **Work**:
  - `src/components/DoneStatusButton.tsx` 생성 (클라이언트 컴포넌트)
  - `DeleteTaskButton` 패턴 참고: props로 `statusChangeAction`, `label`, `hasResources` 수신
  - `hasResources`가 true일 때만 `onSubmit`에서 `confirm()` 호출
  - `src/app/[locale]/task/[id]/page.tsx`에서 Done 전환 버튼만 `DoneStatusButton`으로 교체
- **Convention Notes**: `"use client"`, `useTranslations("taskDetail")` 패턴 준수
- **Verification**: 빌드 성공
- **Exit Criteria**: 상세 페이지 Done 버튼 클릭 시 리소스 있으면 confirm 표시
- **Status**: pending

## Verification Strategy
- `pnpm build` 성공
- 3개 언어 JSON 유효성
- Board D&D / 상세 페이지 양쪽 경로에서 경고 동작 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-02-17: Plan created
- 2026-02-17: All todos completed, TypeScript + JSON validation passed
