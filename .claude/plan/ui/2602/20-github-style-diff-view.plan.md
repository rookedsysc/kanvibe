# GitHub Style Diff View

## Business Goal
Task detail 페이지에서 Git diff를 GitHub 스타일로 시각화하고, 변경된 파일을 Monaco Editor로 직접 편집할 수 있는 기능을 제공한다. 이를 통해 사용자가 터미널 없이도 코드 변경 사항을 확인하고 수정할 수 있다.

## Scope
- **In Scope**:
  - Task detail 사이드바 Actions 카드에 "Diff 보기" 링크 추가
  - 새 페이지 `/[locale]/task/[id]/diff` 생성
  - Git diff 생성 Server Action
  - 파일 읽기/쓰기 Server Action
  - 변경 파일 트리 사이드바 컴포넌트 (폴더 구조 포함)
  - Monaco Diff Editor로 diff 표시 (syntax highlighting 자동 지원)
  - Monaco Editor로 파일 전체 편집 + 저장
  - i18n 번역 키 추가 (ko, en, zh)
- **Out of Scope**:
  - Commit/Push 기능
  - PR 생성 연동
  - 실시간 파일 변경 감지 (WebSocket)
  - SSH 원격 서버의 diff

## Codebase Analysis Summary
- Task detail 페이지는 `src/app/[locale]/task/[id]/page.tsx`에 위치하며 서버 컴포넌트
- 사이드바(CollapsibleSidebar) + 메인(터미널) 2-column 레이아웃
- `KanbanTask` 엔티티에 `worktreePath`, `branchName`, `baseBranch` 필드 존재
- 기존 `execAsync`(child_process) 패턴으로 shell 명령어 실행 가능
- Server Action 패턴은 `src/app/actions/kanban.ts`에 일관되게 사용

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/[locale]/task/[id]/page.tsx` | Task detail 페이지 | Modify - Diff 링크 추가 |
| `src/app/[locale]/task/[id]/diff/page.tsx` | Diff 뷰 페이지 | Create |
| `src/app/actions/diff.ts` | Git diff / 파일 I/O Server Actions | Create |
| `src/components/DiffFileTree.tsx` | 변경 파일 트리 사이드바 | Create |
| `src/components/DiffMonacoViewer.tsx` | Monaco Diff Editor 래퍼 | Create |
| `src/components/DiffFileEditor.tsx` | Monaco Editor 파일 편집 | Create |
| `messages/ko.json` | 한국어 번역 | Modify |
| `messages/en.json` | 영어 번역 | Modify |
| `messages/zh.json` | 중국어 번역 | Modify |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Server Action 패턴 | `src/app/actions/kanban.ts` | `"use server"` + async function export |
| i18n 네임스페이스 | `messages/*.json` | 새 네임스페이스 `diffView` 추가 |
| CSS 토큰 | `CLAUDE.md` | Tailwind + CSS 변수 토큰 사용 (`bg-bg-surface`, `text-text-primary` 등) |
| 컴포넌트 | `src/components/` | `"use client"` 클라이언트 컴포넌트, PascalCase 파일명 |
| 페이지 | `src/app/[locale]/` | 서버 컴포넌트 기본, `getTranslations`으로 번역 |
| 한국어 주석 | `CODE_PRINCIPLES.md` | 주석은 한국어, 서술형으로 작성 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| 에디터 | Monaco Editor | Diff뷰 + 편집 통합, 100+ 언어 syntax highlighting | CodeMirror, react-diff-view |
| 패키지 | `@monaco-editor/react` | React 래퍼, Next.js SSR 호환, dynamic import 지원 | monaco-editor 직접 사용 |
| Diff 생성 | Server Action + execAsync | 기존 패턴 일관성 | API Route |
| 파일 I/O | Server Action + fs | 보안상 서버에서만 파일 접근 | API Route |
| 파일 트리 | 커스텀 트리 컴포넌트 | 간단한 구조, 외부 의존성 불필요 | react-treeview |
| Diff 파싱 | 자체 파서 | git diff --stat + git diff 출력 직접 파싱 | diff2html |

## Implementation Todos

### Todo 1: @monaco-editor/react 패키지 설치
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Monaco Editor React 래퍼 패키지를 설치한다
- **Work**:
  - `pnpm add @monaco-editor/react` 실행
- **Convention Notes**: pnpm 패키지 매니저 사용
- **Verification**: `pnpm list @monaco-editor/react` 확인
- **Exit Criteria**: package.json에 `@monaco-editor/react` 추가됨
- **Status**: pending

### Todo 2: Git diff Server Actions 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Git diff 생성, 파일 읽기, 파일 쓰기를 위한 Server Actions를 구현한다
- **Work**:
  - `src/app/actions/diff.ts` 파일 생성
  - `getGitDiffFiles(taskId: string)`: task의 worktreePath에서 `git diff baseBranch...branchName --name-status` 실행하여 변경 파일 목록 반환
  - `getGitDiffContent(taskId: string, filePath: string)`: 특정 파일의 diff 내용 (old/new 전체 파일) 반환
  - `getFileContent(taskId: string, filePath: string)`: 현재 브랜치의 파일 전체 내용 읽기
  - `saveFileContent(taskId: string, filePath: string, content: string)`: 파일 내용 저장
  - `getOriginalFileContent(taskId: string, filePath: string)`: baseBranch의 원본 파일 내용 반환 (`git show baseBranch:filePath`)
  - 보안: worktreePath 외부 경로 접근 방지 (path traversal 차단)
- **Convention Notes**: 기존 `execAsync` 패턴 사용, `"use server"` 상단 선언, 에러 핸들링 try/catch
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: 모든 Server Action이 올바른 타입 시그니처로 export됨
- **Status**: pending

### Todo 3: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: diffView 네임스페이스에 필요한 번역 키를 3개 언어에 추가한다
- **Work**:
  - `messages/ko.json`에 `diffView` 네임스페이스 추가:
    - `title`: "코드 변경 사항"
    - `backToTask`: "작업으로 돌아가기"
    - `changedFiles`: "변경된 파일"
    - `noChanges`: "변경 사항이 없습니다"
    - `editMode`: "편집 모드"
    - `diffMode`: "비교 모드"
    - `save`: "저장"
    - `saving`: "저장 중..."
    - `saved`: "저장 완료"
    - `saveError`: "저장 실패"
    - `noBranch`: "브랜치가 연결되지 않았습니다"
    - `noWorktree`: "워크트리 경로가 설정되지 않았습니다"
    - `added`: "추가"
    - `modified`: "수정"
    - `deleted`: "삭제"
    - `renamed`: "이름 변경"
    - `viewDiff`: "Diff 보기"
  - `messages/en.json`에 동일 키 영어 번역 추가
  - `messages/zh.json`에 동일 키 중국어 번역 추가
- **Convention Notes**: 기존 네임스페이스 구조 따름, 키는 camelCase
- **Verification**: JSON 파싱 오류 없음 (3개 파일 모두)
- **Exit Criteria**: 3개 언어 파일에 diffView 네임스페이스가 동일한 키 구조로 존재
- **Status**: pending

### Todo 4: DiffFileTree 컴포넌트 생성
- **Priority**: 2
- **Dependencies**: Todo 2, Todo 3
- **Goal**: 변경된 파일을 폴더 구조로 보여주는 트리 사이드바 컴포넌트를 만든다
- **Work**:
  - `src/components/DiffFileTree.tsx` 생성 (클라이언트 컴포넌트)
  - Props: `files: DiffFile[]`, `selectedFile: string | null`, `onSelectFile: (path: string) => void`
  - `DiffFile` 타입: `{ path: string; status: 'added' | 'modified' | 'deleted' | 'renamed' }`
  - 파일 경로를 폴더 구조로 트리 변환 (중첩 객체)
  - 폴더 열기/닫기 토글
  - 파일 상태별 아이콘/색상 (added=초록, modified=노랑, deleted=빨강)
  - 선택된 파일 하이라이트
- **Convention Notes**: Tailwind CSS 토큰 사용, 한국어 주석
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: DiffFileTree 컴포넌트가 파일 목록을 트리 형태로 렌더링
- **Status**: pending

### Todo 5: DiffMonacoViewer 컴포넌트 생성
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Monaco DiffEditor를 사용한 diff 뷰어 컴포넌트를 만든다
- **Work**:
  - `src/components/DiffMonacoViewer.tsx` 생성 (클라이언트 컴포넌트)
  - `next/dynamic`으로 SSR 비활성화하여 import
  - Props: `originalContent: string`, `modifiedContent: string`, `filePath: string`, `language?: string`
  - 파일 확장자에서 언어 자동 감지 (`.ts` → typescript, `.py` → python 등)
  - Monaco DiffEditor 설정: readOnly, renderSideBySide, minimap 끔
- **Convention Notes**: dynamic import 패턴 사용, Tailwind CSS 토큰
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: DiffMonacoViewer가 two-pane diff를 렌더링
- **Status**: pending

### Todo 6: DiffFileEditor 컴포넌트 생성
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Monaco Editor를 사용한 파일 편집기 컴포넌트를 만든다
- **Work**:
  - `src/components/DiffFileEditor.tsx` 생성 (클라이언트 컴포넌트)
  - `next/dynamic`으로 SSR 비활성화하여 import
  - Props: `content: string`, `filePath: string`, `onSave: (content: string) => Promise<void>`, `language?: string`
  - 파일 확장자에서 언어 자동 감지
  - 저장 버튼 + Ctrl/Cmd+S 단축키
  - 저장 상태 표시 (idle/saving/saved/error)
- **Convention Notes**: dynamic import 패턴 사용, Tailwind CSS 토큰
- **Verification**: TypeScript 컴파일 오류 없음
- **Exit Criteria**: DiffFileEditor가 파일 편집 + 저장 기능 제공
- **Status**: pending

### Todo 7: Diff 페이지 생성
- **Priority**: 3
- **Dependencies**: Todo 2, Todo 3, Todo 4, Todo 5, Todo 6
- **Goal**: `/[locale]/task/[id]/diff` 페이지를 생성하여 모든 컴포넌트를 조합한다
- **Work**:
  - `src/app/[locale]/task/[id]/diff/page.tsx` 생성
  - 서버 컴포넌트: task 정보 조회, diff 파일 목록 조회
  - 레이아웃: 왼쪽 사이드바(DiffFileTree) + 오른쪽 메인(Diff/Editor 뷰)
  - 클라이언트 래퍼 컴포넌트(`DiffPageClient.tsx`)로 상태 관리:
    - 선택된 파일 상태
    - diff 모드 / 편집 모드 전환
    - 파일 선택 시 Server Action 호출하여 내용 로드
  - 상단에 "작업으로 돌아가기" 링크
  - 브랜치 정보가 없는 경우 안내 메시지 표시
  - `generateMetadata`로 페이지 타이틀 설정
- **Convention Notes**: 서버/클라이언트 컴포넌트 분리, i18n 번역 사용, Tailwind CSS 토큰
- **Verification**: `pnpm check` (TypeScript 타입 체크) 통과
- **Exit Criteria**: diff 페이지가 정상 렌더링되고, 파일 트리 + diff 뷰 + 편집 기능 동작
- **Status**: pending

### Todo 8: Task Detail 페이지에 Diff 링크 추가
- **Priority**: 3
- **Dependencies**: Todo 3, Todo 7
- **Goal**: Task detail 사이드바 Actions 카드에 "Diff 보기" 링크를 추가한다
- **Work**:
  - `src/app/[locale]/task/[id]/page.tsx` 수정
  - Actions 카드의 상태 전환 버튼 영역 아래에 "Diff 보기" 링크 추가
  - `Link` 컴포넌트로 `/task/${task.id}/diff`로 이동
  - `branchName`이 있을 때만 링크 표시
  - diff 아이콘(코드 비교 아이콘) + 번역 텍스트 사용
- **Convention Notes**: 기존 버튼 스타일 패턴 따름, `Link` from `@/i18n/navigation`
- **Verification**: `pnpm check` 통과
- **Exit Criteria**: branchName이 있는 태스크에서 "Diff 보기" 링크가 노출되고 클릭 시 diff 페이지로 이동
- **Status**: pending

## Verification Strategy
- `pnpm check`: TypeScript 타입 체크 통과
- `pnpm build`: Next.js 빌드 성공
- 수동 검증: 브랜치가 있는 태스크에서 Diff 보기 → 파일 트리 + diff 뷰 + 편집 동작 확인

## Progress Tracking
- Total Todos: 8
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-02-20: Plan created
