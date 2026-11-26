//! KanVibe GPUI 데스크톱 앱. Electron 렌더러(`src/desktop/renderer`)와 main 창 관리를 대신한다.

pub mod app_events;
pub mod app_window;
pub mod assets;
mod blocking_call;
pub mod board_focus_target;
pub mod components;
pub mod desktop_process;
pub mod fonts;
pub mod locale_format;
pub mod navigation;
pub mod notifications;
pub mod polled_value;
pub mod qa;
mod render_cache;
pub mod routes;
pub mod services;
pub mod shortcuts;
pub mod text_baseline;
pub mod text_collation;
pub mod theme;
pub mod views;
pub mod window_open;
pub mod windows;

use std::path::Path;
use std::sync::Arc;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{App, TextRenderingMode, WindowAppearance};
use kanvibe_process::hooks::endpoint::KANVIBE_HOOK_SERVER_PORT;
use kanvibe_process::shell_environment::{ShellEnvironment, current_user_home_directory};
use kanvibe_runtime::{DesktopRuntime, RuntimeConfig, system_notification_locale};
use kanvibe_server::desktop::remote::RemoteHostWiring;
use kanvibe_server::desktop::{DesktopServerConfig, DesktopServices, push_relay_config};
use kanvibe_server::server::MOBILE_TLS_PORT_OFFSET;
use kanvibe_services::board_notifier::BoardEventSubscription;
use kanvibe_services::desktop_notification::NotificationDeliveryCallbacks;
use kanvibe_services::notification_store::NotificationChangeSubscription;

use crate::app_events::{RuntimeEvent, install_app_events, runtime_event_channel};
use crate::desktop_process::{DesktopLaunchPlan, prepare_desktop_process};
use crate::notifications::ForegroundNotificationPresenter;
use crate::qa::faults::FaultRegistry;
use crate::qa::options::LaunchOptions;
use crate::qa::perf_markers::mark_startup_phase;
use crate::qa::targets::QaTargetRegistry;
use crate::qa::{AppClock, NetworkResponseTable, QaRuntime};
use crate::services::AppServices;
use crate::shortcuts::ShortcutState;
use crate::theme::ThemeTokens;
use crate::windows::{WindowNotificationLocale, WindowRegistry};

/// 창을 열기 전 앱 전역을 준비한다. QA 입력은 모두 `launch`에서 오고 프로세스 환경은 읽기만 한다.
/// 서버·진단 로그·개발 DB 없이 `launch.data_dir`의 런타임만 연다(화면 시험과 캡처). 실제 앱은 [`init_desktop_app`]을 쓴다
pub fn init_app(launch: &LaunchOptions, cx: &mut App) -> anyhow::Result<()> {
    init_app_with_desktop(launch, None, cx)
}

/// 실제 앱 실행: `plan`대로 데이터 디렉터리를 고르고 진단 로그·개발 DB·원격 호스트 조립까지 마친다.
/// 서버는 첫 창을 연 뒤 [`start_runtime_services`]가 띄운다
pub fn init_desktop_app(
    launch: &LaunchOptions,
    plan: &DesktopLaunchPlan,
    cx: &mut App,
) -> anyhow::Result<()> {
    init_app_with_desktop(launch, Some(plan), cx)
}

