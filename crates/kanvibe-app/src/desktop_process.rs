//! 앱 프로세스의 UI 밖 수명(TS `electron/main.js`, `electron/runtimeEnvironment.js`).
//!
//! 실행 방식으로 데이터 디렉터리와 hook 포트를 고르고, 소스 개발 실행이면 개발 DB를 준비하고, 진단 로그를 열고,
//! 첫 창을 연 뒤 hook·task CLI·모바일 서버를 같은 프로세스에서 띄운다. 잠자기에서 깨어나면 hook 터널을 다시 잇고,
//! 종료(창을 모두 닫음, 종료 신호)할 때 서버·원격 agent·PTY를 TS `before-quit` 순서로 정리한다.
//!
//! 실행 방식 대응: `packaged` 기능으로 만든 배포 바이너리 = 설치된 앱(`app.isPackaged`), 디버그 빌드 = 소스 개발 실행
//! (`pnpm dev`, `KANVIBE_RENDERER_URL`), 기능 없이 만든 릴리스 빌드 = 소스로 띄운 수동 QA(`pnpm start`),
//! `--qa` = 수동 QA(TS Electron 스위치 `--qa`), `--qa-harness` = 자동 QA(`KANVIBE_QA_MODE=1`). 자동 QA는 TS 런처처럼
//! `--kanvibe-hook-port`를 줄 때만 서버를 띄운다(화면 캡처 실행은 포트를 주지 않아 서버 없이 뜬다).
//! 프로세스 환경은 시작할 때 한 번 읽기만 한다.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{App, AppContext as _, AsyncApp, Global};
use kanvibe_process::shell_environment::ShellEnvironment;
use kanvibe_runtime::development_database::{
    DevelopmentDatabaseOutcome, DevelopmentDatabaseRequest, INSTALLED_APP_DATA_DIRECTORY_NAME,
    InstanceLock, prepare_development_database,
};
use kanvibe_runtime::diagnostics::{DesktopDiagnostics, resolve_desktop_log_path};
use kanvibe_server::desktop::DesktopServices;
use kanvibe_server::desktop::in_process::{InProcessServer, InProcessServerListen};
use kanvibe_server::desktop::remote::{RemoteHostWiring, RemoteHostWiringConfig};
use kanvibe_server::launch::{
    HOOK_SERVER_PORT_SWITCH, LaunchError, LaunchMode, is_dev_pairing_route_enabled,
    resolve_app_data_directory, resolve_hook_server_port, resolve_remote_host_namespace,
};
use kanvibe_server::server::RouteOptions;
use serde_json::json;

use crate::qa::options::LaunchOptions;
use crate::qa::perf_markers::mark_startup_phase;
use crate::services::AppServices;

const DATA_SOURCE_SWITCH: &str = "data-source";
const APPLICATION_DATA_DIRECTORY_SWITCH: &str = "application-data-dir";
/// TS `app.commandLine.hasSwitch("qa")`: 설치된 앱이나 소스 실행을 수동 QA 데이터·포트로 띄운다
const MANUAL_QA_SWITCH: &str = "qa";

/// 이 실행 파일을 어떻게 빌드했는지(TS `app.isPackaged`와 `KANVIBE_RENDERER_URL`이 정하던 것)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppBuild {
    /// 패키지 스크립트가 `packaged` 기능을 켜고 만든 배포 바이너리(TS 설치된 앱)
    Packaged,
    /// 소스에서 기능 없이 만든 릴리스 빌드(TS `pnpm start`)
    SourceRelease,
    /// 소스 디버그 빌드(TS `pnpm dev`)
    SourceDebug,
}

impl AppBuild {
    /// 컴파일할 때 정해진 빌드. `packaged` 기능이 디버그 여부보다 앞선다
    pub const fn current() -> Self {
        if cfg!(feature = "packaged") {
            Self::Packaged
        } else if cfg!(debug_assertions) {
            Self::SourceDebug
        } else {
            Self::SourceRelease
        }
    }
}

/// `kanvibe` 명령줄 가운데 데스크톱 프로세스가 읽는 스위치(TS Electron 스위치와 같은 이름).
/// `--name value`와 Chromium 방식 `--name=value`를 모두 받는다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopLaunchArguments {
    /// `--kanvibe-hook-port`
    pub requested_hook_port: Option<String>,
    /// `--data-source application|existing`
    pub data_source: Option<String>,
    /// `--application-data-dir`
    pub application_data_directory: Option<PathBuf>,
    /// `--qa`(수동 QA)
    pub is_manual_qa: bool,
}

