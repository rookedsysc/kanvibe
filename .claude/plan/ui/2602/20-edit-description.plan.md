# Edit Description on Task Detail Page

## Business Goal
게시물 상세 페이지에서 작업 설명(description)을 인라인으로 수정할 수 있도록 하여, 사용자가 별도 폼 없이 빠르게 설명을 추가/변경할 수 있게 한다.

## Scope
- **In Scope**: description 클릭 시 textarea 전환, 저장/취소 버튼, Ctrl+Enter 저장, Escape 취소, 설명 없을 때 "설명 추가" 플레이스홀더, i18n 3개 언어 번역 키
- **Out of Scope**: 마크다운 렌더링, 리치 텍스트 에디터, title 수정

## Codebase Analysis Summary
- `TaskDetailTitleCard.tsx`: 현재 description을 읽기 전용 `<p>`로 표시. `"use client"` 이미 적용된 클라이언트 컴포넌트.
- `src/app/actions/kanban.ts`: `updateTask(taskId, { description })` 함수 존재.
- `PriorityEditor.tsx`: `useTransition` + `updateTask` + `router.refresh()` 인라인 수정 패턴.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/components/TaskDetailTitleCard.tsx` | description 표시 및 수정 UI | Modify |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |
| `src/app/actions/kanban.ts` | `updateTask` server action | Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 인라인 수정 패턴 | `PriorityEditor.tsx` | `useTransition` + `updateTask` + `router.refresh()` |
| 디자인 토큰 | `globals.css` | Tailwind CSS 변수 사용 (`bg-bg-surface`, `text-text-secondary` 등) |
| i18n | CLAUDE.md | 3개 언어 동시 추가, `useTranslations` 사용 |

## Implementation Todos

### Todo 1: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: description 수정 UI에 필요한 번역 키를 3개 언어 파일에 추가
- **Work**:
  - `messages/ko.json`의 `taskDetail` 네임스페이스에 추가: `editDescription`, `addDescription`, `save`, `cancel`
  - `messages/en.json`의 `taskDetail` 네임스페이스에 동일 키 추가
  - `messages/zh.json`의 `taskDetail` 네임스페이스에 동일 키 추가
- **Convention Notes**: 기존 `taskDetail` 네임스페이스 내 키 위치에 알파벳순 삽입
- **Verification**: JSON 파싱 에러 없음 확인
- **Exit Criteria**: 3개 언어 파일에 동일 키가 존재
- **Status**: pending

### Todo 2: TaskDetailTitleCard에 description 인라인 수정 기능 구현
- **Priority**: 1
- **Dependencies**: none
- **Goal**: description 클릭 시 textarea로 전환되는 인라인 수정 UI 구현
- **Work**:
  - `TaskDetailTitleCard.tsx`에 `isEditing` 상태 추가
  - `updateTask` import, `useRouter`, `useTransition`, `useTranslations` 사용
  - description `<p>` 클릭 시 `isEditing: true` 전환
  - 편집 모드: `<textarea>` + 저장/취소 버튼 렌더링
  - 저장: `updateTask(taskId, { description })` 호출 후 `router.refresh()`
  - 취소: `isEditing: false`로 복원
  - Ctrl+Enter로 저장, Escape로 취소 키보드 핸들링
  - description이 없을 때 "설명 추가" 클릭 가능한 플레이스홀더 표시
  - `isPending` 상태에서 opacity 처리 (PriorityEditor 패턴)
  - Props에 `taskId: string` 추가 필요
- **Convention Notes**: 디자인 토큰 CSS 변수 사용, 한국어 주석
- **Verification**: 클릭 → textarea 전환, 저장/취소 동작, 키보드 단축키 확인
- **Exit Criteria**: description 수정 후 페이지 새로고침 시 변경된 내용 유지
- **Status**: pending

### Todo 3: TaskDetailPage에서 taskId prop 전달
- **Priority**: 2
- **Dependencies**: Todo 2
- **Goal**: TaskDetailTitleCard에 taskId를 전달하여 updateTask 호출 가능하게 함
- **Work**:
  - `src/app/[locale]/task/[id]/page.tsx`에서 `<TaskDetailTitleCard task={task} />` → `<TaskDetailTitleCard task={task} taskId={task.id} />` 변경
- **Convention Notes**: 기존 page.tsx 패턴 유지
- **Verification**: TypeScript 타입 에러 없음
- **Exit Criteria**: TaskDetailTitleCard에 taskId prop이 정상 전달됨
- **Status**: pending

## Verification Strategy
- `pnpm build`로 타입 에러 및 빌드 에러 없음 확인
- 브라우저에서 수동 테스트: description 클릭 → 수정 → 저장 → 새로고침 후 반영 확인

## Progress Tracking
- Total Todos: 3
- Completed: 3
- Status: Execution complete

## Change Log
- 2026-02-20: Plan created
- 2026-02-20: All todos completed, TypeScript type check passed
