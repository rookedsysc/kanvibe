//! 모든 서비스가 함께 쓰는 바닥: DB, 좁힌 자식 환경, 명령 실행기, 호스트 파일 접근, 보드 알림.

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::exec::remote::RemoteSshSettings;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::ShellPlatform;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_terminal::environment::TerminalHostEnvironment;

use crate::config::RuntimeConfig;
use crate::runtime::RuntimeError;

pub(crate) struct Foundation {
    pub database: Arc<Database>,
    pub environment: Arc<TerminalHostEnvironment>,
    /// SSH 전송 쿨다운 해제처럼 구체 타입이 필요한 자리에 쓴다. 나머지는 `runner`로 받는다
    pub system_runner: Arc<SystemShellCommandRunner>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub files: Arc<HostFileAccess>,
    pub ssh_config: Arc<SshConfigReader>,
    pub board: Arc<BoardNotifier>,
}

/// DB는 kanvibe-db가 TS와 같은 순서로 연다. 이미 적용된 마이그레이션은 `migrations` 표를 보고 다시 돌리지 않는다
pub(crate) fn build_foundation(config: &RuntimeConfig) -> Result<Foundation, RuntimeError> {
    let database = Arc::new(Database::open(&config.database_location)?);
    let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
        &config.parent_environment(),
        ShellPlatform::current(),
        &config.home_directory,
    ));
    let system_runner = Arc::new(SystemShellCommandRunner::new(
        environment.shell_environment.clone(),
        &config.home_directory,
        RemoteSshSettings::from_environment(&config.base_environment),
    ));
    let runner: Arc<dyn ShellCommandRunner> = system_runner.clone();
    let files = Arc::new(HostFileAccess::new(
        Arc::clone(&runner),
        config.home_directory.as_str(),
    ));
    Ok(Foundation {
        database,
        environment,
        system_runner,
        runner,
        files,
        ssh_config: Arc::new(SshConfigReader::new(&config.home_directory)),
        board: BoardNotifier::new(),
    })
}

/// 앱 수명 동안 살아 있어야 하는 빌린 참조를 만든다. kanvibe-process의 상태 파일 서비스와 전파 서비스는
/// 참조(`&'a`)로 만들어지는데, exclude 확인 캐시처럼 한 번 만든 값이 호출마다 사라지면 안 되는 상태를 품는다.
/// 런타임은 프로세스마다 하나이므로 그 주인을 의도적으로 놓아 두고(누수) 끝까지 쓴다.
/// 런타임을 여러 번 만드는 테스트에서는 런타임마다 작은 값 몇 개와 DB 연결 하나가 프로세스 끝까지 남는다
pub(crate) fn leak_for_process_lifetime<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}