/// 데스크톱 스위치를 떼어 내고 나머지(화면·QA 인자)를 돌려준다
pub fn split_desktop_launch_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> (DesktopLaunchArguments, Vec<String>) {
    let mut desktop = DesktopLaunchArguments::default();
    let mut remaining = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let Some(switch) = argument.strip_prefix("--") else {
            remaining.push(argument);
            continue;
        };
        let (name, inline_value) = match switch.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (switch, None),
        };
        let slot = match name {
            MANUAL_QA_SWITCH => {
                desktop.is_manual_qa = true;
                continue;
            }
            HOOK_SERVER_PORT_SWITCH => &mut desktop.requested_hook_port,
            DATA_SOURCE_SWITCH => &mut desktop.data_source,
            APPLICATION_DATA_DIRECTORY_SWITCH => {
                let value = inline_value.or_else(|| arguments.next());
                desktop.application_data_directory = value.map(PathBuf::from);
                continue;
            }
            _ => {
                remaining.push(argument);
                continue;
            }
        };
        *slot = inline_value.or_else(|| arguments.next());
    }
    (desktop, remaining)
}

/// 이 실행이 쓸 데이터 디렉터리·포트·서버 여부
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopLaunchPlan {
    pub mode: LaunchMode,
    /// 이 바이너리가 어떻게 빌드됐는지. 자동 QA는 `mode`에서 설치된 앱으로 보지 않으므로 배포 바이너리 여부는 여기서 읽는다
    pub build: AppBuild,
    /// 없으면 DB 없이 뜬다(자동 QA가 데이터 디렉터리를 주지 않은 화면 캡처)
    pub data_dir: Option<PathBuf>,
    pub hook_port: u16,
    pub should_start_server: bool,
    /// OS 앱 데이터 루트(Electron `appData`)
    pub platform_app_data: PathBuf,
    pub arguments: DesktopLaunchArguments,
}

impl DesktopLaunchPlan {
    /// TS `runtimeEnvironment.js`와 같은 규칙으로 고른다. `--data-dir`은 TS `KANVIBE_APP_DATA_DIR`이다.
    /// 자동 QA는 TS처럼 늘 소스 실행으로 보므로 배포 바이너리여도 설치된 앱 데이터·포트·namespace를 쓰지 않는다
    pub fn resolve(
        build: AppBuild,
        launch: &LaunchOptions,
        arguments: DesktopLaunchArguments,
        environment: &ShellEnvironment,
        home_directory: &str,
    ) -> Result<Self, LaunchError> {
        let is_automated_qa = launch.qa.is_some();
        let mode = LaunchMode {
            is_packaged: build == AppBuild::Packaged && !is_automated_qa,
            is_manual_qa: arguments.is_manual_qa,
            is_dev_run: build == AppBuild::SourceDebug && !is_automated_qa,
            is_automated_qa,
            requested_app_data_dir: launch
                .data_dir
                .as_ref()
                .map(|data_dir| data_dir.display().to_string()),
            requested_hook_port: arguments.requested_hook_port.clone(),
        };
        let platform_app_data = platform_app_data_directory(environment, home_directory);
        let installed_user_data = platform_app_data.join(INSTALLED_APP_DATA_DIRECTORY_NAME);
        let data_dir =
            match resolve_app_data_directory(&mode, &platform_app_data, &installed_user_data) {
                Ok(data_dir) => Some(data_dir),
                Err(LaunchError::MissingQaDataDirectory) => None,
                Err(error) => return Err(error),
            };
        let hook_port = resolve_hook_server_port(&mode)?;
        let should_start_server =
            data_dir.is_some() && (!is_automated_qa || arguments.requested_hook_port.is_some());
        Ok(Self {
            mode,
            build,
            data_dir,
            hook_port,
            should_start_server,
            platform_app_data,
            arguments,
        })
    }

    /// TS `SHOULD_USE_SOURCE_MODULES && !app.isPackaged`: 배포 바이너리는 자동 QA로 띄워도 개발용 페어링 경로를 열지 않는다
    pub fn is_dev_pairing_route_enabled(&self) -> bool {
        self.build != AppBuild::Packaged && is_dev_pairing_route_enabled(&self.mode)
    }

