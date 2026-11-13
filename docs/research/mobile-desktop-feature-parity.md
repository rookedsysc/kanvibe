# 데스크톱 기준 모바일 기능 비교

이 문서는 구현 전 기준 조사다. 이후46개 기능의 구현·검증 상태는 [구현 보고서](mobile-parity-implementation.md)에서 확인할 수 있다.


모바일은 태스크 생성·상태 변경·삭제, 원격 터미널, 프로젝트 등록·필터, 채팅 History, 알림함을 이미 지원한다. 남은 주요 격차는 보드 탐색, 태스크 정보 수정, Git 변경 검토, AI 모니터링·계정 관리, 터미널 구성과 PC 설정 제어다. 특히 Done 목록의 추가 조회가 없어 모바일 보드에서는 최근 20개까지만 탐색할 수 있다.

기준은 2026-11-13의 커밋 `20af2db9a71da0884a3cbe3111917c5d3c192025`다. 데스크톱 보드·설정, 데스크톱 태스크·터미널, 모바일 전체를 세 서브에이전트가 병렬 조사하고 결과를 교차 확인했다. 아래 판정은 실제 화면·핸들러·서비스와 모바일 전송 경로에 대한 정적 코드 조사이며, 앱 실행 성공이나 실기기 동작을 검증한 결과는 아니다.

- **미구현**: 해당 사용자 동작을 위한 모바일 UI·클라이언트 연결이 없다.
- **부분 구현**: 같은 기능의 일부 경로는 있지만 데스크톱의 나머지 동작이 없다.
- **표시·접근 차이**: 데이터나 동작은 다른 화면에서 지원하지만 보드의 요약 표시·바로가기가 없다.
- **PC 운영·플랫폼 차이**: 아래의 별도 표에 기록했다. 모바일에 동일하게 이식해야 한다는 뜻은 아니다.

터미널에서 CLI를 직접 실행하면 할 수 있는 일과 앱이 전용 화면·API로 제공하는 기능은 구분했다. 같은 기능이 보드·상세에 반복 노출되는 경우에는 하나의 항목으로 묶었다.

