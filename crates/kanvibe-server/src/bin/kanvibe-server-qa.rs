//! 계약 재생기가 실제 Rust 서버를 두드리도록 앱과 같은 조립으로 hook·모바일 서버만 띄우는 QA 실행기.
//!
//! `serve`는 데이터 디렉터리와 hook 포트를 받아 런타임·모바일 서비스·서버를 띄우고, 데스크탑 화면만 할 수 있는 일
//! (페어링 코드, hook 설치 표)을 0600 Unix 소켓의 오라클로 연다. `oracle`은 그 소켓에 한 번 묻고 JSON을 출력한다.
//! 프로세스 환경은 시작할 때 한 번 읽어 자식 환경의 바탕으로만 넘기고 바꾸지 않는다.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use kanvibe_process::hooks::HookProviderFile;
use kanvibe_runtime::{DEFAULT_NOTIFICATION_LOCALE, DesktopRuntime, RuntimeConfig};
use kanvibe_server::desktop::in_process::{
    InProcessServer, InProcessServerListen, wait_for_termination_signal,
};
use kanvibe_server::desktop::remote::{RemoteHostWiring, RemoteHostWiringConfig};
use kanvibe_server::desktop::{DesktopServerConfig, DesktopServices, push_relay_config};
use kanvibe_server::launch::qa_remote_host_namespace;
use kanvibe_server::qa_oracle::{OracleHandler, ask_oracle, serve_oracle_socket};
use kanvibe_server::server::{MOBILE_TLS_PORT_OFFSET, RouteOptions};
use serde_json::{Value, json};

/// 다이어그램 렌더러가 읽는 글꼴. 앱 번들 글꼴과 같은 파일이다
const BUNDLED_DIAGRAM_FONT_DIRECTORY: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../kanvibe-app/assets/fonts");

const USAGE: &str = "usage: kanvibe-server-qa serve --data-dir <dir> --hook-port <port> --oracle-socket <path> [--agent-binaries <dir>] [--diagram-fonts <dir>]\n       kanvibe-server-qa oracle --socket <path> <name> <json-args>";

struct ServeOptions {
    data_dir: PathBuf,
    hook_port: u16,
    oracle_socket: PathBuf,
    /// 원격 agent 바이너리 디렉터리. 없으면 빈 목록이라 원격 호스트는 순수 ssh 경로로 간다
    agent_binary_directory: Option<PathBuf>,
    /// 다이어그램 글꼴 디렉터리(`*.ttf`, `*.otf`). 기본값은 앱 번들 글꼴 디렉터리다
    diagram_font_directory: PathBuf,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.first().map(String::as_str) {
        Some("serve") => parse_serve_options(&arguments[1..]).and_then(serve),
        Some("oracle") => run_oracle_command(&arguments[1..]),
        _ => Err(USAGE.to_string()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn option_value<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    let index = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("{name} is required\n{USAGE}"))?;
    arguments
        .get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{name} needs a value"))
}

/// 이름이 없으면 `None`, 이름 뒤에 값이 없으면 오류다
fn optional_option_value<'a>(
    arguments: &'a [String],
    name: &str,
) -> Result<Option<&'a str>, String> {
    let is_present = arguments.iter().any(|argument| argument == name);
    if !is_present {
        return Ok(None);
    }
    option_value(arguments, name).map(Some)
}

fn parse_serve_options(arguments: &[String]) -> Result<ServeOptions, String> {
    Ok(ServeOptions {
        data_dir: PathBuf::from(option_value(arguments, "--data-dir")?),
        hook_port: option_value(arguments, "--hook-port")?
            .parse()
            .map_err(|_| "invalid --hook-port".to_string())?,
        oracle_socket: PathBuf::from(option_value(arguments, "--oracle-socket")?),
        agent_binary_directory: optional_option_value(arguments, "--agent-binaries")?
            .map(PathBuf::from),
        diagram_font_directory: PathBuf::from(
            optional_option_value(arguments, "--diagram-fonts")?
                .unwrap_or(BUNDLED_DIAGRAM_FONT_DIRECTORY),
        ),
    })
}

fn run_oracle_command(arguments: &[String]) -> Result<(), String> {
    let socket = option_value(arguments, "--socket")?;
    let rest: Vec<&String> = arguments
        .iter()
        .skip_while(|argument| *argument != "--socket")
        .skip(2)
        .collect();
    let name = rest.first().ok_or(USAGE)?;
    let args: Value = rest
        .get(1)
        .map(|text| serde_json::from_str(text))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(json!({}));
    let value = ask_oracle(Path::new(socket), name, &args)?;
    println!("{value}");
    Ok(())
}

