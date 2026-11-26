//! 로컬 tmux·zellij·Herdr·terminal 세션의 PTY를 새로 띄우고 첫 클라이언트를 붙인다(TS `spawnLocalSession`).

use std::sync::Arc;

use kanvibe_domain::task::SessionType;

use super::attach::EntryBlueprint;
use super::herdr_client::{HerdrTaskClient, prepare_herdr_task_client};
use super::local_sessions::{
    LocalPtySpawnPlan, bootstrap_local_tmux_session, build_local_pty_spawn_plan,
    hide_existing_local_tmux_status_bar, is_tmux_session_alive, is_zellij_session_alive,
};
use super::{
    AttachResult, DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS, LocalAttachRequest,
    TerminalClient, TerminalRegistry, closure,
};
use crate::herdr::{build_herdr_client_command, parse_herdr_session_target};
use crate::pty::PtySpawnRequest;

const PTY_TERMINAL_NAME: &str = "xterm-256color";
const TMUX_CREATION_FAILED_MESSAGE: &str = "tmux 세션 생성에 실패했습니다.";

/// 멀티플렉서 준비 결과
#[derive(Default)]
struct LocalMultiplexerPreparation {
    /// zellij는 TTY 없이 세션을 만들 수 없어 PTY가 생성까지 맡는다
    should_create_zellij_session: bool,
    herdr_workspace_id: Option<String>,
}

