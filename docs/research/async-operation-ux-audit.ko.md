# KanVibe 모바일 및 데스크톱 비동기 작업 UX 점검

task 삭제를 포함해 대기 상태, 중복 실행, 화면 이탈, 실패 복구에 문제가 있는 지점을 32개 항목으로 정리했다. 공통 문제는 작업 상태가 버튼이나 모달 안에만 있거나, 아예 관리되지 않는다는 점이다. 특히 삭제, 낙관적 상태 변경, 조회 실패를 정상적인 빈 화면으로 처리하는 경로를 먼저 개선해야 한다.

2026-11-13 작업 트리의 React 데스크톱과 Flutter 모바일 코드를 기준으로 확인했다. 아래의 지연·실패·화면 이탈 시나리오는 코드 흐름에 근거한 판단이며, 실제 기기 재현이나 작업 시간 측정 결과는 아니다. 애플리케이션 코드는 변경하지 않았다.

P1은 결과 오인, 파괴적 작업 중복 실행 또는 완료 처리 누락을 먼저 막아야 하는 항목이다. P2는 대기 표시, 오류 안내, 재시도와 조작 일관성을 개선해야 하는 항목이다. 서로 같은 개선 기반을 공유하는 항목도 사용자 진입점이 다르면 별도로 기록했다.

**삭제가 오래 걸릴 수밖에 없는 현재 흐름**

