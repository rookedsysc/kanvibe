# fzf 스타일 폴더 검색 UI

## Business Goal
스캔할 폴더 경로를 수동 입력하는 대신, fzf 스타일의 fuzzy finder UI를 제공하여 홈 디렉토리 하위의 git 저장소를 빠르게 검색하고 키보드로 선택할 수 있게 한다. 사용자 경험을 개선하여 경로 오타를 방지하고 탐색 효율을 높인다.

## Scope
- **In Scope**:
  - 홈 디렉토리(~) 하위 git repo 목록 조회 Server Action
  - fzf 스타일 fuzzy finder 컴포넌트 (타이핑 필터, 키보드 내비게이션, Enter 선택)
  - ProjectSettings의 scanPath 입력부를 fzf 컴포넌트로 교체
  - SSH 호스트 지원 (원격 git repo 목록 조회)
  - i18n 번역 키 추가 (ko, en, zh)
- **Out of Scope**:
  - 스캔 로직 자체 변경
  - 디렉토리 생성/삭제 기능
  - 일반 디렉토리 표시 (git repo만)

## Codebase Analysis Summary
- `ProjectSettings.tsx`: 프로젝트 관리 패널. `<input name="scanPath">`로 경로 수동 입력 → `scanAndRegisterProjects` 호출
- `src/app/actions/project.ts`: Server Action 모음. `scanAndRegisterProjects`가 `scanGitRepos` 호출
- `src/lib/gitOperations.ts`: `scanGitRepos(rootPath, sshHost?)` — `find` 명령으로 `.git` 디렉토리 탐색, SSH 지원
- `execGit(command, sshHost?)` — 로컬/원격 명령 실행 추상화

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `src/app/actions/project.ts` | Server Action 모음 | Modify — `listGitRepos` 액션 추가 |
| `src/components/FolderSearchInput.tsx` | fzf 스타일 fuzzy finder 컴포넌트 | Create |
| `src/components/ProjectSettings.tsx` | 프로젝트 설정 패널 | Modify — FolderSearchInput으로 교체 |
| `src/lib/gitOperations.ts` | Git 관련 유틸리티 | Reference (scanGitRepos 재사용) |
| `messages/ko.json` | 한국어 번역 | Modify — 새 키 추가 |
| `messages/en.json` | 영어 번역 | Modify — 새 키 추가 |
| `messages/zh.json` | 중국어 번역 | Modify — 새 키 추가 |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| "use client" 지시어 | 기존 컴포넌트 | 클라이언트 컴포넌트에 필수 |
| useTranslations 훅 | i18n 패턴 | 모든 UI 텍스트는 번역 키 사용 |
| Tailwind 디자인 토큰 | globals.css | `bg-bg-*`, `text-text-*`, `border-border-*` 클래스 사용 |
| Server Action + "use server" | project.ts | 서버 액션 패턴 유지 |
| 한국어 주석 | CODE_PRINCIPLES.md | 주석은 한국어로 작성 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| git repo 목록 조회 | 기존 `scanGitRepos` 재사용 | DRY, 이미 SSH 지원 | 별도 find 명령 구현 |
| Fuzzy matching | 프론트엔드 JS 필터 | 즉각 반응, 서버 부하 없음 | fuse.js 라이브러리 |
| 컴포넌트 분리 | `FolderSearchInput` 별도 파일 | SRP, 재사용 가능성 | ProjectSettings 내 인라인 |
| 드롭다운 위치 | 입력창 아래 absolute | 기존 UI 레이아웃 호환 | Modal/Portal |

## Implementation Todos

### Todo 1: Server Action — listGitRepos 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 홈 디렉토리 하위 git repo 경로 목록을 반환하는 서버 액션 생성
- **Work**:
  - `src/app/actions/project.ts`에 `listGitRepos(sshHost?: string): Promise<string[]>` 추가
  - 내부에서 `scanGitRepos("~", sshHost)` 호출하여 결과 반환
  - `"use server"` 디렉티브 확인 (이미 파일 상단에 있음)
- **Convention Notes**: 기존 Server Action 패턴과 동일하게 export async function
- **Verification**: Server Action이 정상적으로 git repo 경로 배열을 반환하는지 확인
- **Exit Criteria**: `listGitRepos()` 호출 시 `~` 하위 git repo 경로 배열 반환
- **Status**: pending

