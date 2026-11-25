use kanvibe_app::app_window::open_app_window;
use kanvibe_app::assets::KanvibeAssets;
use kanvibe_app::desktop_process::{DesktopLaunchPlan, split_desktop_launch_arguments};
use kanvibe_app::qa::control::start_control_server;
use kanvibe_app::qa::options::{LaunchOptions, parse_launch_options};
use kanvibe_process::shell_environment::current_user_home_directory;

fn main() {
    let (desktop_arguments, arguments) = split_desktop_launch_arguments(std::env::args().skip(1));
    let launch = parse_launch_options(arguments).map_err(|error| error.to_string());
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

    let exit_code = std::rc::Rc::new(std::cell::Cell::new(0));
    let failed_exit_code = exit_code.clone();
    gpui_kit::application()
        .with_assets(KanvibeAssets::new())
        .run(move |cx| {
            if let Err(error) = start_app(&launch, &plan, cx) {
                eprintln!("kanvibe: {error:#}");
                failed_exit_code.set(1);
                cx.quit();
            }
        });
    std::process::exit(exit_code.get());
}

fn start_app(
    launch: &LaunchOptions,
    plan: &DesktopLaunchPlan,
    cx: &mut gpui_kit::App,
) -> anyhow::Result<()> {
    kanvibe_app::init_desktop_app(launch, plan, cx)?;
    open_app_window(&launch.initial_route, launch.window_size, cx)?;
    if let Some(socket_path) = launch
        .qa
        .as_ref()
        .and_then(|qa| qa.control_socket_path.as_deref())
    {
        start_control_server(socket_path, cx)?;
    }
    kanvibe_app::start_runtime_services(cx);
    cx.activate(true);
    Ok(())
}
