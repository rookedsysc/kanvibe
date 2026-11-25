# kanvibe-runtime

앱 프로세스에 하나만 있어야 하는 서비스(DB, 보드·프로젝트·diff, hook 설치기, 터미널 레지스트리·브리지·탭,
pane 미러·pane chat, AI 계정·사용량·로그인 브리지, 알림 저장소·OS 알림 전달, 백그라운드 sync, 릴리스 확인,
다이어그램 렌더러)를 `DesktopRuntime` 하나로 만든다. GPUI 앱과 `kanvibe-server`가 같은 인스턴스를 쓴다.

- 앱: `main`이 `kanvibe_app::init_desktop_app`으로 연다. `desktop_process::DesktopLaunchPlan`이 TS `runtimeEnvironment.js`와
  같은 규칙으로 데이터 디렉터리(설치 앱 = 릴리스 빌드 `<appData>/KanVibe`, 개발 실행 = 디버그 빌드 `<appData>/kanvibe-dev`,
  `--data-dir`이 있으면 그 값. Linux `<appData>`는 `$XDG_CONFIG_HOME` 또는 `~/.config`, macOS는 `~/Library/Application Support`)와
  hook 포트(9736 / 19736, `--kanvibe-hook-port`가 있으면 그 값)를 고른다. 개발 실행이면 `development_database`(`--data-source`,
  `--application-data-dir`)를, 모든 실행에서 `diagnostics`(`<data>/logs/kanvibe-desktop.log`)를 먼저 준비한다.
  `AppServices`가 `Arc<DesktopRuntime>`과 모바일 업무 계층 `Arc<DesktopServices>`를 갖는다.
  `kanvibe_app::start_runtime_services`가 첫 창을 연 뒤 `start()`를 한 번 부르고 프로세스 안 서버(`InProcessServer`)를 띄운다.
  잠자기에서 깨면(GPUI `on_system_wake`: Linux logind `PrepareForSleep(false)` D-Bus 신호, macOS `NSWorkspaceDidWakeNotification`)
  hook 터널을 다시 잇고, 종료(마지막 창 닫기, SIGTERM·SIGINT)할 때 `on_app_quit`에서 서버·런타임을 TS `before-quit` 순서로 닫는다.
  `--qa` 화면 캡처는 `--kanvibe-hook-port`를 줄 때만 서버를 띄운다. 화면 시험은 서버 없는 `init_app`을 쓴다.
- 서버(QA 실행기): `RemoteHostWiring::wire_runtime(&mut config)` → `DesktopRuntime::new` → `DesktopServices::new` →
  `runtime.start()` → `InProcessServer::start`. 앱도 같은 순서다.
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
| settings `install_mobile_pairing_bridge` | `cx.global::<AppServices>().mobile_devices()`(`Option<&Arc<MobileDevices>>`: `start_mobile_pairing`, `stop_mobile_pairing`, `read_mobile_pairing_code`, `list_paired_mobile_devices`, `unpair_mobile_device`). LAN 평문 설정은 `desktop_services().lan_usage`. 프로세스 안 서버와 같은 인스턴스라 화면에서 해제한 기기의 소켓은 서버가 4001로 닫는다. 데이터 디렉터리 없이 뜬 실행이면 `None` |
| `crate::qa::{AppClock, NetworkResponseTable}` | 경로 그대로(런타임 타입을 다시 내보낸다) |
| 새로 고침(TS `useRefreshSignal`) | `app_events::subscribe_to_refresh(scopes, cx, |view, cx| view.reload(cx))`, 보낼 때 `trigger_refresh(scope, cx)` |
| 필터 쿼리만 바꾸기 | `WindowNavigator::replace_search(href, cx)`(화면을 다시 만들지 않는다) |
| locale 정렬(TS `localeCompare`) | `crate::text_collation::LocaleCollator::for_locale(locale).compare(a, b)` |
| provider 아이콘 | `assets::provider_icon_paths::{CLAUDE, CODEX, GEMINI, OPENCODE}`를 `img()`로 |

pane 레이아웃은 상태 없는 함수(`kanvibe_terminal::pane_layout_service::*`)라 `runtime.database()`를 넘겨 부른다.

## 서버와 앱이 함께 쓰는 조립

- pane 미러·pane chat은 런타임 것 하나다(`runtime.pane_mirror()`, `runtime.pane_chat()`). 원격 terminal agent 권한은
  `RemoteHostWiring`이 시작 환경 스냅샷과 `SshConfigReader`로 런타임보다 먼저 만들어 `RuntimeConfig.remote_pane_grants`로 넣고,
  `runtime.shutdown()`이 미러를 멈출 때 agent도 내린다.
- 보드 이벤트 알림은 `runtime.start()`가 한 번만 건다. 서버는 따로 구독하지 않는다(알림 중복 없음).
  OS 알림은 `RuntimeConfig.notification_presenter`(앱: GPUI 시스템 알림, Linux XDG D-Bus `notify-rust`. macOS
  `UNUserNotificationCenter` 경로는 이 머신에서 미검증. 창 없는 실행: `SilentNotificationPresenter`).
- 알림 문구 locale은 `RuntimeConfig.notification_locale`이 이벤트마다 정한다. 기본값은 시작 환경의 시스템 locale
  (`system_notification_locale`, TS `app.getLocale()`), 앱은 가장 먼저 연 창 경로의 locale을 먼저 본다(TS `getDesktopNotificationLocale`).
  QA 실행기는 골든을 녹화한 TS 앱의 첫 창(`/ko`)과 같게 `DEFAULT_NOTIFICATION_LOCALE`을 쓴다.
- 모바일 다이어그램 라우트는 `runtime.diagram_renderer()`를 `kanvibe_server::diagram::render_mobile_diagram`으로 감싸 쓴다.
  글꼴은 `RuntimeConfig.diagram_font_files`(앱: 번들 글꼴, QA 실행기: `--diagram-fonts`, 기본 앱 번들 글꼴 디렉터리).
- QA 실행이면 `RuntimeConfig.qa_network_responses`와 `clock`을 넣는다.
