# Resolve Fix Build Merge Conflicts

## Business Goal
`fix/build` PR을 최신 `dev`와 병합 가능하게 만들고, 양쪽 브랜치에 최근 추가된 기능을 최대한 보존한다.

## Scope
- **In Scope**: `origin/dev`를 `fix/build`에 병합, 충돌 파일 해소, 관련 테스트/빌드 검증, 해결 결과 push
- **Out of Scope**: 기존 기능 리디자인, 충돌과 무관한 리팩터링, PR 내용 재작성

## Codebase Analysis Summary
현재 브랜치는 패키징 런타임 수정, 초기 로딩 복구, 단축키/다이얼로그 개선을 포함한다. `dev`의 최신 변경과 충돌하는 파일을 실제 merge 결과 기준으로 확인하고, 동일 기능 영역에서는 양쪽 변경을 병합해 보존한다.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| merge conflict files | `dev`와 `fix/build` 양쪽 변경이 겹치는 파일 | Modify |
| `package.json` | build/test script와 dependency contract | Modify if conflicted |
| `electron/main.js` | Electron window/shortcut behavior | Modify if conflicted |
| `src/**` | renderer UI, routes, tests | Modify if conflicted |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| 변경 보존 | 사용자 요청 | 최근 추가 기능은 가능한 한 모두 유지한다 |
| 최소 수정 | CODE_PRINCIPLES.md | 충돌 해소에 필요한 범위만 수정한다 |
| 파일 수정 | developer instruction | 수동 편집은 `apply_patch`를 사용한다 |
| 검증 | task-planner execution protocol | 테스트와 빌드로 병합 결과를 확인한다 |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| merge target | `origin/dev` into `fix/build` | PR head branch가 base 최신 변경을 포함해야 GitHub 충돌이 해소된다 | rebase는 기존 PR 커밋 히스토리를 바꾸므로 선택하지 않는다 |
| conflict policy | preserve both compatible changes | 사용자 요청이 최근 기능 보존이므로 삭제보다 통합을 우선한다 | 한쪽 변경 선택 |

## Implementation Todos

### Todo 1: 최신 dev 병합 및 충돌 파악
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 실제 충돌 파일과 충돌 내용을 확인한다.
- **Work**:
  - `git fetch origin dev`로 base 최신 상태를 가져온다.
  - `git merge origin/dev`를 실행해 충돌을 재현한다.
  - `git status --short`와 conflict marker 검색으로 충돌 파일을 확인한다.
- **Convention Notes**: 작업 전 working tree가 clean인지 확인한다.
- **Verification**: `git status --short`
- **Exit Criteria**: 충돌 파일 목록과 해소 대상이 명확하다.
- **Status**: pending

### Todo 2: 충돌 해소
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: `dev`와 `fix/build`의 최근 기능을 모두 살리는 방향으로 파일 충돌을 해소한다.
- **Work**:
  - 각 충돌 파일에서 양쪽 변경 의도를 비교한다.
  - 호환 가능한 변경은 병합하고, 중복 또는 대체 관계는 최신 기능을 해치지 않는 형태로 정리한다.
  - conflict marker가 남지 않도록 확인한다.
- **Convention Notes**: 불필요한 리팩터링 없이 충돌 구간 주변만 수정한다.
- **Verification**: `rg '<<<<<<<|=======|>>>>>>>'`
- **Exit Criteria**: 모든 conflict marker가 제거되고 git index에 해결 파일을 stage할 수 있다.
- **Status**: pending

### Todo 3: 검증 및 push
- **Priority**: 3
- **Dependencies**: Todo 2
- **Goal**: 병합 결과가 타입/테스트/빌드 관점에서 안전한지 확인하고 PR 브랜치를 업데이트한다.
- **Work**:
  - 필요한 focused test를 실행한다.
  - 가능한 전체 검증(`pnpm check`, `pnpm test`, `pnpm build`)을 실행한다.
  - merge commit을 만들고 `origin/fix/build`로 push한다.
- **Convention Notes**: 검증 실패 시 원인 파일만 최소 수정한다.
- **Verification**: `pnpm check`, `pnpm test`, `pnpm build`
- **Exit Criteria**: 검증 결과가 정리되고 원격 PR 브랜치가 업데이트된다.
- **Status**: pending

## Verification Strategy
- `rg '<<<<<<<|=======|>>>>>>>'`로 conflict marker 제거 확인
- `pnpm check`로 타입 안정성 확인
- `pnpm test`로 회귀 테스트 확인
- `pnpm build`로 production build 확인

## Progress Tracking
- Total Todos: 3
- Completed: 0
- Status: Planning complete

## Change Log
- 2026-05-01: Plan created