fn init_app_with_desktop(
    launch: &LaunchOptions,
    plan: Option<&DesktopLaunchPlan>,
    cx: &mut App,
) -> anyhow::Result<()> {
    gpui_kit::init(cx);
    mark_startup_phase("gpui_kit_init");
    fonts::register_bundled_fonts(cx)?;
    mark_startup_phase("fonts_registered");
    // 기준 이미지는 그레이스케일 안티에일리어싱(fontconfig rgba none)으로 찍혔다
    cx.set_text_rendering_mode(TextRenderingMode::Grayscale);

    let (faults, clock, network_responses) = install_qa_runtime(launch, cx)?;
    let (sender, receiver) = runtime_event_channel();
    let window_locale = WindowNotificationLocale::default();
    let environment: ShellEnvironment = std::env::vars().collect();
    let home_directory = current_user_home_directory();
    let data_dir = match plan {
        Some(plan) => plan.data_dir.clone(),
        None => launch.data_dir.clone(),
    };
    let hook_port = plan.map_or(KANVIBE_HOOK_SERVER_PORT, |plan| plan.hook_port);
    let mut desktop = match (plan, data_dir.as_deref()) {
        (Some(plan), Some(data_dir)) => Some(prepare_desktop_process(
            plan,
            data_dir,
            &environment,
            &home_directory,
        )?),
        _ => None,
    };
    mark_startup_phase("desktop_process_prepared");
    let runtime = match data_dir.as_deref() {
        Some(data_dir) => Some(open_runtime(
            &RuntimeInputs {
                launch,
                data_dir,
                hook_port,
                environment: &environment,
                home_directory: &home_directory,
                window_locale: &window_locale,
                remote: desktop.as_ref().and_then(|desktop| desktop.remote.as_ref()),
            },
            clock,
            network_responses,
            &sender,
        )?),
        None => None,
    };
    mark_startup_phase("runtime_opened");
    let notification_subscription = runtime
        .as_ref()
        .map(|runtime| relay_notification_changes(runtime, sender.clone()));
    let board_subscription = runtime
        .as_ref()
        .map(|runtime| relay_board_events(runtime, sender));
    let desktop_services = runtime.as_ref().map(|runtime| {
        Arc::new(DesktopServices::new(
            Arc::clone(runtime),
            DesktopServerConfig {
                home_directory: home_directory.clone().into(),
                mobile_tls_port: hook_port.saturating_add(MOBILE_TLS_PORT_OFFSET),
                push_relay: push_relay_config(&environment),
            },
        ))
    });
    let mut services = AppServices::new(runtime, faults, clock, launch.shortcut_platform());
    if let Some(desktop_services) = desktop_services {
        services = services.with_desktop_services(desktop_services);
    }
    if let Some(mut prepared) = desktop.take() {
        if let (Some(plan), Some(data_dir), Some(remote)) =
            (plan, data_dir.as_deref(), prepared.remote.take())
        {
            desktop_process::plan_server(&mut prepared.process, plan, data_dir, remote);
        }
        cx.set_global(prepared.process);
    }

    mark_startup_phase("services_built");
    let is_system_light = cx.window_appearance() == WindowAppearance::Light
        || cx.window_appearance() == WindowAppearance::VibrantLight;
    cx.set_global(ThemeTokens::for_mode(
        services.theme_preference().resolve(is_system_light),
    ));
    cx.set_global(ShortcutState {
        bindings: services.shortcut_bindings(),
        platform: services.shortcut_platform,
        capturing_windows: Default::default(),
    });
    cx.set_global(services);
    cx.set_global(WindowRegistry::with_notification_locale(window_locale));
    cx.set_global(notifications::NotificationCenter::default());
    install_app_events(receiver, board_subscription, notification_subscription, cx);
    notifications::reload_notification_center(cx);
    shortcuts::install_shortcut_dispatch(cx).detach();
    notifications::install_notification_click_routing(cx);
    mark_startup_phase("app_globals_ready");
    Ok(())
}

/// 백그라운드 task sync와 보드 이벤트 알림 전달을 켜고, [`init_desktop_app`]으로 시작했으면 hook·모바일 서버를 띄운다.
/// 앱 프로세스에서 첫 창을 연 뒤 한 번 부른다
pub fn start_runtime_services(cx: &mut App) {
    desktop_process::start_desktop_process(cx);
}

/// 런타임 조립 재료
struct RuntimeInputs<'a> {
    launch: &'a LaunchOptions,
    data_dir: &'a Path,
    /// 설치하는 hook이 부를 포트(TS `setHookServerPort`)
    hook_port: u16,
    environment: &'a ShellEnvironment,
    home_directory: &'a str,
    window_locale: &'a WindowNotificationLocale,
    /// 서버를 띄우는 실행이면 원격 daemon·hook 터널·원격 agent 조립
    remote: Option<&'a RemoteHostWiring>,
}