fn serve(options: ServeOptions) -> Result<(), String> {
    let environment: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let home_directory = environment.get("HOME").cloned().ok_or("HOME is not set")?;
    let remote = RemoteHostWiring::new(RemoteHostWiringConfig {
        home_directory: home_directory.clone(),
        base_environment: environment.clone(),
        agent_binary_directory: options.agent_binary_directory.clone(),
        namespace: qa_remote_host_namespace(&options.data_dir),
    })?;
    let mut config = RuntimeConfig::new(
        &options.data_dir,
        options.hook_port,
        home_directory.clone(),
        environment.clone(),
    );
    remote.wire_runtime(&mut config);
    // 골든을 녹화한 TS 앱은 첫 창을 기본 경로(`/ko`)에 열어 두었고, 알림 문구는 그 창의 locale을 따랐다
    config.notification_locale = Arc::new(|| DEFAULT_NOTIFICATION_LOCALE.to_string());
    let font_directory = options.diagram_font_directory.clone();
    config.diagram_font_files = Arc::new(move || read_font_files(&font_directory));
    let runtime = Arc::new(DesktopRuntime::new(config).map_err(|error| error.to_string())?);
    let services = Arc::new(DesktopServices::new(
        runtime.clone(),
        DesktopServerConfig {
            home_directory: PathBuf::from(&home_directory),
            mobile_tls_port: options.hook_port + MOBILE_TLS_PORT_OFFSET,
            push_relay: push_relay_config(&environment),
        },
    ));
    runtime.start();
    serve_oracle_socket(&options.oracle_socket, oracle_handler(&services))
        .map_err(|error| error.to_string())?;
    let server = InProcessServer::start(
        services,
        remote,
        &InProcessServerListen {
            hook_port: options.hook_port,
            tls_directory: options.data_dir.join("mobile-tls"),
            route_options: RouteOptions::default(),
        },
    )?;
    eprintln!(
        "[kanvibe-server-qa] listening on {} (tls {:?})",
        server.plaintext_address(),
        server.tls_address()
    );
    server.block_on(wait_for_termination_signal());
    server.stop();
    Ok(())
}

/// 글꼴 디렉터리의 `*.ttf`·`*.otf`를 이름순으로 읽는다. 읽지 못한 파일은 건너뛴다
fn read_font_files(directory: &Path) -> Vec<Vec<u8>> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        eprintln!(
            "[kanvibe-server-qa] diagram font directory missing: {}",
            directory.display()
        );
        return Vec::new();
    };
    let mut font_paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "ttf" || extension == "otf")
        })
        .collect();
    font_paths.sort();
    font_paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .collect()
}

fn oracle_handler(services: &Arc<DesktopServices>) -> OracleHandler {
    let services = services.clone();
    Arc::new(move |name, args| match name {
        "issue_pairing_code" => serde_json::to_value(services.devices.start_mobile_pairing())
            .map_err(|error| error.to_string()),
        "issue_hook_install_ticket" => issue_hook_install_ticket(&services, args),
        _ => Err(format!("unknown oracle {name}")),
    })
}

/// README의 고정 입력(`/tmp/kanvibe-contract-hook/status.sh`)이 기본이고, 인자로 `files`를 주면 그것을 쓴다
fn issue_hook_install_ticket(services: &DesktopServices, args: &Value) -> Result<Value, String> {
    let files = match args.get("files").and_then(Value::as_array) {
        Some(files) => files
            .iter()
            .map(read_provider_file)
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![HookProviderFile {
            file_path: "/tmp/kanvibe-contract-hook/status.sh".to_string(),
            content: "echo kanvibe-contract\n".to_string(),
            mode: Some(0o755),
        }],
    };
    let ticket = services.runtime.hook_install_scripts().issue(&files);
    Ok(json!({ "token": ticket.token, "scriptSha256": ticket.script_sha256 }))
}

fn read_provider_file(value: &Value) -> Result<HookProviderFile, String> {
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or(format!("files[].{name} is required"))
    };
    Ok(HookProviderFile {
        file_path: text("filePath")?,
        content: text("content")?,
        mode: value
            .get("mode")
            .and_then(Value::as_u64)
            .map(|mode| mode as u32),
    })
}
