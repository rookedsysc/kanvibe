use kanvibe_app::app_window::open_app_window;
use kanvibe_app::assets::KanvibeAssets;
use kanvibe_app::qa::control::start_control_server;
use kanvibe_app::qa::options::parse_launch_options;

fn main() {
    let launch = match parse_launch_options(std::env::args().skip(1)) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("kanvibe: {error}");
            std::process::exit(2);
        }
    };

    gpui_kit::application()
        .with_assets(KanvibeAssets::new())
        .run(move |cx| {
            if let Err(error) = start_app(&launch, cx) {
                eprintln!("kanvibe: {error:#}");
                cx.quit();
            }
        });
}

fn start_app(
    launch: &kanvibe_app::qa::options::LaunchOptions,
    cx: &mut gpui_kit::App,
) -> anyhow::Result<()> {
    kanvibe_app::init_app(launch, cx)?;
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
