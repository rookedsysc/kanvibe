# kanvibe-runtime

앱 프로세스에 하나만 있어야 하는 서비스(DB, 보드·프로젝트·diff, hook 설치기, 터미널 레지스트리·브리지·탭,
pane 미러·pane chat, AI 계정·사용량·로그인 브리지, 알림 저장소·OS 알림 전달, 백그라운드 sync, 릴리스 확인,
다이어그램 렌더러)를 `DesktopRuntime` 하나로 만든다. GPUI 앱과 `kanvibe-server`가 같은 인스턴스를 쓴다.

- 앱: `init_app`이 `--data-dir`로 런타임을 열고 `AppServices`가 `Arc<DesktopRuntime>`을 갖는다.
  `kanvibe_app::start_runtime_services`가 첫 창을 연 뒤 `start()`를 한 번 부른다(sync 루프, 보드 이벤트 알림 전달).
- 서버: `DesktopRuntime::new(RuntimeConfig)` 뒤 `runtime.start()`를 부르고, 아래 접근자를 쓴다.
- 이 crate는 프로세스 환경을 바꾸지 않는다. 시작 스냅샷을 `RuntimeConfig.base_environment`로 받아 자식 환경을 좁힌다.

## 화면 작업자용: 임시 전역을 런타임 접근자로 바꾸기

모든 화면은 `cx.global::<AppServices>().runtime()`(데이터 디렉터리 없이 뜬 실행이면 `None`)으로 런타임을 받는다.
DB가 없을 때의 문구·동작은 지금처럼 화면이 정한다.

| 지금 임시로 둔 것 | 바꿀 것 |
|---|---|
| task-detail `TerminalRuntimeSlot`, `terminal_runtime(cx)` | `runtime.terminal_registry()`, `runtime.terminal_bridge()`(`&Arc<TerminalBridge>`), `runtime.terminal_tab_service()`. `open_window()`·`ViewTerminalWindow`·`TerminalWindowEvent`는 화면 쪽에 남긴다(창 id 카운터만 뷰 소유로) |
| task-detail `DirectSshHosts` | 삭제. 런타임이 `RuntimeConfig.remote_host_connections`를 쓴다(기본 연결 없음, 서버가 kanvibe-remote-host 연결을 넣는다) |
| task-detail `LoggedTerminalEvents` | 삭제. 런타임의 `BoardTerminalEvents`가 보드 이벤트로 낸다. 화면은 `app_events::BoardEventReceived`를 구독한다 |
| task-detail `HookServerNotComposed` | 삭제. 브리지가 런타임의 `KanvibeHooksInstaller`로 hook을 확인한다 |
| task-detail `SharedKanbanService` | `runtime.kanban()` |
| task-detail `data.rs`의 `apply_fault` 복사본 | `crate::services::apply_fault`(공개됨). 판정은 `AppServices::check_fault(target, json_args)` |
| diff `SharedDiffService`, `shared_diff_service` | `runtime.diff()`(`&Arc<DiffService>`) |
| diff `text_baseline.rs` | `crate::text_baseline`(같은 API: `TextFamily`, `aligned_text`, `chromium_text`, `baseline_lift`) |
| board `SharedBoardServices`(Kanban·Diff·NotificationStore) | `runtime.kanban()`, `runtime.diff()`, `runtime.notification_store()` |
| board `adapters.rs`(hook 설치 "hook server is not running", 삭제 전파 생략) | 삭제. 런타임 KanbanService가 실제 hook 설치기와 `TaskTargetPropagation`(다른 클라이언트로 삭제 전파)을 쓴다 |
| board ProjectService 전역(scan/delete) | `runtime.project()` |
| board BoardNotifier 구독·debounce | `crate::app_events`: `BoardEventReceived`(모든 보드 이벤트), `subscribe_to_refresh(&[RefreshScope::Board], cx, ..)`(board-updated 250ms 모음 → `All`) |
| board "지금 동기화" | `runtime.background_task_sync().start_background_task_sync_now()`, 상태는 `get_background_task_sync_status()`(같은 인스턴스) |
| board 알림 센터 | 공용 `components::notification_center::NotificationCenterButton`(아래 남은 항목 참고). 목록은 `NotificationCenter` 전역, 변경은 `NotificationsChanged` |
| board·settings QA 자동 확인(`confirm_action`, `window_confirm_prompt`) | `crate::qa::prompts::confirm_action(message, detail, window, cx)` |
| 오류 문구(IPC 머리말 붙이기·떼기) | `crate::services::format_service_error(reason)` 또는 `ServiceFailure::display_message()`. TS와 같은 `Error invoking remote method 'kanvibe:invoke': Error: ` 머리말을 붙인다 |
| settings `AiAccountsBackendHandle`(자체 UsageContext·계정·사용량·로그인 PTY) | `runtime.ai_account_service()`, `runtime.ai_usage_service()`, `runtime.ai_desktop_login_bridge()`. QA 응답 표와 고정 시각은 런타임이 이미 쓴다 |
| settings `install_session_type_bulk_changer` | `runtime.kanban()` + `runtime.session_dependency()` |
| settings `install_mobile_pairing_bridge` | 아직 없음. 기기 목록·페어링은 kanvibe-mobile `MobileDevices`가 갖고 지금은 서버(`DesktopServices.devices`)가 만든다. 앱이 서버를 품을 때 그 인스턴스를 쓴다 |
| `crate::qa::{AppClock, NetworkResponseTable}` | 경로 그대로(런타임 타입을 다시 내보낸다) |
| 새로 고침(TS `useRefreshSignal`) | `app_events::subscribe_to_refresh(scopes, cx, |view, cx| view.reload(cx))`, 보낼 때 `trigger_refresh(scope, cx)` |
| 필터 쿼리만 바꾸기 | `WindowNavigator::replace_search(href, cx)`(화면을 다시 만들지 않는다) |
| locale 정렬(TS `localeCompare`) | `crate::text_collation::LocaleCollator::for_locale(locale).compare(a, b)` |
| provider 아이콘 | `assets::provider_icon_paths::{CLAUDE, CODEX, GEMINI, OPENCODE}`를 `img()`로 |

pane 레이아웃은 상태 없는 함수(`kanvibe_terminal::pane_layout_service::*`)라 `runtime.database()`를 넘겨 부른다.

## 서버(QA 실행기)가 같은 조립을 쓰려면

- `DesktopServices::new`의 `build_mirror`·`build_pane_chat` 대신 `runtime.pane_mirror()`·`runtime.pane_chat()`을 쓴다.
  원격 직접 연결 권한은 `RuntimeConfig.remote_pane_grants`로 넘긴다(지금은 `remote.remote_grants(&runtime)`가 런타임을
  받으므로, 권한 서비스를 환경 스냅샷·`SshConfigReader`로 먼저 만드는 순서로 바꿔야 한다).
- `forward_board_event_notifications` 대신 `RuntimeConfig.notification_presenter`(창 없는 실행은 `SilentNotificationPresenter`)를
  넣고 `runtime.start()`만 부른다. 둘 다 하면 같은 이벤트가 알림 두 개로 남는다.
- 모바일 다이어그램 라우트는 `runtime.diagram_renderer()`(`kanvibe_diagram::service::DiagramRenderer`)를 쓴다.
  글꼴은 `RuntimeConfig.diagram_font_files`로 넘긴다(첫 요청 때 한 번 읽는다).
- QA 실행이면 `RuntimeConfig.qa_network_responses`와 `clock`을 넣는다.