/// 앱 프로세스의 서비스 그래프 하나. 자식 프로세스 환경은 런타임이 시작 스냅샷을 좁혀 만든다
fn open_runtime(
    inputs: &RuntimeInputs<'_>,
    clock: AppClock,
    network_responses: Option<NetworkResponseTable>,
    sender: &UnboundedSender<RuntimeEvent>,
) -> anyhow::Result<Arc<DesktopRuntime>> {
    let launch = inputs.launch;
    let mut config = RuntimeConfig::new(
        inputs.data_dir,
        inputs.hook_port,
        inputs.home_directory,
        inputs.environment.clone(),
    );
    if let Some(remote) = inputs.remote {
        remote.wire_runtime(&mut config);
    }
    // TS: 앞에 있는 창(첫 창) 경로의 locale, 창이 없으면 시스템 locale
    let window_locale = inputs.window_locale.clone();
    let system_locale = system_notification_locale(inputs.environment);
    config.notification_locale = Arc::new(move || {
        window_locale
            .current()
            .unwrap_or_else(|| system_locale.clone())
    });
    config.shortcut_platform = launch.shortcut_platform();
    config.clock = clock;
    config.qa_network_responses = network_responses.map(Arc::new);
    config.notification_presenter = Arc::new(ForegroundNotificationPresenter {
        sender: sender.clone(),
    });
    // 알림 목록 변경은 저장소 구독(`relay_notification_changes`)이 모든 쓰기에서 전한다. 여기서 또 보내면 두 번 다시 읽는다
    config.notification_callbacks = NotificationDeliveryCallbacks {
        on_notifications_changed: None,
        on_notification_click: None,
    };
    config.diagram_font_files = Arc::new(|| {
        fonts::BUNDLED_FONT_FILES
            .iter()
            .map(|bytes| bytes.to_vec())
            .collect()
    });
    Ok(Arc::new(DesktopRuntime::new(config)?))
}

/// 알림 저장소가 바뀔 때마다(데스크톱 전달·클릭, 모바일 읽음 처리 등 쓰기 주체와 상관없이) 전경에 알린다.
/// TS main은 `subscribeToNotificationChanges(broadcastNotificationsChanged)`로 모든 쓰기를 renderer에 보냈다
fn relay_notification_changes(
    runtime: &DesktopRuntime,
    sender: UnboundedSender<RuntimeEvent>,
) -> NotificationChangeSubscription {
    runtime
        .notification_store()
        .subscribe_to_notification_changes(move || {
            let _ = sender.unbounded_send(RuntimeEvent::NotificationsChanged);
        })
}

/// 보드 이벤트를 전경 스레드의 이벤트 버스로 넘긴다
fn relay_board_events(
    runtime: &DesktopRuntime,
    sender: UnboundedSender<RuntimeEvent>,
) -> BoardEventSubscription {
    runtime.board_notifier().subscribe(move |event| {
        let _ = sender.unbounded_send(RuntimeEvent::Board(event.clone()));
    })
}

fn install_qa_runtime(
    launch: &LaunchOptions,
    cx: &mut App,
) -> anyhow::Result<(
    Option<Arc<FaultRegistry>>,
    AppClock,
    Option<NetworkResponseTable>,
)> {
    let Some(qa) = launch.qa.clone() else {
        return Ok((None, AppClock::system(), None));
    };
    let faults = match qa.faults_path.as_deref() {
        Some(path) => FaultRegistry::load_file(path)?,
        None => FaultRegistry::default(),
    };
    let network_responses = match qa.network_responses_path.as_deref() {
        Some(path) => NetworkResponseTable::load_file(path)?,
        None => NetworkResponseTable::default(),
    };
    let clock = AppClock::fixed(qa.fixed_now_ms);
    cx.set_global(QaTargetRegistry::default());
    cx.set_global(QaRuntime {
        options: qa,
        network_responses: network_responses.clone(),
    });
    Ok((Some(Arc::new(faults)), clock, Some(network_responses)))
}
