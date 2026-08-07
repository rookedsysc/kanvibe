# 보드 정렬 순서 필터 + 우선순위 상속

## 원본 요청

> https://github.com/timelabs-inc/timelabs-open-design 이 repository 참조해서 정렬순서 필터 붙여줘.
> 필요하면 clone해도 좋아. 정렬순서 앱 껏다켜도 유지되게 해줘.
> project의 우선순위를 task에서도 상속받도록 해줘(기본값).

## 승인된 결정 (사용자 응답)

| 항목 | 결정 |
| --- | --- |
| 정렬 UI | 다중 키 스택 정렬 — 참조 저장소 `filters/task-sort.html` / `app.js:1123-1240`와 동일 구조 |
| 상속 출처 | **프로젝트 root task**(default branch를 가진 task)의 우선순위를 같은 프로젝트의 다른 task가 상속 |
| 수동 순서 | `display_order`(int)를 **hexorank로 완전 이관하고 컬럼 삭제** |
| 모드 | 수동 우선 / 정렬 우선 / 수동 순서 끔 — 3택 + 각 모드 설명문 |
| 애니메이션 | 정렬 우선 모드에서 드롭 위치가 정렬로 되밀릴 때 자연스러운 리플로우 전환 |

## 확정한 해석 (미지정 부분 — 이 가정으로 진행)

1. **자리(`display_rank`)와 의도(`is_manually_ordered`)를 나눠 담는다.**
   - `display_rank`(varchar, NOT NULL): 카드가 놓인 자리. 기존 `display_order` 순서를 그대로 옮겨 담는다.
   - `is_manually_ordered`(boolean, 기본 false): 사용자가 직접 드래그해 배치했는지. "기본값인 애들"이 여기서 갈린다.

   rank를 nullable로 두고 `NULL`을 "기본값"으로 쓰는 안을 먼저 검토했으나, 그러면 드롭 지점의 앞뒤 이웃이
   모두 `NULL`일 때 사이 값을 만들 수 없어 카드가 엉뚱한 자리로 튄다. 두 컬럼으로 나누면 모든 이웃이
   항상 유효한 rank를 갖고, 마이그레이션이 기존 순서를 100% 보존하면서도 기존 카드가 전부 "기본값"으로
   남아 정렬 필터가 첫 실행부터 전체에 적용된다.
2. **정렬 기준이 하나도 없으면 세 모드가 동일하게 동작한다** — rank ASC → (rank 없는 것) createdAt ASC.
   현재 화면과 동일하므로 기능을 켜지 않은 사용자에게 회귀가 없다.
3. **"아예 끔" = 수동 순서(rank)를 무시**하고 정렬 기준만 사용. 이 모드에서는 같은 컬럼 내 재정렬 DnD를
   비활성화한다(컬럼 간 상태 이동은 유지).
4. 정렬은 **컬럼별로 독립 적용**된다.
5. 상속된 우선순위는 TaskCard 배지에 구분되는 스타일로 표시한다. 태스크가 자기 우선순위를 가지면 그 값이 이긴다.

## 정렬 모드 정의

| 모드 | 정렬 규칙 | 재정렬 DnD |
| --- | --- | --- |
| `manual-first` (수동 순서 우선) | 직접 배치한 태스크가 rank ASC로 먼저, 그 뒤 나머지가 정렬 기준 순 | 허용 |
| `sort-first` (정렬 기준 우선, 기본값) | 정렬 기준 전체 적용 → 동점 시 rank → createdAt | 허용 (드롭 후 정렬 위치로 리플로우) |
| `manual-off` (수동 순서 사용 안 함) | rank 무시, 정렬 기준만 → 동점 시 createdAt | 차단 |

정렬 기준이 하나도 없으면 세 모드 모두 rank ASC로 같은 화면을 그린다.

## 정렬 기준 필드

`우선순위` / `생성일` / `수정일` / `제목` / `프로젝트`

- 값이 없는 항목(우선순위 미지정, 프로젝트 없음)은 방향과 무관하게 **항상 뒤로** (참조 저장소 `SORT_VALUE` 주석과 동일 규칙)
- 우선순위는 상속 해석 후의 **유효 우선순위**로 비교

## 설계 배치표

