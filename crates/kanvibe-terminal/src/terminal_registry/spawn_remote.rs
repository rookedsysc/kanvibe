//! 원격 세션의 PTY를 새로 띄우고 첫 클라이언트를 붙인다(TS `spawnRemoteSession`).
//! daemon 연결이 있으면 Herdr가 아닌 세션은 daemon PTY를 쓰고, 없으면 시스템 `ssh` PTY를 띄운다.

use std::sync::Arc;

use kanvibe_domain::task::SessionType;
use kanvibe_process::ssh_config::{
    SshArgsOptions, SshHostConfig, build_ssh_args, has_local_x11_display,
    kanvibe_ssh_connection_health_options,
};
use kanvibe_process::worktree::quote_for_posix_shell;

use super::attach::EntryBlueprint;
use super::herdr_client::HerdrTaskClient;
use super::remote_commands::{build_remote_attach_command, build_remote_shell_command};
use super::{
    AttachResult, DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS, RemoteAttachRequest,
    RemoteHostConnection, TerminalClient, TerminalRegistry, build_remote_pty_key,
};
use crate::environment::TerminalHostEnvironment;
use crate::herdr::{build_herdr_client_command, parse_herdr_session_target};
use crate::pty::PtyProcess;

/// 원격 서버 준비 결과. Herdr 세션이면 준비한 Space ID가 있다
struct RemoteServer {
    herdr_workspace_id: Option<String>,
}