삭제 서비스는 다른 데스크톱으로 삭제를 전파한 뒤 세션·worktree 등의 리소스를 정리하고, 마지막에 DB 항목을 제거한다. 다른 데스크톱의 응답을 기다리는 동안에도 로컬 카드는 남는다. 모바일 삭제 명령의 응답 대기 한도는 90초다. 이는 실제 소요 시간이나 완료 보장 시간이 아니라 클라이언트 대기 한도다. 현재 UI는 이 단계를 보여 주지 않는다. [삭제 서비스](../../src/desktop/main/services/kanbanService.ts#L857), [모바일 명령 시간 제한](../../mobile/lib/src/common/network/desktop_command.dart#L51)

| 우선순위 | 먼저 개선할 흐름 | 관련 항목 |
| --- | --- | --- |
| P1 | task별 삭제 중 상태를 보드·상세에 공유하고 중복 실행을 차단한다 | 01–05 |
| P1 | 실패와 정상적인 빈 데이터, 응답 지연과 실제 데이터 부재를 구분한다 | 17, 31 |
| P1 | 화면을 먼저 바꾸는 작업에 실패 안내와 복구를 연결한다 | 08 |
| P2 | 생성·세션·탭 작업을 화면 이탈 후에도 추적한다 | 06–13 |
| P2 | 조회·설정·외부 명령에 진행 상태와 재시도를 제공한다 | 14–16, 18–30, 32 |

**01 · P1 · 데스크톱 보드에서 task를 삭제해도 카드에 변화가 없다**

우클릭 메뉴와 키보드 삭제 모두 요청을 시작한 뒤 메뉴만 닫는다. `deleteTaskFromBoard`는 응답 후에만 카드를 제거하며, task별 삭제 상태나 중복 실행 가드가 없다. 실패 시에도 콘솔에만 남긴다. 기다리는 동안 같은 카드를 다시 삭제하거나 상세로 들어갈 수 있다.

카드에 `삭제 중…`과 진행 표시를 붙이고 해당 task의 삭제·이동·진입 정책을 통일해야 한다. 실패하면 카드에 오류와 재시도를 남긴다. [삭제 처리](../../src/components/Board.tsx#L651), [메뉴 진입점](../../src/components/Board.tsx#L938), [카드 인터페이스](../../src/components/TaskCard.tsx#L19)

**02 · P2 · 데스크톱 상세 삭제 버튼도 요청 중 상태가 없다**

`DeleteTaskButton`은 확인창 이후 form action만 실행한다. 버튼의 disabled, pending 문구, 오류 영역이 없고, 상세 라우트도 삭제 완료 후 보드로 이동할 뿐 작업 상태를 관리하지 않는다. 처리 중에도 원래의 삭제 버튼과 상세 화면이 남는다.

삭제 중 상태와 중복 제출 차단을 연결하고, 실패 시 상세 화면에 사유를 남겨야 한다. [삭제 버튼](../../src/components/DeleteTaskButton.tsx#L14), [라우트 액션](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1551)

**03 · P1 · 모바일 보드의 길게 눌러 삭제에는 진행 상태가 없다**

보드의 `_TaskCard`는 `ConsumerWidget`이며 삭제 명령을 기다리는 동안 카드를 바꾸거나 입력을 막는 상태가 없다. 상세 시트의 `삭제하는 중…`도 이 진입점에서는 사용하지 않는다. 사용자는 카드가 그대로인 상태에서 다시 길게 누르거나 상세로 들어갈 수 있다.

task ID로 관리하는 삭제 상태를 보드와 상세에서 공유해야 한다. 카드에 진행 표시를 유지하고 삭제 요청을 한 번만 실행한다. [모바일 카드 삭제와 터치 처리](../../mobile/lib/src/features/board/presentation/board_screen.dart#L343)

**04 · P1 · 모바일 상태·삭제·세션 시트를 닫으면 완료 처리가 누락될 수 있다**

상태 시트와 세션 시트는 기본 dismiss 동작을 사용하는 bottom sheet다. 삭제 중 시트를 닫으면 `_delete`가 응답 뒤 `!mounted`에서 반환해 `_finishDeletion`의 보드 갱신과 `TaskDeleted` 전달이 생략된다. 따라서 상세 화면을 보드로 돌려보내는 처리와 다른 데스크톱 삭제 실패 안내도 이 경로에서는 실행되지 않는다. 상태 이동과 세션 변경도 상세로 결과를 돌려주는 동작이 시트의 mounted 여부에 묶여 있어, 시트를 먼저 닫으면 그 결과를 상세의 로컬 상태에 전달하지 못한다.

작업 생명주기를 시트 밖에 두고 결과를 반영해야 한다. 진행 중 닫기를 허용한다면 상세·보드에 동일한 진행 상태를 이어서 보여 준다. [시트 열기와 결과 처리](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L232), [삭제 응답 이후 mounted 검사](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L126), [세션 변경 완료 처리](../../mobile/lib/src/features/task_detail/presentation/widgets/task_session_type_sheet.dart#L61)

**05 · P1 · PR 병합 후 일괄 삭제는 시작하자마자 진행 UI가 사라진다**

`handlePrMergeConfirm`은 선택된 작업을 지우기 전에 `backgroundSyncReview`를 null로 만든다. 컴포넌트가 즉시 null을 렌더링하므로 pending 상태가 있어도 화면에 표시되지 않는다. 삭제 예외를 안내하는 처리도 없다. 서비스는 작업을 순차 삭제하므로 중간 실패 시 일부만 삭제된 상태가 생길 수 있지만 UI는 성공·실패 대상을 요약하지 않는다.

선택한 N개 중 처리한 수와 대상별 결과를 유지하고, 실패한 항목만 다시 시도할 수 있어야 한다. [확인창 닫기와 삭제 호출](../../src/desktop/renderer/components/BackgroundSyncReviewDialog.tsx#L24), [순차 삭제](../../src/desktop/main/services/kanbanService.ts#L931)

**06 · P2 · 데스크톱 task 생성·분기 중 취소나 Escape가 작업 추적을 끊는다**

생성·분기 버튼에는 pending 문구가 있지만 취소 버튼과 Escape는 계속 `onClose`를 실행한다. 닫기는 진행 중인 서버 작업을 취소하지 않는다. 완료 시의 모달 닫기나 생성된 task로 이동하는 로직은 계속 실행될 수 있고, 모달이 사라진 상태의 오류는 사용자가 확인하기 어렵다. 생성 폼의 Enter 처리도 pending 여부 없이 `requestSubmit`을 실행한다.

닫기를 실제 취소와 구분하고, 닫은 뒤에도 진행·결과를 추적해야 한다. 제출 핸들러에서도 중복 제출을 차단한다. [생성 모달](../../src/components/CreateTaskModal.tsx#L138), [Enter 제출](../../src/components/CreateTaskModal.tsx#L188), [분기 모달](../../src/components/BranchTaskModal.tsx#L61)

**07 · P2 · 모바일 task 생성 중 뒤로 가면 결과를 놓친다**

생성 화면은 입력과 만들기 버튼을 비활성화하지만 뒤로 가기를 제어하거나 생성 상태를 화면 밖에 보관하지 않는다. 요청 도중 화면을 나가면 성공 응답 후 `!mounted`에서 반환하여 이 경로의 보드 invalidate와 생성 결과 반환이 생략된다. 데스크톱에서는 task가 만들어져도 사용자는 취소된 것으로 이해할 수 있다.

뒤로 가기의 의미를 안내하고 생성 작업을 앱 상태에서 추적해야 한다. 성공하면 보드를 갱신하고 생성한 task를 열 수 있는 결과 안내를 제공한다. [생성 완료 처리](../../mobile/lib/src/features/task_create/presentation/create_task_screen.dart#L93), [화면 구성](../../mobile/lib/src/features/task_create/presentation/create_task_screen.dart#L151)

**08 · P1 · 데스크톱 보드 이동은 화면만 먼저 바뀌고 실패 복구 안내가 없다**

카드를 드래그하거나 메뉴에서 상태를 바꾸면 `setTasks`와 Done 개수를 먼저 변경한다. 저장 transition의 pending 값을 버리고, renderer에는 실패 catch나 원래 카드 위치로 복구하는 처리가 없다. 서비스의 Done 경로에는 일부 복구가 있지만 일반 이동 실패 전체에 대한 renderer 복구·안내를 대신하지 못한다.

task별 `저장 중` 상태를 표시하고 실패하면 실제 저장 상태로 다시 맞춘 뒤 재시도를 제공해야 한다. [pending 값 미사용](../../src/components/Board.tsx#L366), [낙관적 이동](../../src/components/Board.tsx#L803), [서비스의 오류 처리 범위](../../src/desktop/main/services/kanbanService.ts#L1195)

**09 · P2 · 데스크톱 상세의 상태 이동과 Done 버튼에 진행 상태가 없다**

일반 상태 form과 Done form은 서버 결과를 기다린 뒤 상세 상태를 바꾼다. 처리 중 문구, 제출 차단, 실패 메시지가 없으며 커맨드 팔레트 이동도 같은 결과를 기다리는 UI가 없다. 여러 상태를 연달아 요청할 수 있다.

현재 이동 목표를 표시하고 동일 task의 상태 요청을 직렬화해야 한다. 실패 시 현재 상태를 유지하고 사유를 보여 준다. [상태 액션](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1520), [Done 버튼](../../src/components/DoneStatusButton.tsx#L41), [일반 상태 form](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1701)

**10 · P2 · 모바일 상태 이동·세션 변경은 버튼이 흐려질 뿐 무엇을 하는지 알기 어렵다**

상태 시트는 `_isMoving`, 세션 시트는 `_isChanging`으로 버튼들을 막지만 버튼 문구는 그대로다. 선택한 목표나 진행 표시가 없다. 삭제는 `삭제하는 중…` 문구가 있어 같은 시트 계열에서도 피드백 수준이 다르다.

선택한 버튼에 `Review로 옮기는 중…`, `세션을 준비하는 중…` 등을 표시하고 오류와 재시도를 같은 위치에 유지해야 한다. [상태 버튼](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L202), [세션 버튼](../../mobile/lib/src/features/task_detail/presentation/widgets/task_session_type_sheet.dart#L143)

**11 · P2 · 데스크톱 터미널 탭 추가·이름 변경·순서 변경에 진행·실패 피드백이 없다**

`useTerminalTabs`는 각 명령과 목록 재조회를 기다리지만 mutation 상태를 노출하지 않는다. 탭 바의 추가 버튼 등은 계속 활성화돼 있고, 호출부는 Promise를 `void`로 버린다. 선택은 화면을 먼저 바꾸지만 실패 복구가 없다. 상세의 탭 닫기에는 ref 기반 중복 방지가 있으나 화면에 드러나지 않고, 워크스페이스의 닫기는 hook을 직접 호출한다.

생성 중 임시 탭, 대상 탭의 처리 표시, 오류와 재시도를 제공하고 입력을 작업 단위로 제한해야 한다. [탭 명령 hook](../../src/desktop/renderer/hooks/useTerminalTabs.ts#L152), [추가 버튼](../../src/desktop/renderer/components/TerminalTabBar.tsx#L143), [상세 호출부](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1778), [워크스페이스 호출부](../../src/desktop/renderer/components/WorkspaceTerminalPane.tsx#L51)

**12 · P2 · terminal 세션의 탭 목록을 기다리거나 읽지 못하면 빈 터미널이 보인다**

`TerminalLoader`는 탭 배열이 비어 있으면 빈 div를 반환한다. hook은 목록 오류를 `error`로 제공하지만 상세와 워크스페이스는 이를 표시하지 않는다. 목록 대기, 세션 문제, 조회 실패가 모두 빈 검은 영역으로 보일 수 있다. 이 경우 실제 `Terminal`의 연결 중·오류 UI도 마운트되지 않는다.

탭 목록의 loading, empty, error를 분리하고 실패 영역에 재시도를 제공해야 한다. [빈 배열 처리](../../src/desktop/renderer/components/TerminalLoader.tsx#L31), [목록 조회 실패](../../src/desktop/renderer/hooks/useTerminalTabs.ts#L89), [워크스페이스 표시](../../src/desktop/renderer/components/WorkspaceTerminalPane.tsx#L63)

**13 · P2 · 모바일 탭 닫기 사전 조회에는 진행 표시가 없고 닫는 중 표시도 잘못된다**

닫기 확인을 위한 프로세스 조회는 controller의 `isClosing` 설정 전에 실행된다. 이 대기 구간에는 진행·재진입 방지가 없어 닫기 확인을 중복 요청할 수 있다. 실제 닫기를 시작하면 `isMutating`을 `isCreating`으로 전달하기 때문에 추가 버튼에 스피너와 `터미널 추가 중…` 툴팁이 표시된다.

`닫기 확인 중`, `닫는 중`, `추가 중`을 구분하고 대상 탭에서 상태를 표시해야 한다. [사전 확인](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L141), [공유 mutation 값 전달](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L437), [추가 중으로 표현하는 UI](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L645)

**14 · P2 · 모바일·데스크톱 브랜치 목록에 조회 중·실패·빈 목록 구분이 없다**

데스크톱 생성 모달은 branches 배열만 관리하고 조회 reject를 안내하지 않는다. 분기 모달은 실패를 빈 배열로 바꾼다. 모바일은 `AsyncValue.value`만 읽어 loading과 error를 버린다. 원격 저장소 응답이 늦어도 기본 브랜치만 보이며 다른 브랜치가 없는 것처럼 보인다.

브랜치 입력 옆에 조회 중 표시, 오류와 다시 불러오기를 추가하고 수동 기본값과 서버 목록을 구분해야 한다. [데스크톱 생성 조회](../../src/components/CreateTaskModal.tsx#L119), [데스크톱 분기 조회](../../src/components/BranchTaskModal.tsx#L49), [모바일 목록 소비](../../mobile/lib/src/features/task_create/presentation/create_task_screen.dart#L294)

**15 · P2 · 데스크톱 폴더 탐색 실패를 하위 폴더가 없는 것으로 처리한다**

`FolderSearchInput`은 조회 중 상태는 계산하지만 실패 시 디렉터리를 빈 배열로 저장한다. SSH 연결 실패나 권한 오류를 정상적인 빈 폴더와 구분할 오류 상태·재시도가 없다.

이전 목록을 유지하면서 실패 이유와 재조회 버튼을 보여 줘야 한다. [폴더 조회와 catch](../../src/components/FolderSearchInput.tsx#L43)

**16 · P2 · task 빠른 검색과 프로젝트 작업 목록은 조회 실패가 빈 결과처럼 보인다**

빠른 검색은 조회에 `finally`만 있어 reject 시 로딩만 끝나고 오류 UI가 없다. 프로젝트 작업 모달은 catch에서 콘솔만 기록한다. 처음 열 때 실패하면 task가 없는 것으로 보이고, 기존 데이터를 유지하는 경우에는 갱신 실패를 알 수 없다.

목록 영역에 오류와 재시도를 표시하고 정상적인 검색 결과 없음과 구분해야 한다. [빠른 검색](../../src/desktop/renderer/components/TaskQuickSearchDialog.tsx#L280), [프로젝트 작업 목록](../../src/components/ProjectBranchTasksModal.tsx#L42)

**17 · P1 · 데스크톱 초기 조회 지연·실패를 빈 데이터나 존재하지 않는 task로 표시한다**

보드와 설정은 첫 조회가 5초를 넘거나 실패하면 빈 보드·기본 설정을 넣는다. 상세는 초기 state를 null로 만들어 task 부재 화면으로, Diff도 빈 state의 task 부재 화면으로 간다. 워크스페이스 역시 실패하면 빈 task·project 목록을 만든다. 캐시가 있으면 이전 데이터를 유지하지만 갱신 실패를 알리지 않는다. 아직 기다리는 작업을 데이터 소실이나 설정 초기화로 오인할 수 있다.

`loading`, `stale`, `error`, 실제 `not-found`를 구분해야 한다. 시간 초과는 `조회가 지연되고 있습니다`로 안내하고 이전 데이터와 재시도를 유지한다. [5초 설정](../../src/desktop/renderer/utils/loadingTimeout.ts#L1), [보드](../../src/desktop/renderer/routes/BoardRoute.tsx#L131), [설정](../../src/desktop/renderer/routes/SettingsRoute.tsx#L51), [상세](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1326), [Diff](../../src/desktop/renderer/routes/DiffRoute.tsx#L48), [워크스페이스](../../src/desktop/renderer/routes/TaskWorkspaceRoute.tsx#L63)

**18 · P2 · 커맨드 팔레트 수동 sync는 시작·진행·변경 없음 완료를 알리지 않는다**

sync를 선택하면 팔레트를 닫고 Promise 결과를 사용하지 않는다. 실패 catch도 콘솔만 기록한다. 서버는 변경이나 검토할 내용이 있을 때 결과 이벤트를 보내므로 변경 없는 정상 완료는 별도 피드백이 없다. 서버에는 중복 사이클 공유가 있어 명령을 다시 눌러도 같은 사이클을 기다릴 수 있지만 UI에서는 실행 여부를 알 수 없다.

화면에 sync 진행 상태를 유지하고 `변경 없음`을 포함한 완료 결과와 실패·재시도를 제공해야 한다. [명령 실행](../../src/desktop/renderer/components/CommandPaletteDialog.tsx#L54), [서버 결과 이벤트와 중복 실행 공유](../../src/desktop/main/services/backgroundTaskSyncService.ts#L140)

**19 · P2 · 데스크톱 프로젝트 삭제도 스캔 중으로 표시되고 예외 안내가 없다**

스캔과 프로젝트 삭제가 하나의 `isPending`을 공유한다. 삭제 중에도 스캔 버튼 문구가 `scanning`으로 바뀌고, 삭제 대상 행은 이름 변화 없이 전체 삭제 버튼만 비활성화된다. 두 작업의 transition에는 reject를 error state에 연결하는 catch가 없으며, 작업 중에도 모달을 닫을 수 있다. 정상 스캔 응답의 `result.errors` 표시는 있지만 IPC reject를 대신하지 못한다.

스캔과 삭제 상태를 분리하고 삭제 대상 행에 진행·실패를 표시해야 한다. 닫은 뒤에도 진행 중인 등록·삭제 결과를 확인할 수 있어야 한다. [스캔·삭제 처리](../../src/components/ProjectRegistryDialog.tsx#L55), [공유 pending 문구](../../src/components/ProjectRegistryDialog.tsx#L162), [삭제 행](../../src/components/ProjectRegistryDialog.tsx#L256)

**20 · P2 · 세션 종류 일괄 변경은 긴 순차 처리 동안 버튼이 비활성화될 뿐이다**

호스트별 의존성 확인과 전체 task 변경이 끝날 때까지 버튼은 같은 적용 문구를 유지한다. 마지막에 성공·실패 개수는 보여 주지만 현재 어느 호스트·task를 처리하는지 또는 얼마나 진행됐는지 알 수 없다.

대상 수와 처리 수, 현재 준비·변경 단계를 표시하고 실패 대상을 다시 선택할 수 있어야 한다. [호스트별 확인과 일괄 변경](../../src/components/BulkSessionTypeChangeSettings.tsx#L22), [버튼 표시](../../src/components/BulkSessionTypeChangeSettings.tsx#L80)

**21 · P2 · 설명·우선순위·색상 저장의 대기·실패 피드백이 약하다**

설명과 우선순위는 opacity와 `pointer-events-none`으로만 저장 중을 표현하고 실패 안내가 없다. CSS는 키보드 제출을 막지 못하며 설명 textarea의 Enter 핸들러에는 pending 가드도 없다. 색상은 미리 바꾸고 팝오버를 닫은 뒤 저장하는데 실패 복구가 없다.

명시적인 저장 상태와 오류를 제공하고 실제 disabled 또는 핸들러 가드로 입력을 제어해야 한다. 색상은 실패하면 저장된 값으로 되돌리거나 미저장 상태를 표시한다. [설명 저장과 키보드](../../src/components/TaskDetailTitleCard.tsx#L44), [우선순위](../../src/components/PriorityEditor.tsx#L18), [색상](../../src/components/ProjectColorEditor.tsx#L33)

**22 · P2 · 설정 스위치는 저장 실패해도 바뀐 값이 유지된다**

테마, 사이드바, 기본 세션, 알림, background sync 등의 설정은 로컬 값을 먼저 바꾸고 저장한다. 저장 실패를 사용자에게 알리고 이전 값으로 복구하는 경로가 없다. 일부 설정의 pending 값은 이전 props가 덮어쓰지 못하게 유지하므로 실패한 값이 화면에서 계속 유효한 설정처럼 보일 수 있다. 여러 설정이 공유하는 `isPending`은 어떤 항목을 저장하는지도 드러내지 않는다.

항목별 저장 상태를 제공하고 성공한 값과 미저장 값을 구분해야 한다. 실패 시 복구하거나 재시도를 제공한다. [로컬·pending 상태](../../src/components/ProjectSettings.tsx#L102), [테마 저장](../../src/components/ProjectSettings.tsx#L173), [사이드바 저장](../../src/components/ProjectSettings.tsx#L302), [sync 저장](../../src/components/ProjectSettings.tsx#L447)

**23 · P2 · AI 계정 추가·삭제에 진행 상태가 없고 목록 실패도 숨긴다**

추가 버튼은 이름이 비었는지만 검사하고 요청 중에도 활성화된다. 삭제 역시 확인 후 기다리는 표시와 예외 안내가 없다. 추가 API의 정상 오류 결과는 표시하지만 reject는 처리하지 않는다. 목록 조회 실패는 처음에는 빈 배열로, 이후에는 기존 목록으로 처리한다.

계정별 pending과 실패 상태를 제공하고, 조회 실패와 계정 없음도 구분해야 한다. [계정 추가](../../src/desktop/renderer/routes/AiAccountsRoute.tsx#L107), [목록 조회·삭제](../../src/desktop/renderer/routes/AiAccountsRoute.tsx#L152)

**24 · P2 · 모바일 연결 코드 발급·폐기·기기 해제에 대기·중복 실행 제어가 없다**

데스크톱 설정의 `runPairingAction`은 오류 메시지만 관리한다. 코드 발급·폐기와 기기 해제 버튼은 계속 눌릴 수 있으며 요청 중 표시도 없다. 연결된 기기 목록 조회 실패는 빈 배열로 바뀌어 기존 연결이 없는 것처럼 보일 수 있다.

코드 작업과 기기별 해제 상태를 관리하고 겹치는 작업을 차단해야 한다. 기기 목록의 읽기 실패도 별도로 안내한다. [공통 액션과 목록 조회](../../src/components/MobilePairingSettings.tsx#L46), [버튼과 명령](../../src/components/MobilePairingSettings.tsx#L143)

**25 · P2 · 데스크톱 알림 목록·읽음 처리·알림 열기에 진행 상태가 없다**

초기 알림 조회에는 loading·error 관리가 없고 읽음 처리, 전체 읽음 처리도 기다리는 표시와 오류 안내가 없다. 알림 클릭은 읽음 저장 후 패널을 닫고 task 조회·이동을 기다리므로 느리거나 실패하면 클릭 결과를 이해하기 어렵다. 모바일 알림 화면은 별도의 opening·updating 표시를 갖고 있어 플랫폼 간 차이도 있다.

목록 조회와 각 액션의 진행 상태를 분리하고, 알림 열기는 대상 행에서 추적해야 한다. [목록 조회](../../src/components/NotificationCenterButton.tsx#L146), [읽음 처리와 이동](../../src/components/NotificationCenterButton.tsx#L213), [전체 읽음](../../src/components/NotificationCenterButton.tsx#L340)

**26 · P2 · 모바일 보드 당겨서 새로고침이 실제 조회를 기다리지 않는다**

보드의 `RefreshIndicator.onRefresh`는 provider를 invalidate한 직후 Future가 완료된다. 데이터를 받기 전에 새로고침 표시가 끝날 수 있다. 모바일 대화 기록 화면은 invalidate 이후 `provider.future`를 기다려 구현도 일관되지 않다.

보드도 실제 조회 완료까지 기다리고 기존 카드들을 유지하면서 실패·재시도를 알려야 한다. [보드 새로고침](../../mobile/lib/src/features/board/presentation/board_screen.dart#L142), [대화 기록의 대기 처리](../../mobile/lib/src/features/chat_history/presentation/chat_history_screen.dart#L175)

**27 · P2 · PR 찾기·VS Code 열기는 기다리는 표시가 없고 PR 오류가 부재로 바뀐다**

상세 dock에서 PR URL 조회나 VS Code 실행을 기다릴 때 버튼에 진행 상태나 재진입 가드가 없다. PR 서비스는 조회 오류를 로그 후 null로 바꾸고, UI는 이를 `pullRequestNotFound`로 안내한다. 네트워크·CLI 오류와 실제 PR 부재를 구분하기 어렵다.

`PR 조회 중`, `편집기 여는 중`을 표시하고, PR 조회 실패와 PR 없음의 결과를 분리해야 한다. [외부 명령 UI](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L843), [PR 서비스 오류 처리](../../src/desktop/main/services/kanbanService.ts#L1241)

**28 · P2 · 원격 터미널 이미지 붙여넣기는 전송 중 피드백이 없다**

클립보드 이미지는 FileReader로 읽은 뒤 원격으로 전송하고 완료 후 경로를 터미널에 쓴다. 이 사이에는 전송 표시가 없어 붙여넣기가 무시된 것처럼 보인다. 정상 응답의 실패는 터미널에 쓰지만 Promise reject를 처리하는 catch도 없다.

파일명이나 전송 대기 항목을 표시하고 전송 실패·재시도를 제공해야 한다. 원격에 전달되는 경로 입력의 순서도 명확히 유지한다. [원격 이미지 전송](../../src/desktop/renderer/components/Terminal.tsx#L142)

**29 · P2 · Diff 저장 중 단축키로 중복 저장할 수 있고 실패가 자동으로 사라진다**

저장 버튼은 saving 상태에서 disabled지만 Monaco의 저장 단축키는 `handleSave`를 직접 부른다. 핸들러에는 진행 중 가드가 없어 키보드로 저장을 반복할 수 있다. 실패 시 사유를 보존하지 않고 일반 실패 문구를 3초 뒤 idle로 돌린다.

공유 저장 가드로 버튼과 단축키를 함께 제어해야 한다. 실패 상태와 원인을 사용자가 재시도할 때까지 유지하고 미저장 변경도 구분한다. [저장·실패 상태](../../src/components/DiffFileEditor.tsx#L38), [단축키](../../src/components/DiffFileEditor.tsx#L57)

**30 · P2 · hook 설치의 Promise reject에는 사용자 오류 메시지가 없다**

hook 설치는 정상 반환 결과의 성공·실패와 설치 중 상태를 표시한다. 그러나 `runInstall`은 try/finally만 있어 IPC 등에서 Promise가 reject되면 설치 중 상태만 해제하고 메시지를 남기지 않는다. 설치에 실패했는지 완료됐는지 다시 상태를 확인해야 한다.

구조화된 설치 실패 결과와 별도로 예외 경로도 메시지로 연결하고 재시도를 제공해야 한다. [상태 카드 설치](../../src/components/HooksStatusCard.tsx#L154), [설치 다이얼로그](../../src/components/HooksStatusDialog.tsx#L184)

**31 · P1 · 모바일 명령 시간 초과를 작업 실패로만 안내해 결과가 불명확하다**

모바일 command 채널은 응답 제한 시간이 지나면 pending request를 제거한다. 서버 취소나 결과 재조회는 이 경로에 없고, 늦게 오는 command-result도 기다리는 Promise와 연결되지 않는다. 삭제는 이를 일반 연결 실패·재시도 문구로 바꾼다. 생성 등의 HTTP 요청도 클라이언트 timeout만으로 서버 작업이 취소됐다고 보장하지 못한다. 요청이 실행된 뒤 응답만 늦거나 끊긴 경우에는 성공 여부를 모르는 상태다.

명확한 서버 실패와 `결과 확인 중`을 구분해야 한다. 재연결·시간 초과 뒤에는 보드나 task 상태를 재조회하고 필요하면 operation ID로 결과를 확인한 뒤 재시도를 안내한다. 실제로 시간 초과가 발생했다는 측정 결과는 아니다. [command 응답 처리](../../mobile/lib/src/common/network/desktop_client.dart#L1187), [timeout 이후 pending 제거](../../mobile/lib/src/common/network/desktop_client.dart#L1213), [삭제 오류 변환](../../mobile/lib/src/features/board/application/task_deletion.dart#L52), [생성 HTTP 요청 제한](../../mobile/lib/src/common/network/desktop_client.dart#L783)

**32 · P2 · 완료 task 더 불러오기는 화면에 따라 진행·오류 피드백이 다르다**

칸반 보드는 loading 가드와 finally는 있지만 오류 메시지를 설정하는 catch가 없다. 워크스페이스는 loading 상태 없이 요청하고 실패는 콘솔에만 남겨 같은 페이지를 반복 요청할 수 있다. 지연이나 실패가 있어도 더 불러올 항목이 없는 것인지 구분하기 어렵다.

두 보기에 동일한 loading·실패·재시도 상태를 제공하고 같은 페이지의 중복 요청을 막아야 한다. [칸반 더 불러오기](../../src/components/Board.tsx#L778), [워크스페이스 더 불러오기](../../src/desktop/renderer/routes/TaskWorkspaceRoute.tsx#L121)

**공통 개선 기준**

1. 작업을 시작한 즉시 대상 카드·행·버튼에 작업 종류와 진행 중 상태를 표시한다. task의 업무 상태인 Todo·Progress·Done과 삭제·저장 같은 요청 상태는 별도로 관리한다.
2. 동일 대상의 중복 실행은 핸들러와 상태 저장소에서 막는다. CSS로 클릭만 막는 것으로 키보드와 다른 진입점을 제어하지 않는다.
3. 오래 걸리는 작업의 상태는 모달이나 시트보다 오래 유지한다. 화면을 닫는 동작이 취소인지 백그라운드 실행인지 명확히 한다.
4. 여러 단계나 여러 대상을 처리하는 작업에는 실제 확인 가능한 단계·처리 수를 표시한다. 근거 없는 퍼센트나 남은 시간을 만들지 않는다.
5. 조회 중, 정상적인 빈 결과, 실패, 오래된 데이터, 결과 확인 중을 구분한다. 실패를 빈 목록·기본 설정·task 없음으로 치환하지 않는다.
6. 실패 메시지는 작업 대상과 함께 유지하고 복구·재시도를 제공한다. 일부 성공한 작업은 성공 대상과 실패 대상을 나눠 알려 준다.
7. 낙관적으로 바꾼 화면은 저장 성공과 구분하며 실패 시 실제 상태로 복구한다. 보조기술에도 busy·status·alert 상태를 전달한다.

**재현 확인에 사용할 시나리오**

각 작업의 응답을 보류한 상태에서 화면 표시, 키보드·터치 중복 입력, 다른 진입점 실행을 확인한다. 그 상태에서 모달·시트·화면을 닫은 뒤 성공·실패 응답을 각각 반환해 전역 결과 반영을 확인한다. 삭제는 peer 응답 지연과 부분 실패, 조회는 5초를 넘는 지연과 명시적 오류, 모바일은 명령 timeout 이후 늦은 성공 응답을 따로 검증한다. 이 시나리오는 후속 QA 기준이며 이번 점검에서 실행한 테스트 목록이 아니다.