    fn development_database_request(&self, data_dir: &Path) -> DevelopmentDatabaseRequest {
        DevelopmentDatabaseRequest {
            is_development: self.mode.is_dev_run,
            is_packaged: self.mode.is_packaged,
            is_qa: self.mode.is_automated_qa || self.mode.is_manual_qa,
            data_source: self.arguments.data_source.clone(),
            application_data_directory: self.arguments.application_data_directory.clone(),
            platform_app_data: self.platform_app_data.clone(),
            target_directory: data_dir.to_path_buf(),
        }
    }
}

/// Electron `app.getPath("appData")`: macOS `~/Library/Application Support`, 나머지는 `$XDG_CONFIG_HOME`(절대 경로일 때)
/// 또는 `~/.config`
pub fn platform_app_data_directory(
    environment: &ShellEnvironment,
    home_directory: &str,
) -> PathBuf {
    if cfg!(target_os = "macos") {
        return Path::new(home_directory).join("Library/Application Support");
    }
    environment
        .get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .unwrap_or_else(|| Path::new(home_directory).join(".config"))
}

/// 시작 단계에서 정한 것. 창을 연 뒤 [`start_desktop_process`]가 서버를 띄운다
pub struct DesktopProcess {
    diagnostics: Arc<DesktopDiagnostics>,
    /// 개발 실행 데이터 디렉터리 잠금. 프로세스가 끝날 때까지 쥔다
    _instance_lock: Option<InstanceLock>,
    pending_server: Option<(RemoteHostWiring, InProcessServerListen)>,
    running_server: Option<Arc<InProcessServer>>,
}

impl Global for DesktopProcess {}

impl DesktopProcess {
    pub fn diagnostics(&self) -> &Arc<DesktopDiagnostics> {
        &self.diagnostics
    }

    /// 프로세스 안에서 듣고 있는 서버. 띄우기 전이거나 포트를 잡지 못했으면 `None`이다
    pub fn running_server(&self) -> Option<&Arc<InProcessServer>> {
        self.running_server.as_ref()
    }
}

/// 런타임을 만들기 전 단계: 진단 로그, 개발 DB, 원격 호스트 조립. 실패하면 앱을 띄우지 않는다(TS `app.exit(1)`)
pub(crate) struct PreparedDesktopProcess {
    pub process: DesktopProcess,
    pub remote: Option<RemoteHostWiring>,
}

pub(crate) fn prepare_desktop_process(
    plan: &DesktopLaunchPlan,
    data_dir: &Path,
    environment: &ShellEnvironment,
    home_directory: &str,
) -> anyhow::Result<PreparedDesktopProcess> {
    let diagnostics = Arc::new(DesktopDiagnostics::new(
        resolve_desktop_log_path(data_dir),
        env!("CARGO_PKG_VERSION"),
    ));
    eprintln!(
        "[kanvibe] Desktop diagnostics log: {}",
        diagnostics.log_path().display()
    );
    install_panic_diagnostics(&diagnostics);
    let instance_lock = prepare_database(plan, data_dir, &diagnostics)?;
    diagnostics.log(
        "main:startup",
        json!({
            "userDataPath": data_dir,
            "cwd": std::env::current_dir().ok(),
            "isPackaged": plan.mode.is_packaged,
            "isDevelopment": plan.mode.is_dev_run,
            "hookServerPort": plan.hook_port,
        }),
    );
    let remote = match plan.should_start_server {
        true => Some(
            RemoteHostWiring::new(RemoteHostWiringConfig {
                home_directory: home_directory.to_string(),
                base_environment: environment.clone(),
                agent_binary_directory: std::env::current_exe()
                    .ok()
                    .and_then(|executable| bundled_agent_binary_directory(&executable)),
                namespace: resolve_remote_host_namespace(&plan.mode, data_dir),
            })
            .map_err(anyhow::Error::msg)?,
        ),
        false => None,
    };
    Ok(PreparedDesktopProcess {
        process: DesktopProcess {
            diagnostics,
            _instance_lock: instance_lock,
            pending_server: None,
            running_server: None,
        },
        remote,
    })
}