impl TerminalRegistry {
    pub(super) fn spawn_remote_session(
        self: &Arc<Self>,
        request: &RemoteAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> AttachResult {
        let terminal_key = super::build_terminal_key(&request.task_id, request.tab_id.as_deref());
        if request.session_type != SessionType::Herdr
            && let Some(connection) = self
                .dependencies
                .remote_hosts
                .connection(&request.ssh_host)?
        {
            return self.spawn_host_daemon_session(
                &terminal_key,
                request,
                connection.as_ref(),
                client,
            );
        }
        self.spawn_remote_ssh_session(&terminal_key, request, client)
    }

    /// 시스템 `ssh` PTY를 띄운다. 공유 Herdr Space는 ssh 포워딩이나 daemon 소켓을 거쳐 Herdr client를 띄운다
    fn spawn_remote_ssh_session(
        self: &Arc<Self>,
        terminal_key: &str,
        request: &RemoteAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> AttachResult {
        let Some(server) = self.prepare_remote_server(request, client)? else {
            return Ok(None);
        };
        let ssh_args = self.build_remote_attach_ssh_args(request)?;
        if let Some(attachment) = self.attach_if_connected_during_bootstrap(terminal_key, client) {
            return Ok(attachment);
        }
        let is_herdr = request.session_type == SessionType::Herdr;
        let blueprint = EntryBlueprint::new(
            request.session_type,
            &request.session_name,
            &request.task_id,
            request.tab_id.as_deref(),
            is_herdr.then_some(request.ssh_host.as_str()),
        );
        let Some(blueprint) = self.prepare_entry_blueprint(
            blueprint,
            server.herdr_workspace_id,
            Some(&request.ssh_host),
            client,
        )?
        else {
            return Ok(None);
        };
        let (program, args) = self
            .build_remote_launch(&blueprint, &request.session_name, ssh_args)
            .inspect_err(|_| blueprint.discard_herdr_client())?;
        let home_directory = self.dependencies.environment.home_directory.clone();
        let spawn_request = self.build_spawn_request(
            program,
            args,
            home_directory,
            (request.columns, request.rows),
        );
        self.spawn_and_register(
            terminal_key,
            blueprint,
            spawn_request,
            client,
            "SSH 연결 실패",
            "Remote",
        )
    }

    /// Herdr면 원격 Space를 준비한다. 준비하는 사이 클라이언트가 떠나면 `Ok(None)`이다
    fn prepare_remote_server(
        &self,
        request: &RemoteAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<RemoteServer>, String> {
        let server = match request.session_type {
            SessionType::Herdr => self.prepare_remote_herdr_server(request, client)?,
            _ => Some(RemoteServer {
                herdr_workspace_id: None,
            }),
        };
        Ok(server.filter(|_| client.is_open()))
    }

    /// 원격 attach 명령을 마지막 인자로 단 ssh 인자
    fn build_remote_attach_ssh_args(
        &self,
        request: &RemoteAttachRequest,
    ) -> Result<Vec<String>, String> {
        let attach_command = build_remote_attach_command(
            request.session_type,
            &request.session_name,
            request.worktree_path.as_deref(),
            request.pane_layout.as_ref(),
        )?;
        let mut ssh_args =
            build_remote_ssh_args(&self.dependencies.environment, &request.ssh_config);
        ssh_args.push(build_remote_shell_command(&attach_command));
        self.log_remote_spawn(request, &ssh_args, &attach_command);
        Ok(ssh_args)
    }

    /// 띄울 프로세스와 인자. 공유 Space는 Herdr client를, 그 밖에는 ssh를 띄운다
    fn build_remote_launch(
        &self,
        blueprint: &EntryBlueprint,
        session_name: &str,
        ssh_args: Vec<String>,
    ) -> Result<(String, Vec<String>), String> {
        match &blueprint.herdr_client {
            Some(herdr_client) => build_herdr_client_launch(
                &self.dependencies.environment,
                session_name,
                herdr_client,
                ssh_args,
            ),
            None => Ok(("ssh".to_string(), ssh_args)),
        }
    }

    /// 서버 교체가 진행 중이면 끝나기를 기다린 뒤 원격 Space를 준비하고, machine 등록은 연결을 막지 않도록 뒤에서 시작한다
    fn prepare_remote_herdr_server(
        &self,
        request: &RemoteAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<RemoteServer>, String> {
        let target = parse_herdr_session_target(&request.session_name)?;
        self.wait_for_herdr_server_handoff(&request.ssh_host, &target.server_name);
        let working_directory = request
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or("~");
        let workspace_id = self
            .dependencies
            .herdr
            .ensure_session(
                &request.session_name,
                working_directory,
                request.pane_layout.as_ref(),
                Some(&request.ssh_host),
            )
            .map_err(|error| error.to_string())?;
        if !client.is_open() {
            return Ok(None);
        }
        self.start_herdr_machine_registration(&target.server_name, &request.ssh_host);
        Ok(Some(RemoteServer {
            herdr_workspace_id: workspace_id,
        }))
    }

    /// daemon이 소유하는 PTY를 연다. 창이 닫혀도 세션은 daemon에 남는다
    fn spawn_host_daemon_session(
        self: &Arc<Self>,
        terminal_key: &str,
        request: &RemoteAttachRequest,
        connection: &dyn RemoteHostConnection,
        client: &Arc<dyn TerminalClient>,
    ) -> AttachResult {
        let command = build_remote_attach_command(
            request.session_type,
            &request.session_name,
            request.worktree_path.as_deref(),
            request.pane_layout.as_ref(),
        )?;
        let pty_key =
            build_remote_pty_key(terminal_key, request.session_type, &request.session_name);
        let columns = request.columns.unwrap_or(DEFAULT_TERMINAL_COLUMNS);
        let rows = request.rows.unwrap_or(DEFAULT_TERMINAL_ROWS);
        // PTY가 열리자마자 낸 첫 화면을 놓치지 않도록 출력 자리에 첫 클라이언트를 먼저 붙인다
        let output = self.create_entry_output(request.session_type, columns, rows, Some(client));
        let callbacks = self.build_pty_callbacks(terminal_key, &output, "Host");
        let remote_pty = connection.open_pty(&pty_key, &command, columns, rows, callbacks)?;
        if !client.is_open() {
            let _ = remote_pty.kill();
            return Ok(None);
        }
        let blueprint = EntryBlueprint::new(
            request.session_type,
            &request.session_name,
            &request.task_id,
            request.tab_id.as_deref(),
            None,
        );
        let pty: Arc<dyn PtyProcess> = remote_pty.clone();
        let entry = blueprint.into_entry(pty, Some(remote_pty), output);
        Ok(self.register_terminal_entry(terminal_key, entry, client, "Host"))
    }

    fn log_remote_spawn(
        &self,
        request: &RemoteAttachRequest,
        ssh_args: &[String],
        attach_command: &str,
    ) {
        if !self.is_debug_logging_enabled {
            return;
        }
        let args = serde_json::to_string(ssh_args).unwrap_or_default();
        eprintln!("[터미널] Remote PTY spawn: shell=ssh, args={args}");
        self.debug_log(
            "attachRemoteSession 시작",
            serde_json::json!({
                "taskId": request.task_id,
                "sshHost": request.ssh_host,
                "sessionName": request.session_name,
                "sessionType": request.session_type.as_str(),
                "worktreePath": request.worktree_path,
                "attachCommand": attach_command,
            }),
        );
    }
}

fn build_remote_ssh_args(
    environment: &TerminalHostEnvironment,
    ssh_config: &SshHostConfig,
) -> Vec<String> {
    build_ssh_args(
        ssh_config,
        &SshArgsOptions {
            is_tty_forced: true,
            has_trusted_x11_forwarding: has_local_x11_display(&environment.shell_environment),
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            ..SshArgsOptions::default()
        },
    )
}

/// 공유 Space client를 띄우는 프로세스. daemon이 소켓을 이어 줬으면 로컬 로그인 셸에서 Herdr client를 띄우고,
/// 아니면 ssh에 포워딩 인자를 끼우고 원격 client가 끝날 때 원격 소켓을 지우게 한다
fn build_herdr_client_launch(
    environment: &TerminalHostEnvironment,
    session_name: &str,
    herdr_client: &HerdrTaskClient,
    mut ssh_args: Vec<String>,
) -> Result<(String, Vec<String>), String> {
    let client_command =
        build_herdr_client_command(session_name, &herdr_client.client_socket_path)?;
    if herdr_client.has_host_transport {
        return Ok((
            environment.resolve_login_shell(),
            vec!["-c".to_string(), client_command],
        ));
    }
    let destination_index = ssh_args.len() - 2;
    ssh_args.splice(
        destination_index..destination_index,
        herdr_client.forwarding_args.iter().cloned(),
    );
    let cleanup = format!(
        "rm -f {}",
        quote_for_posix_shell(&herdr_client.client_socket_path)
    );
    let remote_command = format!(
        "trap {} EXIT; {client_command}",
        quote_for_posix_shell(&cleanup)
    );
    if let Some(last_arg) = ssh_args.last_mut() {
        *last_arg = build_remote_shell_command(&remote_command);
    }
    Ok(("ssh".to_string(), ssh_args))
}