| 새 심볼 | 배치 | 재사용 검토(실행한 검색) | 가시성 | 근거 |
| --- | --- | --- | --- | --- |
| `rankBetween`, `compareDisplayRank` | `src/desktop/shared/displayRank.ts` (신규) | `rg -n "displayOrder\|display_order" src electron` — 기존 순서 유틸 없음 | export | main(kanbanService)·renderer(정렬) 양쪽에서 필요. `src/desktop/shared/keyboardShortcut.ts` 선례 |
| `BoardSortKey`, `BoardSortMode`, `parseBoardSortPreference`, `serializeBoardSortPreference` | `src/desktop/shared/boardSort.ts` (신규) | `rg -n "TASK_KIND_FILTER_VALUES"` — 필터 상수는 훅 안에 있으나 main과 공유 안 함 | export | 설정 직렬화 포맷을 main·renderer가 공유 |
| `getBoardSortPreference` / `setBoardSortPreference` | `src/desktop/main/services/appSettingsService.ts` (기존 파일) | 동 파일의 `getNotificationSettings` 패턴 재사용 | export | 앱 전역 설정은 AppSettings 계층 (CLAUDE.md) |
| `useBoardSortPreference` | `src/desktop/renderer/hooks/useBoardSortPreference.ts` (신규) | `useProjectFilterParams`/`useTaskKindFilterParams` 확인 — 둘 다 sessionStorage라 재사용 불가 | export | 재시작 유지 요구 + CLAUDE.md의 sessionStorage 금지 조항 |
| `sortTasksForBoard`, `resolveEffectivePriority`, `buildProjectRootPriorityMap` | `src/desktop/renderer/utils/boardTaskSort.ts` (신규) | `rg -n "isProjectRootTask" src` — Board.tsx 내부 지역 함수 → **이 파일로 이동해 재사용** | export | 순수 로직, `boardFocusTarget.ts` 선례 |
| `useFlipReflow` | `src/desktop/renderer/hooks/useFlipReflow.ts` (신규) | `rg -n "transition\|animate" src/components/Column.tsx` — 없음 | export | Column 전용이나 훅 계층이 이미 있음 |
| `BoardSortPicker` | `src/components/BoardSortPicker.tsx` (신규) | `ProjectSelector`(팝오버 다중선택) 검토 — 정렬 순위/방향 개념이 없어 재사용 불가 | default export | 보드 헤더 컨트롤, 기존 컴포넌트와 동일 위치 |

## 아키텍처 규약 (관측된 것)

- 엔티티(TypeORM) → main 서비스 → `serviceRegistry` IPC → `renderer/actions` 래퍼 → 컴포넌트. 새 코드도 이 층을 따른다.
- 스키마는 **TypeORM 마이그레이션(`src/migrations/`)과 raw SQLite 빌더(`src/lib/sqliteSchema.ts`) 양쪽**에 반영해야 한다.
- 색상은 시맨틱 토큰만 사용(`--color-brand-*`, `--color-button-neutral-*`). hex 직접 사용 금지.

## 의존 티어와 작업

### Tier 1 (병렬 가능)

1. **T1-A hexorank 유틸** — `src/desktop/shared/displayRank.ts`
   - `rankBetween(prev: string | null, next: string | null): string` — 16진 fractional ranking
   - `compareDisplayRank(a, b)` — null은 항상 뒤
2. **T1-B 정렬 설정 공유 타입** — `src/desktop/shared/boardSort.ts`
3. **T1-C 스키마 이관**
   - `src/entities/KanbanTask.ts`: `displayOrder: number` → `displayRank: string` + `isManuallyOrdered: boolean`
   - `src/migrations/1771700000000-ReplaceDisplayOrderWithDisplayRank.ts`: 컬럼 추가 → status별 `display_order, created_at` 순서로 rank 채움 → `display_order` 삭제 → 인덱스 `(status, display_rank, created_at)` 재생성. `down()`은 rank 순서를 정수 순번으로 되돌린다
   - `src/lib/sqliteSchema.ts`: CREATE TABLE / `ensureColumns` / `ensureIndexes` 갱신 + **`backfillDisplayRankFromDisplayOrder()`** — migrations 테이블이 없어 baseline 처리되는 오래된 DB는 TypeORM 마이그레이션이 실행되지 않으므로 여기서도 순서를 옮겨야 카드 배치가 사라지지 않는다

### Tier 2 (Tier 1 의존)

4. **T2-A kanbanService rank 전환**
   - 조회 3곳: `ORDER BY display_rank IS NULL ASC, display_rank ASC, created_at ASC` (QueryBuilder)
   - `createTask`: `MAX(displayOrder)` 조회 제거, rank는 `null`로 둔다
   - `reorderTasks(status, movedTaskId, orderedIds)` / `moveTaskToColumn(taskId, status, movedTaskId 포함 destOrderedIds)`:
     이동한 태스크 **1행만** 인접 non-null rank 사이 값으로 갱신
   - `src/desktop/renderer/actions/kanban.ts` 시그니처 동기화
5. **T2-B AppSettings 정렬 설정** — `appSettingsService` + `renderer/actions/appSettings.ts` + `useBoardSortPreference`
   - 키: `board_sort_preference`, 값: `{"keys":[{"key":"priority","dir":"asc"}],"mode":"sort-first"}`
   - 파싱 실패/알 수 없는 키는 기본값으로 폴백

### Tier 3 (Tier 2 의존)