/// 패키지가 원격 agent 바이너리(`kanvibe-remote-agent-<OS>-<arch>`)를 넣은 디렉터리(`packaging/`이 만든다).
/// 실행 파일 위치로 찾고, 없으면(소스 빌드 실행) `None`이라 원격 호스트는 순수 ssh 경로로 간다
fn bundled_agent_binary_directory(executable: &Path) -> Option<PathBuf> {
    let executable_directory = executable.parent()?;
    [
        // macOS `KanVibe.app/Contents/MacOS/kanvibe` → `Contents/Resources/remote-agents`
        executable_directory.join("../Resources/remote-agents"),
        // Linux `<prefix>/bin/kanvibe` → `<prefix>/lib/kanvibe/remote-agents`(tarball, .deb)
        executable_directory.join("../lib/kanvibe/remote-agents"),
    ]
    .into_iter()
    .find(|directory| directory.is_dir())
}

/// 런타임과 모바일 서비스가 생긴 뒤 서버를 띄울 준비를 해 둔다
pub(crate) fn plan_server(
    process: &mut DesktopProcess,
    plan: &DesktopLaunchPlan,
    data_dir: &Path,
    remote: RemoteHostWiring,
) {
    let listen = InProcessServerListen {
        hook_port: plan.hook_port,
        tls_directory: data_dir.join("mobile-tls"),
        route_options: RouteOptions {
            is_dev_pairing_route_enabled: plan.is_dev_pairing_route_enabled(),
        },
    };
    process.pending_server = Some((remote, listen));
}

fn prepare_database(
    plan: &DesktopLaunchPlan,
    data_dir: &Path,
    diagnostics: &DesktopDiagnostics,
) -> anyhow::Result<Option<InstanceLock>> {
    let outcome = prepare_development_database(&plan.development_database_request(data_dir))
        .inspect_err(|error| {
            diagnostics.log(
                "main:development-database-failed",
                json!({ "error": error.to_string() }),
            );
            eprintln!("[kanvibe] Development database preparation failed: {error}");
        })?;
    if let Some(message) = outcome.log_message() {
        eprintln!("{message}");
    }
    Ok(match outcome {
        DevelopmentDatabaseOutcome::NotDevelopmentRun => None,
        DevelopmentDatabaseOutcome::KeptExisting(lock)
        | DevelopmentDatabaseOutcome::ApplicationDatabaseMissing { lock, .. }
        | DevelopmentDatabaseOutcome::Copied { lock, .. } => Some(lock),
    })
}

/// TS `process.on("uncaughtException")`. 패닉을 진단 로그에 남긴 뒤 기본 처리기에 넘긴다
fn install_panic_diagnostics(diagnostics: &Arc<DesktopDiagnostics>) {
    let diagnostics = Arc::clone(diagnostics);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        diagnostics.log(
            "main:uncaught-exception",
            json!({ "error": panic.to_string() }),
        );
        previous_hook(panic);
    }));
}

/// 첫 창을 연 뒤 부른다: 런타임 시작(백그라운드 sync, 보드 이벤트 알림), 서버 시작, 깨어남·종료 처리 등록
pub fn start_desktop_process(cx: &mut App) {
    let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
        return;
    };
    runtime.start();
    let services = cx.global::<AppServices>().desktop_services().cloned();
    start_pending_server(services, cx);
    cx.on_app_quit(move |cx| {
        // TS `before-quit`: 서버(터널·리스너), 푸시, sync 예약, pane 미러·원격 agent, PTY 순서로 닫는다.
        // 서버가 아직 뜨는 중이면 런타임만 닫는다
        let server = cx
            .try_global::<DesktopProcess>()
            .and_then(|process| process.running_server.clone());
        match server {
            Some(server) => server.stop(),
            None => runtime.shutdown(),
        }
        async {}
    })
    .detach();
}