## 보드에서 찾고 탐색하는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| B01 | 전체 태스크 빠른 검색 | 제목·프로젝트·브랜치 fuzzy 검색과 결과 이동. 프로젝트 선택창의 검색과는 별개다. | 미구현. [Desktop 전체 검색](../../src/desktop/renderer/components/TaskQuickSearchDialog.tsx#L145), [Mobile 보드 액션](../../mobile/lib/src/features/board/presentation/board_screen.dart#L83) |
| B02 | 현재 보드 안의 텍스트 찾기 | 일치 항목의 이전·다음 이동. 전체 태스크 검색과 달리 현재 화면이 대상이다. | 미구현. [Desktop 찾기](../../src/components/BoardPageFindBar.tsx#L11), [Mobile 보드](../../mobile/lib/src/features/board/presentation/board_screen.dart#L136) |
| B03 | 태스크 종류 필터 | 전체·프로젝트 본체·일반 태스크를 골라 보기. 일반 태스크는 프로젝트 root 외의 모든 태스크다. 프로젝트별 필터는 이미 있다. | 미구현. [Desktop 종류 필터](../../src/components/Board.tsx#L961), [Mobile 프로젝트 필터만 적용](../../mobile/lib/src/features/board/data/board_snapshot.dart#L25) |
| B04 | 여러 기준의 정렬과 설정 저장 | 우선순위·생성일·수정일·제목·프로젝트의 복수 기준과 방향 선택. 모바일은 서버의 최근 수정순을 사용한다. | 미구현. [Desktop 정렬](../../src/components/BoardSortPicker.tsx#L64), [정렬 기준](../../src/desktop/shared/boardSort.ts#L8), [Mobile 순서](../../mobile/lib/src/features/board/data/board_snapshot.dart#L7) |
| B05 | Done 더보기와 전체 개수 | 최초 20개 이후의 완료 태스크 탐색, 전체 개수 표시. 모바일 모델은 응답의 `doneTotal`·`doneLimit`을 보존하지 않는다. 알림의 ID를 통한 개별 조회는 가능하다. | 부분 구현. [Desktop 더보기](../../src/components/Board.tsx#L764), [20개 조회 제한](../../src/desktop/main/services/kanbanService.ts#L425), [Mobile 파싱](../../mobile/lib/src/features/board/data/board_snapshot.dart#L61) |
| B06 | 카드 우선순위와 상속 우선순위 | 카드의 P1/P2/P3 및 프로젝트 root 우선순위의 상속 표시. 생성 시 우선순위 입력과 상세에서 값 읽기는 이미 있다. | 표시 차이. [Desktop 우선순위](../../src/components/TaskCardContent.tsx#L125), [Mobile 카드](../../mobile/lib/src/features/board/presentation/board_screen.dart#L445) |
| B07 | 카드의 추가 요약 정보 | 설명 미리보기·에이전트·세션 종류·Base 표식·프로젝트 아이콘. 모바일 카드에는 제목·프로젝트·브랜치·SSH·PR·미읽음 배지가 있다. | 표시 차이. [Desktop 요약](../../src/components/TaskCardContent.tsx#L140), [Mobile 배지](../../mobile/lib/src/features/board/presentation/board_screen.dart#L477) |
| B08 | 카드의 PR 바로 열기 | 보드 PR 배지 클릭으로 브라우저 열기. 모바일 PR 배지는 표시용이며 상세 메뉴에서 링크를 열 수 있다. | 접근 차이. [Desktop PR 배지](../../src/components/TaskCardContent.tsx#L194), [Mobile PR 배지](../../mobile/lib/src/features/board/presentation/board_screen.dart#L505) |
| B09 | Workspace 보드 | 왼쪽 태스크 목록과 오른쪽 터미널을 함께 보고 즉시 태스크 전환. 모바일은 태블릿에서도 보드→상세 이동 구조다. | 미구현. [Desktop Workspace](../../src/desktop/renderer/routes/TaskWorkspaceRoute.tsx#L138), [뷰 선택](../../src/desktop/renderer/routes/BoardViewRoute.tsx#L45), [Mobile 보드](../../mobile/lib/src/features/board/presentation/board_screen.dart#L136) |

## 태스크를 생성하고 정보를 바꾸는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| T01 | 생성 시 세션 종류 선택 | 새 태스크에서 tmux·Zellij·Herdr·terminal 종류 선택. 모바일 생성은 PC 기본값을 사용한다. 생성 후 개별 전환은 이미 있다. | 부분 구현. [Desktop 생성 옵션](../../src/components/CreateTaskModal.tsx#L322), [Mobile 생성 입력](../../mobile/lib/src/features/task_create/presentation/create_task_screen.dart#L103), [서버 기본값 적용](../../src/desktop/main/services/mobileBridgeService.ts#L705) |
| T02 | 기존 태스크 설명 편집 | 생성 후 설명 변경·저장. 생성 시 설명 입력 및 상세 조회는 지원한다. | 부분 구현. [Desktop 편집](../../src/components/TaskDetailTitleCard.tsx#L34), [Mobile 읽기 전용 설명](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L50) |
| T03 | 기존 태스크 우선순위 변경 | 생성 후 우선순위 변경·해제. 생성 입력과 정보 조회는 지원한다. | 부분 구현. [Desktop 우선순위 편집](../../src/components/PriorityEditor.tsx#L20), [Mobile 정보 행](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L48) |
| T04 | 프로젝트 색상 변경 | 태스크 정보에서 프로젝트 색상 편집·저장. 모바일에는 PC 색상의 표시와 기본 팔레트만 있다. | 부분 구현. [Desktop 색상 편집](../../src/components/TaskDetailInfoCard.tsx#L73), [색상 저장 서비스](../../src/desktop/main/services/kanbanService.ts#L637), [Mobile 정보](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L37) |
| T05 | 관련 태스크 바로가기 | 제목 옆 다른 작업 버튼으로 같은 프로젝트 목록 열기, 기준 브랜치에 해당하는 태스크 바로 열기. 모바일 정보에는 텍스트만 표시한다. | 미구현. [Desktop 프로젝트 작업 목록](../../src/components/TaskDetailTitleCard.tsx#L80), [기준 브랜치 링크](../../src/components/TaskDetailInfoCard.tsx#L47), [Mobile 텍스트](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L38) |
| T06 | 생성일·수정일 표시 | 태스크 날짜를 상세 정보에서 확인. 모바일 태스크 모델에도 날짜 필드가 없다. | 표시 차이. [Desktop 날짜](../../src/components/TaskDetailInfoCard.tsx#L126), [Mobile 모델](../../mobile/lib/src/features/board/domain/board_task.dart#L24) |
| T07 | 수동 Pending 이동 | Desktop 보드 우클릭·명령 팔레트에서는 Pending을 고를 수 있다. 모바일은 Todo·Progress·Review·Done만 제공한다. 자동 Pending 갱신은 지원한다. | 부분 구현. [Desktop 전체 상태](../../src/components/Board.tsx#L61), [명령 팔레트](../../src/desktop/renderer/components/CommandPaletteDialog.tsx#L83), [Mobile 선택 상태](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L17) |
| T08 | Done 이동 안내와 다시 묻지 않기 | Done 이동 때 worktree·세션 등 리소스 보존을 알리고 안내 숨김을 저장. 모바일은 즉시 상태를 바꾼다. | 부분 구현. [Desktop 안내](../../src/components/DoneConfirmDialog.tsx#L14), [Mobile 상태 요청](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L65) |
| T09 | 저장 URL이 없을 때 PR 탐색 | Desktop은 링크가 없어도 PR 액션을 표시하고 GitHub CLI로 URL을 찾아 연다. 저장된 URL이 있으면 바로 연다. 모바일은 저장된 URL을 여는 기능만 있다. | 부분 구현. [Desktop PR 조회](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L939), [Mobile 조건부 메뉴](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L320), [Mobile 링크 열기](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L251) |
| T10 | 기존 태스크를 브랜치 작업으로 전환 | 브랜치 없는 기존 태스크에 프로젝트·base branch·새 브랜치·worktree·세션을 연결하고 같은 ID를 Progress로 옮기기. 모바일의 후속 태스크 생성은 새 ID를 만드는 별도 기능이다. | 미구현. [Desktop 전환 메뉴](../../src/components/TaskContextMenu.tsx#L183), [전환 요청](../../src/components/BranchTaskModal.tsx#L78), [동일 태스크 갱신](../../src/desktop/main/services/kanbanService.ts#L967), [Mobile 새 태스크 생성](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L74) |

## Git 변경을 검토하는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| G01 | 변경량 요약 | 보드 카드·상세의 변경 파일 수, 추가·삭제 줄, 파일 목록과 변경 비율. | 미구현. [Desktop 통계](../../src/components/TaskDiffStats.tsx#L123), [Git 조회](../../src/desktop/main/services/diffService.ts#L223), [Mobile 누락 명시](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L8) |
| G02 | Git Diff 화면 | 변경 파일 트리, A/M/D/R와 줄 수, 원본·현재 내용 비교, 파일별 탐색. | 미구현. [Desktop Diff](../../src/desktop/renderer/components/DiffPageClient.tsx#L197), [Mobile 상세 액션](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L280), [Mobile API 범위](../../electron/mobileRoutes.js#L75) |
| G03 | Diff 파일 편집·저장 | worktree 파일을 에디터에서 수정하고 실제 파일에 저장. | 미구현. [Desktop 저장](../../src/desktop/renderer/components/DiffPageClient.tsx#L111), [실제 파일 저장](../../src/desktop/main/services/diffService.ts#L276), [Mobile 클라이언트](../../mobile/lib/src/common/network/desktop_client.dart#L138) |
| G04 | 파일별 Viewed 체크 | Diff에서 검토한 파일 표시. Desktop도 화면의 로컬 상태이며 영속적인 검토 기록은 아니다. | 미구현. [Desktop Viewed](../../src/desktop/renderer/components/DiffPageClient.tsx#L129), [Mobile 상세 액션](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L280) |

## AI 작업을 관찰하고 기록을 읽는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| A01 | 실행 중 AI 세션 모니터링 | 보드의 세션 수·미리보기, 상세 Live AI 패널, 실행 중 하위 작업 확인과 해당 터미널 포커스. History와 상태 hook 알림은 이미 지원한다. | 미구현. [Desktop 카드](../../src/components/TaskCardLiveSessions.tsx#L37), [Live 패널 연결](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1774), [Mobile History](../../mobile/lib/src/features/chat_history/presentation/chat_history_screen.dart#L39) |
| A02 | Agent 호출 그래프·타임라인 | 에이전트·하위 에이전트 관계와 실행 시간 시각화. | 미구현. [Desktop 그래프](../../src/desktop/renderer/components/AgentCallGraphPanel.tsx#L73), [Mobile 상세 액션](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L280) |
| A03 | 채팅 메시지 역할 필터 | User·Assistant·Tool 등 역할별 메시지 보기. 모바일 서버는 역할 인자를 받지만 앱에서 연결하지 않는다. | 부분 구현. [Desktop 역할 선택](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L694), [서버 지원](../../electron/mobileRoutes.js#L255), [Mobile 상세 요청](../../mobile/lib/src/common/network/desktop_client.dart#L183) |
| A04 | 여러 AI provider 동시 선택 | 예를 들어 Claude와 Codex만 동시에 보기. 모바일은 전체 또는 단일 provider 선택이다. | 부분 구현. [Desktop 다중 선택](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L599), [Mobile 단일 선택](../../mobile/lib/src/features/chat_history/presentation/chat_history_screen.dart#L20) |
| A05 | 선택한 대화의 메시지 본문 검색 | Desktop 검색어는 세션 목록과 선택 대화 상세 조회에 함께 적용된다. 모바일에는 History 목록 검색만 있고 메시지 조회에는 검색어를 보내지 않는다. | 부분 구현. [Desktop 메시지 검색 인자](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L419), [Mobile 상세 요청](../../mobile/lib/src/common/network/desktop_client.dart#L183) |
| A06 | 채팅 내용 서식 렌더링 | Markdown·표·링크·이미지·Mermaid 표시. 모바일은 선택·복사가 가능한 원문 텍스트다. | 부분 구현. [Desktop 렌더러](../../src/desktop/renderer/components/AiSessionMessageContent.tsx#L78), [Mobile 원문 표시](../../mobile/lib/src/features/chat_history/presentation/chat_history_screen.dart#L256) |
| A07 | AI 계정 관리 | Claude·Codex·Gemini 계정 추가·삭제, 로그인 상태와 로그인 터미널. 태스크 터미널에서 CLI를 직접 실행하는 것과 별개다. | 미구현. [Desktop 계정 화면](../../src/desktop/renderer/routes/AiAccountsRoute.tsx#L107), [Mobile 앱 진입점](../../mobile/lib/src/app.dart#L116), [Mobile API 범위](../../electron/mobileRoutes.js#L75) |
| A08 | AI 사용량 확인 | 계정별 사용량·구독 정보·기간별 한도·초기화 시각·갱신. | 미구현. [Desktop 사용량](../../src/desktop/renderer/components/AiUsagePanel.tsx#L100), [Mobile API 범위](../../electron/mobileRoutes.js#L75) |

## 터미널 구성을 관리하는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| R01 | 탭 이름 변경 | 터미널 탭을 사용자 이름으로 바꾸기. 선택·추가·닫기는 이미 있다. | 부분 구현. [Desktop 이름 편집](../../src/desktop/renderer/components/TerminalTabBar.tsx#L42), [실제 rename](../../src/desktop/main/services/terminalTabService.ts#L351), [Mobile 탭](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L470) |
| R02 | 탭 순서 변경 | 탭 드래그 재정렬 및 세션 반영. | 부분 구현. [Desktop 이동](../../src/desktop/renderer/components/TerminalTabBar.tsx#L74), [실제 move](../../src/desktop/main/services/terminalTabService.ts#L405), [Mobile 탭](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L470) |
| R03 | Pane 레이아웃 템플릿 편집 | 전역·프로젝트별 레이아웃, 각 pane의 시작 명령 저장. PC에 저장된 설정을 모바일 생성 시 공통 backend가 적용하는 것과 설정 편집은 별개다. | 부분 구현. [Desktop 설정](../../src/desktop/renderer/routes/PaneLayoutRoute.tsx#L135), [템플릿 적용](../../src/lib/worktree.ts#L389), [Mobile 기존 pane 제어](../../mobile/lib/src/features/task_detail/domain/terminal_shortcut.dart#L49) |
| R04 | 분할 pane의 동시 표시 | Desktop은 터미널 세션의 분할 배치를 함께 표시한다. 모바일은 pane을 평탄화한 탭에서 하나씩 선택하며 넓은 화면도 같다. 분할·확대·floating 명령 자체는 지원한다. | 부분 구현. [Desktop 세션 표시](../../src/desktop/renderer/components/TerminalLoader.tsx#L6), [멀티플렉서 attach](../../src/lib/terminal.ts#L613), [Mobile 선택 pane](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L423), [넓은 화면 테스트](../../mobile/test/src/features/task_detail/task_detail_screen_test.dart#L818) |
| R05 | 원격 터미널 이미지 붙여넣기 | 클립보드 이미지를 SSH 대상에 올리고 파일 경로를 터미널에 입력. Desktop도 원격 세션 전용이다. 모바일의 일반 텍스트 입력·복사는 지원한다. | 미구현. [Desktop 이미지 전송](../../src/desktop/renderer/components/Terminal.tsx#L140), [원격 세션 조건](../../src/desktop/renderer/components/Terminal.tsx#L162), [Mobile 터미널](../../mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart#L235) |

## 프로젝트와 앱 설정을 관리하는 기능

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| P01 | SSH 프로젝트 탐색·등록 | SSH host를 선택해 원격 폴더를 탐색하고 저장소 등록. 모바일은 선택한 PC의 로컬 폴더 스캔만 제공한다. 이미 등록된 SSH 프로젝트의 태스크 생성·터미널은 지원한다. | 부분 구현. [Desktop host 선택](../../src/components/ProjectRegistryDialog.tsx#L138), [Mobile 로컬 스캔](../../src/desktop/main/services/mobileBridgeService.ts#L515) |
| P02 | 등록 프로젝트 삭제 | 프로젝트 등록 목록에서 삭제. 태스크 삭제·깨진 태스크 등록만 제거와는 다른 동작이다. | 미구현. [Desktop 프로젝트 삭제](../../src/components/ProjectRegistryDialog.tsx#L93), [Mobile 등록 화면](../../mobile/lib/src/features/project_registry/presentation/project_registry_screen.dart#L62) |
| S01 | 기본 세션 종류 변경 | 앞으로 생성할 태스크의 PC 기본 세션 종류 설정. 모바일은 기본값을 따르지만 바꾸는 화면은 없다. | 부분 구현. [Desktop 기본값 설정](../../src/components/ProjectSettings.tsx#L328), [Mobile 기본값 사용](../../src/desktop/main/services/mobileBridgeService.ts#L713) |
| S02 | 기존 태스크의 세션 종류 일괄 변경 | 여러 기존 태스크를 한꺼번에 전환하고 이전 세션 종료 여부 선택. 모바일은 태스크별 전환만 있다. | 부분 구현. [Desktop 일괄 전환](../../src/components/BulkSessionTypeChangeSettings.tsx#L33), [Mobile 개별 전환](../../mobile/lib/src/features/task_detail/presentation/widgets/task_session_type_sheet.dart#L33) |
| S03 | 시스템·라이트·다크 테마 선택 | 테마 선택과 저장. 모바일은 다크 테마 고정이다. | 미구현. [Desktop 테마](../../src/components/ProjectSettings.tsx#L173), [Mobile 다크 고정](../../mobile/lib/src/common/constants/app_theme.dart#L12) |
| S04 | 다국어 화면 | 한국어·영어·중국어 UI 리소스와 locale 적용. 모바일 주요 UI는 한국어 문자열이고 locale delegate가 없다. Desktop의 언어 선택 버튼은 확인하지 않았다. | 미구현. [Desktop locale](../../src/desktop/renderer/utils/locales.ts#L1), [Mobile 앱 설정](../../mobile/lib/src/app.dart#L116) |
| S05 | PC 백그라운드 동기화 제어 | 동기화 켜기·주기 변경·즉시 실행. 모바일 새로고침은 보드 재조회이며 이 동작과 다르다. PC가 자동 동기화한 결과의 보드 갱신은 수신한다. | 부분 구현. [Desktop 주기 설정](../../src/components/ProjectSettings.tsx#L435), [즉시 실행](../../src/desktop/renderer/components/CommandPaletteDialog.tsx#L54), [Mobile 이벤트 반영](../../src/desktop/main/services/mobileNotificationService.ts#L70) |
| S06 | 동기화 결과 검토와 후속 처리 | 새 worktree·자동 pull·실패 내역·병합 PR 태스크 검토, 선택 태스크 삭제. 모바일 알림함은 텍스트를 읽지만 검토 액션은 실행하지 않는다. | 부분 구현. [Desktop 검토](../../src/desktop/renderer/components/BackgroundSyncReviewDialog.tsx#L55), [알림 액션](../../src/components/NotificationCenterButton.tsx#L232), [Mobile task 없는 알림](../../mobile/lib/src/features/notifications/presentation/notification_center_screen.dart#L109) |

## 알림의 세부 동작

| ID | 데스크톱 기능 | 모바일에 없는 범위 | 판정과 근거 |
| --- | --- | --- | --- |
| N01 | 태스크 상세 포커스 시 관련 알림 자동 읽음 | Desktop은 태스크 상세가 활성화될 때 해당 태스크 알림을 읽음 처리한다. 모바일은 알림함에서 개별·모두 읽음과 항목 클릭 시 읽음만 연결돼 있고 보드 카드로 상세를 열 때는 실행하지 않는다. | 부분 구현. [Desktop focus 읽음](../../src/desktop/renderer/hooks/useMarkTaskNotificationsReadWhenFocused.ts#L11), [Mobile 상세 진입](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L35), [Mobile 명시적 읽음](../../mobile/lib/src/features/notifications/application/notification_inbox_controller.dart#L72) |
| N02 | 운영 이벤트의 실시간 경고 | hook 상태 변경 대상 누락 알림, 태스크 생성 후 hook 설치 실패 경고. 모바일 전경·푸시 경로는 board 갱신과 태스크 상태 변경 중심이다. 알림함에 저장된 항목을 읽는 기능과 즉시 경고는 구분해야 한다. 모바일이 요청한 삭제의 peer 실패 안내는 이미 있다. | 부분 구현. [Desktop hook 경고](../../src/desktop/renderer/components/BoardEventAlert.tsx#L9), [대상 누락 시스템 알림](../../src/desktop/main/services/desktopNotificationService.ts#L114), [Mobile 이벤트 선별](../../src/desktop/main/services/mobileNotificationService.ts#L70) |

## PC 운영과 플랫폼 차이로 분리한 기능

이 표는 모바일에 동일한 기능이 없다는 비교다. 휴대폰용 기능으로 제공할지, PC에서 계속 관리할지는 별도 제품 판단이 필요하다.

| 항목 | Desktop 구현 | Mobile 현재 상태 |
| --- | --- | --- |
| 태스크별 Hooks 상태 진단·재조회·설치·재설치 | [HooksStatusCard](../../src/components/HooksStatusCard.tsx#L88), [상세 연결](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L1813) | [상태 시트](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L44)는 PC에서 관리하도록 제외한다. 공통 backend의 자동 hook 설정·상태 알림은 이미 동작한다. |
| 세션 도구 설치 | [세션 선택 설치 흐름](../../src/components/SessionTypeEditor.tsx#L33), [생성 설치 흐름](../../src/components/CreateTaskModal.tsx#L163) | [서버 의존성 오류](../../src/desktop/main/services/mobileBridgeService.ts#L720)로 PC에서 설치하도록 안내한다. |
| SSH 대상 GitHub CLI 설치 | [PR 조회의 설치 프롬프트](../../src/desktop/renderer/utils/fetchPrUrlWithPrompt.ts#L22) | 모바일에는 설치 제어가 없다. 모든 로컬 CLI에 공통으로 적용되는 설치 기능은 아니다. |
| PC VS Code·Remote SSH 열기 | [상세 편집기 열기](../../src/desktop/renderer/routes/TaskDetailRoute.tsx#L963), [PC 프로세스 실행](../../src/desktop/main/services/editorService.ts#L46) | 모바일에는 PC 편집기를 여는 액션이 없다. |
| 전역 앱 단축키 설정·Vim 보드·명령 팔레트 | [단축키 설정](../../src/desktop/renderer/routes/ShortcutSettingsRoute.tsx#L80), [Vim 보드](../../src/components/Board.tsx#L568), [명령 팔레트](../../src/desktop/renderer/components/CommandPaletteDialog.tsx#L87) | 모바일 터미널 전송 키·사용자 콤보·순서 편집은 이미 지원한다. 앱 전체의 동일한 키보드 체계는 없다. |
| 여러 창과 상세 sidebar 설정 | [창 정책](../../src/desktop/renderer/utils/taskNavigation.ts#L1), [sidebar 기본 접기](../../src/components/ProjectSettings.tsx#L293) | 모바일 화면 전환·메뉴·시트는 구현돼 있다. 동일한 별도 창·sidebar 구조가 없다. |
| 페어링 서버와 다른 기기 관리 | [코드 발급·취소와 기기 목록](../../src/components/MobilePairingSettings.tsx#L143), [기기 해제·LAN 설정](../../src/components/MobilePairingSettings.tsx#L213) | 모바일은 PC 추가·선택·별칭 변경·자기 연결 해제를 지원한다. 다른 기기 revoke, PC의 페어링 코드 발급·LAN 평문 정책 제어는 없다. |
| 릴리스 버전 공지 | [버전 공지](../../src/desktop/renderer/components/ReleaseUpdateDialog.tsx#L206), [버전별 숨김·릴리스 링크](../../src/desktop/renderer/components/ReleaseUpdateDialog.tsx#L309) | 대응 화면이 없다. Desktop 기능도 자동 앱 업데이트가 아니라 공지·링크·해당 버전 숨김이다. |

## 이미 구현된 모바일 기능

| 영역 | 확인된 구현 범위 | 코드 근거 |
| --- | --- | --- |
| 여러 PC 연결 | 페어링, 저장·선택, 별칭 변경, 자기 연결 해제 | [연결 목록](../../mobile/lib/src/features/connection/presentation/desktop_connections_screen.dart#L25) |
| 실시간 보드 | 상태별 목록, 폰·태블릿 배치, 수동 갱신, PC 이벤트·앱 복귀·재연결 갱신 | [보드](../../mobile/lib/src/features/board/presentation/board_screen.dart#L136), [이벤트](../../src/desktop/main/services/mobileNotificationService.ts#L70) |
| 프로젝트 필터 | 다중 선택·검색, 선택 PC별 저장, 소속 worktree 포함 | [필터 상태](../../mobile/lib/src/features/board/application/board_project_filter_controller.dart#L18), [worktree 포함](../../mobile/lib/src/features/board/data/board_snapshot.dart#L25) |
| 프로젝트 등록 | PC 폴더 탐색·직접 입력·상위 이동, 재귀 Git 스캔, 신규·중복·실패 결과, worktree 발견 | [등록 화면](../../mobile/lib/src/features/project_registry/presentation/project_registry_screen.dart#L70) |
| 태스크 생성 | 프로젝트·base branch·브랜치·설명·우선순위, 현재 태스크에서 후속 태스크 생성, 생성 후 상세 이동 | [생성](../../mobile/lib/src/features/task_create/presentation/create_task_screen.dart#L79), [후속 생성](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L43) |
| 태스크 상태·삭제 | 상태 이동, 카드 길게 누르기·상태 시트 삭제, dirty worktree 강제 삭제 확인, 깨진 등록만 제거, peer 삭제 실패 안내 | [상태·삭제](../../mobile/lib/src/features/task_detail/presentation/widgets/task_status_sheet.dart#L65), [명령 정의](../../mobile/lib/src/common/network/desktop_command.dart#L5) |
| 정보 조회·색상 | 프로젝트·브랜치·base·상태·에이전트·SSH·우선순위·설명, PC의 프로젝트 색상 표시 | [정보 시트](../../mobile/lib/src/features/task_detail/presentation/widgets/task_info_sheet.dart#L35), [카드 색상](../../mobile/lib/src/features/board/presentation/board_screen.dart#L438) |
| 원격 터미널·세션 | tmux·Zellij·Herdr·일반 terminal, SSH 대상, 세션 시작·재시도, 종류 변경과 이전 세션 유지·종료 선택 | [세션 상태](../../mobile/lib/src/common/providers.dart#L251), [종류 변경](../../mobile/lib/src/features/task_detail/presentation/widgets/task_session_type_sheet.dart#L33) |
| 터미널 탭 | 선택·추가·프로세스 확인 후 닫기·마지막 탭 보호 | [탭 추가·닫기](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L98) |
| Pane 제어 | 좌우·상하 분할, 확대, tmux copy mode·scroll, Zellij floating | [전송 액션](../../mobile/lib/src/features/task_detail/domain/terminal_shortcut.dart#L49) |
| 터미널 입력 보조 | Esc·Tab·Ctrl·Alt·화살표 등, 키 표시·순서·사용자 콤보 설정 | [단축키 정의](../../mobile/lib/src/features/task_detail/domain/terminal_shortcut.dart#L30) |
| 스크롤·복사·화면 맞춤 | 과거 출력 스크롤, 한글·여러 줄 선택 복사, OSC52 수신 복사, viewport 크기 적용과 복원 | [터미널 뷰](../../mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart#L148), [복사](../../mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart#L217) |
| 채팅 History | 목록 검색·단일 provider 필터·세션 상세·더보기·이전 메시지·원문 선택·메시지 복사 | [History](../../mobile/lib/src/features/chat_history/presentation/chat_history_screen.dart#L39) |
| PR 열기 | 저장된 PR URL을 외부 앱으로 열기 | [링크 열기](../../mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart#L251) |
| 알림함 | 전체·안 읽음, 개별·모두 읽음, 카드 미읽음 배지, 최신 태스크 조회 후 이동 | [알림함](../../mobile/lib/src/features/notifications/presentation/notification_center_screen.dart#L70), [카드 배지](../../mobile/lib/src/features/board/presentation/board_screen.dart#L420) |
| 알림 설정 | PC별 on/off·상태 선택과 Desktop AppSettings 공유 | [설정 화면](../../mobile/lib/src/features/notifications/presentation/notification_settings_screen.dart#L20) |
| 전경·백그라운드 알림 | 저장한 PC 전체의 전경 상태 이벤트, FCM/APNs 경로, 클릭 후 PC 선택·최신 태스크 이동, lifecycle 처리 | [앱 lifecycle](../../mobile/lib/src/app.dart#L33), [알림 제어](../../mobile/lib/src/features/notifications/application/mobile_notification_controller.dart#L71). 실제 백그라운드 수신에는 Firebase/APNs·중계 설정이 필요하다. |

## 비교에서 제외한 추정

- **태스크 제목 편집**: backend의 범용 update는 제목을 받지만 조사한 Desktop 상세 UI는 설명 편집이다. Desktop 제목 편집이 있다는 가정으로 모바일 누락을 만들지 않았다.
- **프로젝트 이름·기본 브랜치 편집, 일반 보드 다중선택·일괄 상태변경, 터미널 글자 크기 설정**: 모바일 부재만으로는 부족하다. 대응하는 Desktop UI의 연결된 구현을 확인하지 못해 목록에서 제외했다.
- **같은 상태 열 안의 수동 순서 저장**: Desktop도 [같은 열 이동을 무시](../../src/components/Board.tsx#L285)한다. 상태 열 사이 드래그는 지원하지만 모바일의 상태 시트와 비교하면 입력 방식 차이다. Pending 선택 차이는 T07에 따로 기록했다.
- **모바일 분할·탭 닫기·History·프로젝트 등록 미구현**: 현재 코드에는 모두 구현돼 있으므로 누락으로 분류하지 않았다.
- **모바일 원격 기능 전체 미구현**: 기존 SSH 프로젝트 기반 생성과 원격 터미널은 지원한다. 빠진 것은 SSH host를 지정한 프로젝트 등록이다.
- **자동 앱 업데이트·CLI 토큰 관리 GUI·언어 선택 버튼**: Desktop의 실제 구현 범위를 넘어서는 주장이라 제외했다.
- **프로젝트 그룹 접기·브랜치 트리**: 보조 함수나 컴포넌트 이름만으로 기능을 판정하지 않았다. 현재 [ProjectTaskGroup](../../src/components/ProjectTaskGroup.tsx#L63)은 시각적 그룹 상자 없이 자식만 표시한다.

## 후속 구현을 묶는 기준

제품 우선순위가 정해지지 않았으므로 다음은 코드 의존성에 따른 묶음이다.

1. **보드 탐색과 기본 수정**: B01–B05, T02–T03, T07, T10, N01. 추가 조회·검색·수정 API와 모바일 화면을 함께 연결한다. Done pagination은 조회 가능한 작업 범위를 직접 넓힌다.
2. **변경 검토**: G01–G04와 T09의 저장 URL이 없을 때 PR 조회. Diff 읽기·파일 저장 경계를 먼저 설계하고 모바일 파일 선택·검토 UI를 연결한다.
3. **AI 관찰과 History 보완**: A01–A06. 역할·본문 검색은 기존 모바일 서버 지원을 앱에 연결할 수 있다. Live 세션·호출 그래프는 새 전송 경로가 필요하다.
4. **구성·운영**: R01–R05, P01–P02, S01–S06, A07–A08. PC 설정을 모바일에서 어느 범위까지 편집할지 정한 뒤 API와 권한 범위를 추가한다.
