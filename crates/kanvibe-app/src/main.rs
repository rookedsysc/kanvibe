use kanvibe_app::app_window::open_app_window;
use kanvibe_app::assets::KanvibeAssets;
use kanvibe_app::desktop_process::{DesktopLaunchPlan, split_desktop_launch_arguments};
use kanvibe_app::qa::control::start_control_server;
use kanvibe_app::qa::options::{LaunchOptions, parse_launch_options};
use kanvibe_app::qa::perf_markers::{
    enable_startup_timeline, install_perf_markers, mark_startup_phase, require_qa_for_perf_markers,
    split_perf_markers_switch,
};
use kanvibe_process::shell_environment::current_user_home_directory;

fn main() {
    let (desktop_arguments, arguments) = split_desktop_launch_arguments(std::env::args().skip(1));
    let (is_perf_markers_enabled, arguments) = split_perf_markers_switch(arguments);
    let launch = parse_launch_options(arguments)
        .map_err(|error| error.to_string())
        .and_then(|launch| require_qa_for_perf_markers(launch, is_perf_markers_enabled));
    let environment: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let plan = launch.as_ref().map_err(Clone::clone).and_then(|launch| {
        DesktopLaunchPlan::resolve(
            launch,
            desktop_arguments,
            &environment,
            &current_user_home_directory(),
        )
        .map_err(|error| error.to_string())
    });
    let (launch, plan) = match (launch, plan) {
        (Ok(launch), Ok(plan)) => (launch, plan),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("kanvibe: {error}");
            std::process::exit(2);
        }
    };

    if is_perf_markers_enabled {
        enable_startup_timeline();
        mark_startup_phase("main");
    }
    let exit_code = std::rc::Rc::new(std::cell::Cell::new(0));
    let failed_exit_code = exit_code.clone();
    desktop_application()
        .with_assets(KanvibeAssets::new())
        .run(move |cx| {
            mark_startup_phase("platform_ready");
            if let Err(error) = start_app(&launch, &plan, is_perf_markers_enabled, cx) {
                eprintln!("kanvibe: {error:#}");
                failed_exit_code.set(1);
                cx.quit();
            }
        });
    std::process::exit(exit_code.get());
}

/// TS(Electron 32)는 Linux에서 Wayland 세션이어도 기본으로 X11(XWayland)로 떴다. HTML 미리보기 WebView(WebKitGTK)도
/// X11 창에서만 붙으므로 X 디스플레이가 있으면 X11만 허용한다. 환경 값은 읽기만 하고 바꾸지 않는다
#[cfg(target_os = "linux")]
fn desktop_application() -> gpui_kit::Application {
    use gpui_kit::{GraphicalEnvironment, WindowingModes};
    let has_x11_display = GraphicalEnvironment::detect()
        .x11_display
        .is_some_and(|display| !display.is_empty());
    let allowed_modes = match has_x11_display {
        true => WindowingModes::X11 | WindowingModes::HEADLESS,
        false => WindowingModes::all(),
    };
    gpui_kit::platform::linux(allowed_modes)
}

#[cfg(not(target_os = "linux"))]
fn desktop_application() -> gpui_kit::Application {
    gpui_kit::application()
}

fn start_app(
    launch: &LaunchOptions,
    plan: &DesktopLaunchPlan,
    is_perf_markers_enabled: bool,
    cx: &mut gpui_kit::App,
) -> anyhow::Result<()> {
    kanvibe_app::init_desktop_app(launch, plan, cx)?;
    let window_handle = open_app_window(&launch.initial_route, launch.window_size, cx)?;
    mark_startup_phase("window_opened");
    if is_perf_markers_enabled {
        install_perf_markers(window_handle, cx)?;
    }
    if let Some(socket_path) = launch
        .qa
        .as_ref()
        .and_then(|qa| qa.control_socket_path.as_deref())
    {
        start_control_server(socket_path, cx)?;
    }
    kanvibe_app::start_runtime_services(cx);
    mark_startup_phase("runtime_services_started");
    cx.activate(true);
    Ok(())
}