/// 서버 시작(tokio 런타임, 처음 실행이면 모바일 TLS RSA 키 생성, 리스너, hook 터널)은 수십~수백 ms가 걸린다.
/// 전경 스레드에서 하면 첫 화면이 그만큼 늦으므로 백그라운드 스레드에서 띄우고, 다 뜨면 전경에서 등록한다
/// (TS도 렌더러가 첫 화면을 그리는 동안 main 프로세스에서 서버를 띄웠다)
fn start_pending_server(services: Option<Arc<DesktopServices>>, cx: &mut App) {
    let Some(process) = cx.try_global::<DesktopProcess>() else {
        return;
    };
    let diagnostics = Arc::clone(&process.diagnostics);
    let Some(services) = services else {
        return;
    };
    let Some((remote, listen)) = cx.global_mut::<DesktopProcess>().pending_server.take() else {
        return;
    };
    let starting =
        cx.background_spawn(async move { InProcessServer::start(services, remote, &listen) });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let started = starting.await;
        cx.update(|cx| register_started_server(started, &diagnostics, cx));
    })
    .detach();
}

fn register_started_server(
    started: Result<InProcessServer, String>,
    diagnostics: &DesktopDiagnostics,
    cx: &mut App,
) {
    match started {
        Ok(server) => {
            let server = Arc::new(server);
            diagnostics.log(
                "main:hook-server-listening",
                json!({ "address": server.plaintext_address().to_string(), "tls": server.tls_address().map(|address| address.to_string()) }),
            );
            watch_system_wake(&server, cx);
            watch_termination_signal(&server, cx);
            cx.global_mut::<DesktopProcess>().running_server = Some(server);
            mark_startup_phase("hook_server_registered");
        }
        Err(error) => {
            // TS도 리스너 오류를 기록만 하고 hook 서버 없이 화면을 계속 띄웠다
            eprintln!("[kanvibe] {error}");
            diagnostics.log("main:hook-server-failed", json!({ "error": error }));
        }
    }
}

/// 잠자기에서 깨면 hook 터널을 다시 잇는다(TS `powerMonitor.on("resume")`). GPUI가 Linux에서는 logind
/// `PrepareForSleep(false)` D-Bus 신호로, macOS에서는 `NSWorkspaceDidWakeNotification`으로 알려 준다
fn watch_system_wake(server: &Arc<InProcessServer>, cx: &mut App) {
    let server = Arc::clone(server);
    cx.on_system_wake(move |_| server.reconnect_hook_tunnels())
        .detach();
}