### Todo 2: FolderSearchInput 컴포넌트 생성
- **Priority**: 1
- **Dependencies**: none
- **Goal**: fzf 스타일 fuzzy finder 입력 컴포넌트 구현
- **Work**:
  - `src/components/FolderSearchInput.tsx` 생성 (클라이언트 컴포넌트)
  - Props: `{ onSelect: (path: string) => void; sshHost?: string; name: string; placeholder?: string }`
  - 상태 관리:
    - `query`: 검색 입력값
    - `repos`: 서버에서 가져온 전체 git repo 목록
    - `filteredRepos`: fuzzy 필터링된 결과
    - `selectedIndex`: 현재 선택된 항목 인덱스
    - `isOpen`: 드롭다운 표시 여부
    - `isLoading`: 로딩 상태
  - 마운트 시 (또는 input focus 시) `listGitRepos(sshHost)` 호출하여 repo 목록 캐싱
  - 입력값 변경 시 fuzzy matching으로 필터링:
    - 쿼리 문자열을 소문자로 변환
    - 경로의 각 문자가 쿼리의 순서대로 포함되는지 확인 (fzf 스타일 subsequence matching)
    - 매칭 점수 기반 정렬 (연속 매칭 우선, 경로 끝부분 매칭 우선)
  - 키보드 이벤트 처리:
    - `ArrowDown`: selectedIndex 증가
    - `ArrowUp`: selectedIndex 감소
    - `Enter`: 현재 선택 항목 확정 → `onSelect(path)` 호출, 드롭다운 닫기
    - `Escape`: 드롭다운 닫기
  - 드롭다운 UI:
    - 입력창 하단에 absolute 위치
    - 최대 높이 제한 + 스크롤
    - 선택된 항목 하이라이트 (bg-brand-primary/10)
    - 매칭된 문자 하이라이트 (text-brand-primary font-bold)
  - 외부 클릭 시 드롭다운 닫기 (useRef + useEffect)
  - 선택 완료 후 입력창에 선택된 경로 표시
  - hidden input으로 form에 값 전달 (`name` prop 활용)
- **Convention Notes**:
  - "use client" 지시어 필수
  - Tailwind 디자인 토큰 사용 (bg-bg-page, text-text-primary, border-border-default 등)
  - 한국어 주석
- **Verification**: 컴포넌트 렌더링 확인, 키보드 내비게이션 동작 확인
- **Exit Criteria**: fuzzy 검색 → 화살표 키 내비게이션 → Enter 선택이 모두 동작
- **Status**: pending

### Todo 3: ProjectSettings 컴포넌트에 FolderSearchInput 통합
- **Priority**: 2
- **Dependencies**: Todo 1, Todo 2
- **Goal**: 기존 scanPath 텍스트 입력을 FolderSearchInput으로 교체
- **Work**:
  - `src/components/ProjectSettings.tsx`에서 `<input name="scanPath" .../>` 제거
  - `FolderSearchInput` import 및 사용
  - `sshHost` 값을 FolderSearchInput에 전달 (SSH select 변경 시 연동)
  - SSH host select를 controlled component로 변경하여 `sshHost` 상태 관리
  - `handleScan` 함수는 기존대로 formData에서 scanPath 읽음 (hidden input으로 전달됨)
- **Convention Notes**: 기존 form action 패턴 유지
- **Verification**: 설정 패널에서 폴더 검색 → 선택 → 스캔 전체 플로우 동작 확인
- **Exit Criteria**: fzf로 폴더 선택 후 스캔 버튼 클릭 시 정상 등록
- **Status**: pending

### Todo 4: i18n 번역 키 추가
- **Priority**: 1
- **Dependencies**: none
- **Goal**: FolderSearchInput 관련 UI 텍스트 번역 키 추가
- **Work**:
  - `messages/ko.json`의 `settings` 네임스페이스에 추가:
    - `"searchReposPlaceholder"`: "폴더명을 입력하여 검색..."
    - `"loadingRepos"`: "git 저장소 검색 중..."
    - `"noReposFound"`: "일치하는 저장소가 없습니다."
    - `"repoCount"`: "{count}개의 저장소"
  - `messages/en.json`에 동일 키 영어 번역 추가
  - `messages/zh.json`에 동일 키 중국어 번역 추가
- **Convention Notes**: 기존 번역 파일 구조와 동일하게 settings 네임스페이스 사용
- **Verification**: 각 locale에서 번역 키가 정상 출력되는지 확인
- **Exit Criteria**: ko, en, zh 세 언어 모두 번역 키 존재
- **Status**: pending

## Verification Strategy
- `npm run build`로 빌드 오류 없음 확인
- 브라우저에서 설정 패널 열기 → FolderSearchInput에 문자 입력 → 드롭다운 표시 확인
- 키보드 화살표 + Enter로 선택 가능한지 확인
- 선택 후 "스캔 및 등록" 버튼으로 정상 스캔되는지 확인
- SSH host 변경 시 원격 repo 목록으로 갱신되는지 확인

## Progress Tracking
- Total Todos: 4
- Completed: 4
- Status: Execution complete

## Change Log
- 2026-02-15: Plan created
- 2026-02-15: All todos executed and verified (build pass)