6. **T3-A 정렬·상속 로직** — `boardTaskSort.ts` (`isProjectRootTask`를 Board.tsx에서 이동)
7. **T3-B 정렬 피커 UI** — `BoardSortPicker.tsx` + i18n 3개 로케일
8. **T3-C 보드 통합** — Board.tsx 헤더 트리거/배지, 정렬 적용, `manual-off`에서 재정렬 차단, TaskCard 상속 배지, Column 리플로우 애니메이션

## 테스트 케이스 계획

| # | 설계 기법 | 대상 | 입력 | 기대 결과 | 우선순위 |
| --- | --- | --- | --- | --- | --- |
| 1 | 경계값 | `rankBetween` | `(null, null)` / `(null, "8")` / `("8", null)` / `("8", "9")` | 항상 `prev < 결과 < next`인 문자열 | P0 |
| 2 | 반복 삽입 | `rankBetween` | 같은 두 값 사이에 50회 연속 삽입 | 매번 사전순 단조 증가, 충돌 0 | P0 |
| 3 | 동등 분할 | `compareDisplayRank` | `null` 포함 배열 정렬 | null이 전부 뒤로 | P1 |
| 4 | 상태 전이 | `reorderTasks` | 5개 컬럼에서 3번째를 1번째로 | 대상 1행만 UPDATE, 조회 순서가 요청 순서와 일치 | P0 |
| 5 | 회귀 | `createTask` | 신규 태스크 생성 | `displayRank === null`, Todo 컬럼 맨 뒤에 조회됨 | P0 |
| 6 | 왕복 | `get/setBoardSortPreference` | 키 2개 + 모드 저장 후 재조회 | 동일 값 복원 | P0 |
| 7 | 오류 처리 | `parseBoardSortPreference` | 깨진 JSON / 알 수 없는 필드 키 / 잘못된 모드 | 기본값(빈 키, `sort-first`) | P1 |
| 8 | 다중 키 | `sortTasksForBoard` | `우선순위 asc → 제목 asc` | 우선순위 동점일 때만 제목이 개입 | P0 |
| 9 | 방향 | `sortTasksForBoard` | 같은 키 `desc` | 순서 반전, **단 null 항목은 여전히 뒤** | P0 |
| 10 | 모드 분기 | `sortTasksForBoard` | rank 있는 2개 + 없는 3개, 세 모드 각각 | 표의 모드 정의와 정확히 일치 | P0 |
| 11 | 상속 | `resolveEffectivePriority` | task.priority=null, root task=HIGH | HIGH 반환 | P0 |
| 12 | 상속 오버라이드 | `resolveEffectivePriority` | task.priority=LOW, root task=HIGH | LOW 반환 | P0 |
| 13 | 상속 부재 | `resolveEffectivePriority` | root task 없음 / 프로젝트 없음 | null 반환 | P1 |
| 14 | 상태 전이 | `BoardSortPicker` | 기준 클릭 → 재클릭 → 방향 토글 → 정렬 해제 | 추가·제거·방향 반전·전체 비움 | P0 |
| 15 | UI 상태 | `BoardSortPicker` | 모드 3개 전환 | 각 모드 설명문이 바뀌어 노출 | P1 |
| 16 | 통합 | `Board` | 우선순위 정렬 활성 상태로 렌더 | 카드 DOM 순서가 정렬 결과와 일치 | P0 |
| 17 | 회귀 | `Board` | 정렬 기준 0개 | rank 순서 그대로(기존 동작 유지) | P0 |
| 18 | 통합 | `TaskCard` | 상속된 우선순위 | 상속 배지 렌더 + 직접 지정과 구분 가능 | P1 |

## QA 명령 매트릭스

| 게이트 | 명령 |
| --- | --- |
| narrow | `pnpm exec vitest run src/desktop/shared/__tests__/displayRank.test.ts src/desktop/renderer/utils/__tests__/boardTaskSort.test.ts` |
| feature | `pnpm exec vitest run src/components/__tests__/BoardSortPicker.test.tsx src/components/__tests__/Board.test.tsx src/desktop/main/services/__tests__/kanbanService.test.ts src/desktop/main/services/__tests__/appSettingsService.test.ts` |
| full | `pnpm check && pnpm lint && pnpm test` |

## 위험과 정지 조건

- **되돌릴 수 없는 스키마 변경**: `display_order` 컬럼 삭제. 마이그레이션 `down()`에 역변환(rank → 순번 int)을 구현하고, `up()`은 값 채우기 후에만 컬럼을 삭제한다.
- **FLIP 애니메이션이 `@hello-pangea/dnd`의 transform과 충돌**할 수 있다. 드래그 중에는 리플로우 전환을 끄고, 충돌이 해소되지 않으면 애니메이션만 분리해 보고한다(정렬 기능 자체는 차단하지 않는다).
- 범위 밖: 프로젝트 상세/설정의 우선순위 UI 신설, TaskDetail의 상속 안내 문구, 정렬 기준의 서버측(main) 적용.