/// SIGTERM·SIGINT를 정상 종료(`cx.quit()` → `on_app_quit`)로 바꾼다
fn watch_termination_signal(server: &Arc<InProcessServer>, cx: &mut App) {
    let (sender, mut receiver) = unbounded::<()>();
    server.on_termination_signal(move || {
        let _ = sender.unbounded_send(());
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        if receiver.next().await.is_some() {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qa::options::parse_launch_options;

    fn arguments(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    const HARNESS: &str = "--qa-harness --fixed-now 2026-01-15T03:00:00Z";

    fn plan(line: &str, environment: &[(&str, &str)]) -> Result<DesktopLaunchPlan, LaunchError> {
        plan_for_build(AppBuild::current(), line, environment)
    }

    #[test]
    fn automated_qa_opens_the_dev_pairing_route_only_on_source_builds() {
        let line = format!("{HARNESS} --data-dir /d --kanvibe-hook-port 41234");
        for (build, is_expected_open) in [
            (AppBuild::SourceDebug, true),
            (AppBuild::SourceRelease, true),
            (AppBuild::Packaged, false),
        ] {
            let plan = plan_for_build(build, &line, &[]).unwrap();

            assert_eq!(
                plan.is_dev_pairing_route_enabled(),
                is_expected_open,
                "{build:?}"
            );
        }
    }

    fn plan_for_build(
        build: AppBuild,
        line: &str,
        environment: &[(&str, &str)],
    ) -> Result<DesktopLaunchPlan, LaunchError> {
        let (desktop, remaining) = split_desktop_launch_arguments(arguments(line));
        let launch = parse_launch_options(remaining).unwrap();
        let environment: ShellEnvironment = environment
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        DesktopLaunchPlan::resolve(build, &launch, desktop, &environment, "/home/user")
    }

    #[test]
    fn desktop_switches_accept_both_electron_forms_and_leave_the_rest() {
        let (desktop, remaining) = split_desktop_launch_arguments(arguments(
            "--kanvibe-hook-port=41234 --qa --data-dir /d --data-source application --application-data-dir=/app --route /ko",
        ));

        assert_eq!(desktop.requested_hook_port.as_deref(), Some("41234"));
        assert!(desktop.is_manual_qa);
        assert_eq!(desktop.data_source.as_deref(), Some("application"));
        assert_eq!(
            desktop.application_data_directory,
            Some(PathBuf::from("/app"))
        );
        assert_eq!(remaining, arguments("--data-dir /d --route /ko"));
    }

    #[test]
    fn the_packaged_feature_decides_the_installed_build_before_the_debug_profile() {
        let expected = match (cfg!(feature = "packaged"), cfg!(debug_assertions)) {
            (true, _) => AppBuild::Packaged,
            (false, true) => AppBuild::SourceDebug,
            (false, false) => AppBuild::SourceRelease,
        };

        assert_eq!(AppBuild::current(), expected);
    }

    #[test]
    fn each_build_and_the_qa_switch_get_their_own_data_directory_port_and_namespace() {
        // TS 설치된 앱, `pnpm dev`, `pnpm start`, 설치된 앱 `--qa`, 소스 실행 `--qa` 순서다
        let cases = [
            (AppBuild::Packaged, "", "KanVibe", 9736, "prod", false),
            (AppBuild::SourceDebug, "", "kanvibe-dev", 19736, "dev", true),
            (
                AppBuild::SourceRelease,
                "",
                "kanvibe-qa",
                29736,
                "qa-",
                false,
            ),
            (
                AppBuild::Packaged,
                "--qa",
                "kanvibe-qa",
                29736,
                "qa-",
                false,
            ),
            (
                AppBuild::SourceDebug,
                "--qa",
                "kanvibe-qa",
                29736,
                "qa-",
                true,
            ),
            (
                AppBuild::SourceRelease,
                "--qa",
                "kanvibe-qa",
                29736,
                "qa-",
                false,
            ),
        ];
        for (build, line, directory, port, namespace, is_pairing_route_enabled) in cases {
            let plan = plan_for_build(build, line, &[("XDG_CONFIG_HOME", "/xdg")]).unwrap();
            let data_dir = plan.data_dir.clone().unwrap();

            let case = format!("{build:?} {line:?}");
            assert_eq!(data_dir, Path::new("/xdg").join(directory), "{case}");
            assert_eq!(plan.hook_port, port, "{case}");
            assert!(plan.should_start_server, "{case}");
            assert!(
                resolve_remote_host_namespace(&plan.mode, &data_dir).starts_with(namespace),
                "{case}"
            );
            assert_eq!(
                plan.is_dev_pairing_route_enabled(),
                is_pairing_route_enabled,
                "{case}"
            );
        }
    }

    #[test]
    fn manual_qa_on_the_installed_app_reuses_its_directory_across_launches() {
        let first = plan_for_build(AppBuild::Packaged, "--qa", &[]).unwrap();
        let second = plan_for_build(AppBuild::Packaged, "--qa", &[]).unwrap();

        assert_eq!(first.data_dir, second.data_dir);
        assert_eq!(
            first.data_dir,
            Some(PathBuf::from("/home/user/.config/kanvibe-qa"))
        );
    }

    #[test]
    fn only_a_debug_build_without_qa_prepares_the_development_database() {
        let cases = [
            (AppBuild::SourceDebug, "", true),
            (AppBuild::SourceDebug, "--qa", false),
            (AppBuild::SourceRelease, "", false),
            (AppBuild::Packaged, "", false),
        ];
        for (build, line, is_prepared) in cases {
            let app_data = tempfile::tempdir().unwrap();
            let xdg = app_data.path().to_str().unwrap();
            let plan = plan_for_build(build, line, &[("XDG_CONFIG_HOME", xdg)]).unwrap();
            let request = plan.development_database_request(&plan.data_dir.clone().unwrap());

            let outcome = prepare_development_database(&request).unwrap();

            let is_development_run =
                !matches!(outcome, DevelopmentDatabaseOutcome::NotDevelopmentRun);
            assert_eq!(is_development_run, is_prepared, "{build:?} {line:?}");
        }
    }

    #[test]
    fn the_qa_harness_never_counts_as_the_installed_app() {
        let plan = plan_for_build(
            AppBuild::Packaged,
            &format!("{HARNESS} --data-dir /fixture --kanvibe-hook-port 41234"),
            &[],
        )
        .unwrap();

        assert!(!plan.mode.is_packaged);
        assert_eq!(plan.data_dir, Some(PathBuf::from("/fixture")));
        assert_eq!(plan.hook_port, 41234);
        assert!(
            resolve_remote_host_namespace(&plan.mode, Path::new("/fixture")).starts_with("qa-")
        );
    }

    #[test]
    fn a_relative_xdg_config_home_falls_back_to_dot_config() {
        let plan = plan("--data-dir /d", &[("XDG_CONFIG_HOME", "relative")]).unwrap();

        assert_eq!(plan.platform_app_data, Path::new("/home/user/.config"));
        assert_eq!(plan.data_dir, Some(PathBuf::from("/d")));
    }

    #[test]
    fn automated_qa_starts_the_server_only_on_a_launcher_port() {
        let capture = plan(&format!("{HARNESS} --data-dir /d"), &[]).unwrap();
        let without_data = plan(HARNESS, &[]).unwrap();
        let launched = plan(
            &format!("{HARNESS} --data-dir /d --kanvibe-hook-port 41234"),
            &[],
        )
        .unwrap();

        assert!(!capture.should_start_server);
        assert_eq!(without_data.data_dir, None);
        assert!(!without_data.should_start_server);
        assert!(launched.should_start_server);
        assert_eq!(launched.hook_port, 41234);
    }

    #[test]
    fn bundled_agent_binaries_are_found_next_to_the_packaged_executable() {
        let root = tempfile::tempdir().unwrap();
        let mac_agents = root
            .path()
            .join("KanVibe.app/Contents/Resources/remote-agents");
        let linux_agents = root.path().join("usr/lib/kanvibe/remote-agents");
        // 실행 파일 디렉터리도 있어야 `..`이 풀린다(실제 번들처럼)
        for directory in [
            mac_agents.clone(),
            linux_agents.clone(),
            root.path().join("KanVibe.app/Contents/MacOS"),
            root.path().join("usr/bin"),
            root.path().join("target/release"),
        ] {
            std::fs::create_dir_all(directory).unwrap();
        }

        let mac =
            bundled_agent_binary_directory(&root.path().join("KanVibe.app/Contents/MacOS/kanvibe"));
        let linux = bundled_agent_binary_directory(&root.path().join("usr/bin/kanvibe"));
        let source_build =
            bundled_agent_binary_directory(&root.path().join("target/release/kanvibe"));

        assert_eq!(
            mac.map(|directory| directory.canonicalize().unwrap()),
            Some(mac_agents.canonicalize().unwrap())
        );
        assert_eq!(
            linux.map(|directory| directory.canonicalize().unwrap()),
            Some(linux_agents.canonicalize().unwrap())
        );
        assert_eq!(source_build, None);
    }

    #[test]
    fn a_qa_data_directory_wins_over_inherited_production_paths() {
        let inherited = [
            ("KANVIBE_APP_DATA_DIR", "/production/app-data"),
            ("KANVIBE_DB_PATH", "/production/kanvibe.db"),
        ];

        let qa = plan(&format!("{HARNESS} --data-dir /fixture"), &inherited).unwrap();

        // DB는 늘 데이터 디렉터리 안 `kanvibe.db`라 앱 데이터와 DB가 같은 fixture를 쓴다
        assert_eq!(qa.data_dir, Some(PathBuf::from("/fixture")));
    }

    #[test]
    fn startup_and_panics_are_written_to_the_diagnostics_log() {
        let data_dir = tempfile::tempdir().unwrap();
        let plan = plan(
            &format!("{HARNESS} --data-dir {}", data_dir.path().display()),
            &[],
        )
        .unwrap();

        let prepared = prepare_desktop_process(
            &plan,
            data_dir.path(),
            &ShellEnvironment::new(),
            "/home/user",
        )
        .unwrap();
        let _ = std::panic::catch_unwind(|| panic!("native module failed"));

        let log = std::fs::read_to_string(prepared.process.diagnostics().log_path()).unwrap();
        assert!(log.contains("] main:startup "), "{log}");
        assert!(log.contains(&format!(r#""hookServerPort":{}"#, plan.hook_port)));
        assert!(log.contains("] main:uncaught-exception "), "{log}");
        assert!(log.contains("native module failed"));
    }

    #[test]
    fn an_invalid_launcher_port_is_refused() {
        let error = plan("--kanvibe-hook-port 70000", &[]).unwrap_err();

        assert!(error.to_string().contains("--kanvibe-hook-port"));
    }
}