impl TerminalRegistry {
    pub(super) fn spawn_local_session(
        self: &Arc<Self>,
        request: &LocalAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> AttachResult {
        let terminal_key = super::build_terminal_key(&request.task_id, request.tab_id.as_deref());
        let Some((mut plan, herdr_workspace_id)) = self.plan_local_pty(request, client)? else {
            return Ok(None);
        };
        if !client.is_open() {
            return Ok(None);
        }
        if let Some(attachment) = self.attach_if_connected_during_bootstrap(&terminal_key, client) {
            return Ok(attachment);
        }
        let blueprint = EntryBlueprint::new(
            request.session_type,
            &request.session_name,
            &request.task_id,
            request.tab_id.as_deref(),
            None,
        );
        let Some(blueprint) =
            self.prepare_entry_blueprint(blueprint, herdr_workspace_id, None, client)?
        else {
            return Ok(None);
        };
        if let Some(herdr_client) = &blueprint.herdr_client {
            let client_command =
                build_herdr_client_command(&request.session_name, &herdr_client.client_socket_path)
                    .inspect_err(|_| blueprint.discard_herdr_client())?;
            plan.args = vec!["-c".to_string(), format!("exec {client_command}")];
        }
        let spawn_request = self.build_spawn_request(
            plan.program,
            plan.args,
            plan.working_directory,
            (request.columns, request.rows),
        );
        self.spawn_and_register(
            &terminal_key,
            blueprint,
            spawn_request,
            client,
            "터미널 프로세스 생성 실패",
            "Local",
        )
    }

    /// 멀티플렉서를 준비하고 띄울 프로세스를 정한다. Herdr면 준비한 Space ID도 돌려준다. 준비 중 클라이언트가 떠나면 `Ok(None)`이다
    fn plan_local_pty(
        &self,
        request: &LocalAttachRequest,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<(LocalPtySpawnPlan, Option<String>)>, String> {
        let working_directory = request
            .working_directory
            .as_deref()
            .filter(|directory| !directory.is_empty());
        let Some(preparation) =
            self.prepare_local_multiplexer(request, working_directory, client)?
        else {
            return Ok(None);
        };
        let plan = build_local_pty_spawn_plan(
            &self.dependencies.environment,
            request.session_type,
            &request.session_name,
            working_directory,
            preparation.should_create_zellij_session,
        )?;
        if self.is_debug_logging_enabled {
            let args = serde_json::to_string(&plan.args).unwrap_or_default();
            eprintln!(
                "[터미널] PTY spawn: shell={}, args={args}, cwd={}",
                plan.program, plan.working_directory
            );
        }
        Ok(Some((plan, preparation.herdr_workspace_id)))
    }

    /// Herdr 공유 Space면 client 중계를 준비해 항목에 단다. 준비하는 사이 클라이언트가 떠나면 중계를 정리하고 `None`이다
    pub(super) fn prepare_entry_blueprint(
        &self,
        mut blueprint: EntryBlueprint,
        herdr_workspace_id: Option<String>,
        ssh_host: Option<&str>,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<EntryBlueprint>, String> {
        blueprint.herdr_client = self.prepare_shared_herdr_client(
            blueprint.session_type,
            &blueprint.session_name,
            herdr_workspace_id,
            ssh_host,
        )?;
        if !client.is_open() {
            blueprint.discard_herdr_client();
            return Ok(None);
        }
        Ok(Some(blueprint))
    }

    /// 크기를 받지 못했으면 기본 크기로 띄운다. 자식에게는 서버 런타임 값을 걷어 낸 셸 환경만 넘긴다
    pub(super) fn build_spawn_request(
        &self,
        program: String,
        args: Vec<String>,
        working_directory: String,
        (columns, rows): (Option<u16>, Option<u16>),
    ) -> PtySpawnRequest {
        PtySpawnRequest {
            program,
            args,
            terminal_name: PTY_TERMINAL_NAME.to_string(),
            columns: columns.unwrap_or(DEFAULT_TERMINAL_COLUMNS),
            rows: rows.unwrap_or(DEFAULT_TERMINAL_ROWS),
            working_directory,
            environment: self.dependencies.environment.shell_environment.clone(),
        }
    }

    /// tmux는 세션이 없으면 detached 세션을 먼저 만들고, zellij는 세션 유무만 보고, Herdr는 공유 Space를 준비한다.
    /// 클라이언트가 준비 중 떠나면 `Ok(None)`이다
    fn prepare_local_multiplexer(
        &self,
        request: &LocalAttachRequest,
        working_directory: Option<&str>,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<LocalMultiplexerPreparation>, String> {
        let runner = self.dependencies.runner.as_ref();
        let directory = working_directory.unwrap_or(&self.dependencies.environment.home_directory);
        let mut preparation = LocalMultiplexerPreparation::default();
        match request.session_type {
            SessionType::Herdr => {
                preparation.herdr_workspace_id = self
                    .dependencies
                    .herdr
                    .ensure_session(
                        &request.session_name,
                        directory,
                        request.pane_layout.as_ref(),
                        None,
                    )
                    .map_err(|error| error.to_string())?;
                if !client.is_open() {
                    return Ok(None);
                }
            }
            SessionType::Tmux => self.ensure_local_tmux_session(request, directory, client)?,
            SessionType::Zellij => {
                preparation.should_create_zellij_session =
                    !is_zellij_session_alive(runner, &request.session_name);
                eprintln!(
                    "[터미널] zellij sessionName=\"{}\", needsCreation={}, cwd={}",
                    request.session_name,
                    preparation.should_create_zellij_session,
                    working_directory.unwrap_or("undefined")
                );
            }
            SessionType::Terminal => {}
        }
        Ok(Some(preparation))
    }

    fn ensure_local_tmux_session(
        &self,
        request: &LocalAttachRequest,
        working_directory: &str,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<(), String> {
        let runner = self.dependencies.runner.as_ref();
        if is_tmux_session_alive(runner, &request.session_name) {
            hide_existing_local_tmux_status_bar(runner, &request.session_name);
            return Ok(());
        }
        let bootstrap = bootstrap_local_tmux_session(
            runner,
            &request.session_name,
            working_directory,
            request.pane_layout.as_ref(),
        );
        if let Err(error) = bootstrap {
            eprintln!("[터미널] tmux 세션 자동 생성 실패: {error}");
            client.close(closure(1008, TMUX_CREATION_FAILED_MESSAGE));
            return Err(TMUX_CREATION_FAILED_MESSAGE.to_string());
        }
        Ok(())
    }

    /// 공유 Space(`kanvibe:<key>`) 세션만 client 중계를 거친다
    pub(super) fn prepare_shared_herdr_client(
        &self,
        session_type: SessionType,
        session_name: &str,
        workspace_id: Option<String>,
        ssh_host: Option<&str>,
    ) -> Result<Option<HerdrTaskClient>, String> {
        let is_shared_space = session_type == SessionType::Herdr
            && parse_herdr_session_target(session_name)
                .is_ok_and(|target| target.workspace_key.is_some());
        if !is_shared_space {
            return Ok(None);
        }
        prepare_herdr_task_client(&self.dependencies, session_name, workspace_id, ssh_host)
            .map(Some)
    }

    /// PTY를 띄워 레지스트리에 올린다. 띄우지 못하면 클라이언트를 1011로 닫고 실패한다
    pub(super) fn spawn_and_register(
        self: &Arc<Self>,
        terminal_key: &str,
        blueprint: EntryBlueprint,
        spawn_request: PtySpawnRequest,
        client: &Arc<dyn TerminalClient>,
        failure_message: &str,
        debug_label: &'static str,
    ) -> AttachResult {
        let output = self.create_entry_output(
            blueprint.session_type,
            spawn_request.columns,
            spawn_request.rows,
            Some(client),
        );
        let callbacks = self.build_pty_callbacks(terminal_key, &output, debug_label);
        let pty = match self
            .dependencies
            .pty_spawner
            .spawn(spawn_request, callbacks)
        {
            Ok(pty) => pty,
            Err(error) => {
                blueprint.discard_herdr_client();
                let log_label = if debug_label == "Remote" {
                    "Remote PTY spawn 실패"
                } else {
                    "PTY spawn 실패"
                };
                eprintln!("[터미널] {log_label}: {error}");
                client.close(closure(1011, failure_message));
                return Err(failure_message.to_string());
            }
        };
        let entry = blueprint.into_entry(pty, None, output);
        Ok(self.register_terminal_entry(terminal_key, entry, client, debug_label))
    }
}
