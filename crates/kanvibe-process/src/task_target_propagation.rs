//! 같은 worktree나 프로젝트를 다루는 다른 KanVibe client에 task 등록·삭제를 전파한다(TS `taskTargetPropagationService.ts`).
//! 전파 요청은 worktree가 있는 호스트에서 task CLI로 보낸다. targets.json의 주소는 그 호스트에서 각 client로 가는 경로이고,
//! 토큰도 그 호스트의 파일에서 바로 읽혀 명령줄에 실리지 않기 때문이다.

use std::time::Duration;

use serde::Serialize;

use crate::exec::ExecGitOptions;
use crate::host_file_access::HostFileAccess;
use crate::kanvibe_project_state::get_kanvibe_targets_path;
use crate::kanvibe_project_state::parse_kanvibe_targets;
use crate::task_cli::script::{TASK_CLI_PEER_RESULT_PREFIX, build_task_cli_command};

/// 원격 task 생성이 끝날 때까지 기다리는 client가 있을 수 있어 넉넉히 잡는다
pub const PEER_PROPAGATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// 상대 client 목록을 읽기 전에 실패하면 어느 client인지 알 수 없어, 실패 사실만 이 이름으로 알린다
const UNRESOLVED_PEER_URL: &str = "unknown";

/// 다른 client 하나에 보낸 전파 요청의 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerTargetResult {
    pub url: String,
    pub is_success: bool,
    pub http_status: String,
    pub message: String,
}

/// ssh_host에서 이 client의 hook 서버로 닿는 주소(TS `getHookServerUrl`)
pub type HookServerUrlResolver = dyn Fn(Option<&str>) -> Result<String, String> + Send + Sync;

pub struct TaskTargetPropagation<'a> {
    files: &'a HostFileAccess,
    hook_server_url: &'a HookServerUrlResolver,
}

#[derive(Clone, Copy)]
enum PeerCommand {
    Delete,
    Register,
}

impl PeerCommand {
    fn name(self) -> &'static str {
        match self {
            Self::Delete => "propagate-delete",
            Self::Register => "propagate-register",
        }
    }
}

impl<'a> TaskTargetPropagation<'a> {
    pub fn new(files: &'a HostFileAccess, hook_server_url: &'a HookServerUrlResolver) -> Self {
        Self {
            files,
            hook_server_url,
        }
    }

    /// 같은 worktree를 공유하는 다른 KanVibe client에 task 삭제를 전파한다. 그 client는 다시 전파하지 않는다
    pub fn propagate_task_deletion_to_peers(
        &self,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        self.run_peer_propagation(worktree_path, ssh_host, PeerCommand::Delete, &[])
    }

    /// 다른 client에 새 브랜치 worktree를 task로 등록하라고 전파한다.
    /// 프로젝트 루트의 targets.json에는 그 프로젝트를 다루는 모든 client와 각 client의 루트 task가 있다.
    pub fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        self.run_peer_propagation(
            project_repo_path,
            ssh_host,
            PeerCommand::Register,
            &["--branch", branch_name],
        )
    }

    fn run_peer_propagation(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        command: PeerCommand,
        extra_arguments: &[&str],
    ) -> Vec<PeerTargetResult> {
        let mut peer_urls = Vec::new();
        let outcome = self.send_peer_command(
            repo_path,
            ssh_host,
            command,
            extra_arguments,
            &mut peer_urls,
        );
        match outcome {
            Ok(results) => results,
            Err(message) => {
                eprintln!(
                    "[task-cli] 다른 client로 전파하지 못했습니다 {{ repoPath: {repo_path:?}, sshHost: {ssh_host:?}, commandName: {:?}, message: {message:?} }}",
                    command.name()
                );
                failed_peer_results(&peer_urls, &message)
            }
        }
    }

    /// 실패했을 때 어느 client가 대상이었는지 알리도록 읽은 상대 주소를 `peer_urls`에 남긴다
    fn send_peer_command(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        command: PeerCommand,
        extra_arguments: &[&str],
        peer_urls: &mut Vec<String>,
    ) -> Result<Vec<PeerTargetResult>, String> {
        let own_url = (self.hook_server_url)(ssh_host)?;
        *peer_urls = self.read_peer_target_urls(repo_path, &own_url, ssh_host)?;
        if peer_urls.is_empty() {
            return Ok(Vec::new());
        }

        let mut command_arguments = vec![command.name(), "--exclude-url", own_url.as_str()];
        command_arguments.extend_from_slice(extra_arguments);
        let task_cli_command = build_task_cli_command(repo_path, &command_arguments, ssh_host);
        let options = ExecGitOptions {
            timeout: Some(PEER_PROPAGATION_TIMEOUT),
            ..ExecGitOptions::default()
        };
        let output = self
            .files
            .runner()
            .exec_git(&task_cli_command, ssh_host, &options)
            .map_err(|error| error.message)?;
        Ok(parse_peer_target_results(&output))
    }

    /// 실행 전에 전파할 상대가 있는지 본다. 혼자 쓰는 worktree에서는 task CLI를 부르지 않는다
    fn read_peer_target_urls(
        &self,
        repo_path: &str,
        own_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let normalized_own_url = own_url.trim_end_matches('/');
        let targets_path = get_kanvibe_targets_path(repo_path);
        let targets_content = self
            .files
            .read_text_file(&targets_path, ssh_host, &ExecGitOptions::default())
            .map_err(|error| error.to_string())?;
        Ok(parse_kanvibe_targets(&targets_content)
            .targets
            .into_iter()
            .map(|target| target.url)
            .filter(|url| url != normalized_own_url)
            .collect())
    }
}

fn failed_peer_results(peer_urls: &[String], message: &str) -> Vec<PeerTargetResult> {
    let failed_urls = if peer_urls.is_empty() {
        vec![UNRESOLVED_PEER_URL.to_string()]
    } else {
        peer_urls.to_vec()
    };
    failed_urls
        .into_iter()
        .map(|url| PeerTargetResult {
            url,
            is_success: false,
            http_status: "000".into(),
            message: message.to_string(),
        })
        .collect()
}

fn parse_peer_target_results(output: &str) -> Vec<PeerTargetResult> {
    let record_prefix = format!("{TASK_CLI_PEER_RESULT_PREFIX}\t");
    output
        .split('\n')
        .filter(|line| line.starts_with(&record_prefix))
        .map(|line| {
            let mut fields = line.split('\t').skip(1);
            let url = fields.next().unwrap_or_default().to_string();
            let http_status = fields.next().unwrap_or("000").to_string();
            let message = fields.next().unwrap_or_default().to_string();
            PeerTargetResult {
                is_success: http_status.starts_with('2'),
                url,
                http_status,
                message,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::exec::ExecGitError;
    use crate::exec::testing::{RecordedCommand, ScriptedCommandRunner};

    const OWN_URL: &str = "http://10.0.0.2:9736";
    const PEER_URL: &str = "http://10.0.0.7:9736";

    fn targets_content(targets: &[(&str, &str, &str)]) -> String {
        let targets: Vec<serde_json::Value> = targets
            .iter()
            .map(|(url, task_id, token)| serde_json::json!({ "url": url, "taskId": task_id, "token": token }))
            .collect();
        serde_json::json!({ "schemaVersion": 1, "targets": targets }).to_string()
    }

    fn own_url(_ssh_host: Option<&str>) -> Result<String, String> {
        Ok(OWN_URL.to_string())
    }

    /// targets.json 읽기(`test -f ... cat`)와 task CLI 실행(`bash -c`)에 따로 답하는 가짜 원격 셸
    fn remote_runner(
        targets: String,
        task_cli: impl Fn() -> Result<String, ExecGitError> + Send + Sync + 'static,
    ) -> Arc<ScriptedCommandRunner> {
        Arc::new(ScriptedCommandRunner::new(move |call: &RecordedCommand| {
            if call.command.starts_with("bash -c") {
                task_cli()
            } else {
                Ok(targets.clone())
            }
        }))
    }

    fn task_cli_calls(runner: &ScriptedCommandRunner) -> Vec<RecordedCommand> {
        runner
            .calls()
            .into_iter()
            .filter(|call| call.command.starts_with("bash -c"))
            .collect()
    }

    fn write_local_targets(repo: &tempfile::TempDir, content: &str) {
        let directory = repo.path().join(".kanvibe");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("targets.json"), content).unwrap();
    }

    // parity: TS-main-taskTargetPropagationService-1d75cc
    #[test]
    fn worktree_without_other_clients_does_not_run_the_task_cli() {
        let runner = remote_runner(targets_content(&[(OWN_URL, "task-own", "own")]), || {
            Ok(String::new())
        });
        let files = HostFileAccess::new(runner.clone(), "/home/local");
        let propagation = TaskTargetPropagation::new(&files, &own_url);

        let results = propagation
            .propagate_task_deletion_to_peers("/remote/repo__worktrees/feat", Some("devbox"));

        assert!(results.is_empty());
        assert!(task_cli_calls(&runner).is_empty());
    }

    // parity: TS-main-taskTargetPropagationService-5803b8
    #[test]
    fn deletion_runs_the_task_cli_on_the_worktree_host_and_returns_results_per_client() {
        let targets = targets_content(&[
            (OWN_URL, "task-own", "own"),
            (PEER_URL, "task-peer", "peer"),
            ("http://10.0.0.9:9736", "task-offline", "offline"),
        ]);
        let runner = remote_runner(targets, || {
            Ok([
                "noise",
                &format!("__KANVIBE_PEER_RESULT__\t{PEER_URL}\t200\t{{\"success\":true}}"),
                "__KANVIBE_PEER_RESULT__\thttp://10.0.0.9:9736\t000\t연결할 수 없습니다",
                "",
            ]
            .join("\n"))
        });
        let files = HostFileAccess::new(runner.clone(), "/home/local");
        let propagation = TaskTargetPropagation::new(&files, &own_url);

        let results = propagation
            .propagate_task_deletion_to_peers("/remote/repo__worktrees/feat", Some("devbox"));

        let read_call = &runner.calls()[0];
        assert!(
            read_call
                .command
                .contains("'/remote/repo__worktrees/feat/.kanvibe/targets.json'")
        );
        assert_eq!(read_call.ssh_host.as_deref(), Some("devbox"));
        let task_cli_call = &task_cli_calls(&runner)[0];
        let expected_suffix = format!(
            " '/remote/repo__worktrees/feat/.kanvibe/task.sh' 'propagate-delete' '--exclude-url' '{OWN_URL}'"
        );
        assert!(task_cli_call.command.ends_with(&expected_suffix));
        assert_eq!(task_cli_call.ssh_host.as_deref(), Some("devbox"));
        assert_eq!(
            task_cli_call.options.timeout,
            Some(PEER_PROPAGATION_TIMEOUT)
        );
        assert_eq!(
            results,
            [
                PeerTargetResult {
                    url: PEER_URL.into(),
                    is_success: true,
                    http_status: "200".into(),
                    message: "{\"success\":true}".into(),
                },
                PeerTargetResult {
                    url: "http://10.0.0.9:9736".into(),
                    is_success: false,
                    http_status: "000".into(),
                    message: "연결할 수 없습니다".into(),
                },
            ]
        );
    }

    // parity: TS-main-taskTargetPropagationService-f9e20f
    #[test]
    fn registration_passes_the_new_branch_to_the_project_root_task_cli() {
        let repo = tempfile::tempdir().unwrap();
        write_local_targets(&repo, &targets_content(&[(PEER_URL, "root-peer", "peer")]));
        let runner = Arc::new(ScriptedCommandRunner::returning(&format!(
            "__KANVIBE_PEER_RESULT__\t{PEER_URL}\t200\tok\n"
        )));
        let files = HostFileAccess::new(runner.clone(), "/home/local");
        let propagation = TaskTargetPropagation::new(&files, &own_url);
        let repo_path = repo.path().to_str().unwrap();

        propagation.propagate_task_registration_to_peers(repo_path, "feat/new", None);

        let call = &runner.calls()[0];
        let expected_suffix = format!(
            " '{repo_path}/.kanvibe/task.sh' 'propagate-register' '--exclude-url' '{OWN_URL}' '--branch' 'feat/new'"
        );
        assert!(call.command.starts_with("bash -c "));
        assert!(call.command.ends_with(&expected_suffix));
        assert_eq!(call.ssh_host, None);
        assert_eq!(call.options.timeout, Some(PEER_PROPAGATION_TIMEOUT));
    }

    // parity: TS-main-taskTargetPropagationService-6fb93f
    #[test]
    fn task_cli_failure_reports_every_peer_as_failed_instead_of_raising() {
        let repo = tempfile::tempdir().unwrap();
        write_local_targets(&repo, &targets_content(&[(PEER_URL, "task-peer", "peer")]));
        let runner = Arc::new(ScriptedCommandRunner::new(|_| {
            Err(ExecGitError::new(
                "bash: task.sh: No such file or directory",
            ))
        }));
        let files = HostFileAccess::new(runner, "/home/local");
        let propagation = TaskTargetPropagation::new(&files, &own_url);

        let results =
            propagation.propagate_task_deletion_to_peers(repo.path().to_str().unwrap(), None);

        assert_eq!(
            results,
            [PeerTargetResult {
                url: PEER_URL.into(),
                is_success: false,
                http_status: "000".into(),
                message: "bash: task.sh: No such file or directory".into(),
            }]
        );
    }

    fn assert_unresolved_failure(results: &[PeerTargetResult], message: &str) {
        assert_eq!(
            results,
            [PeerTargetResult {
                url: "unknown".into(),
                is_success: false,
                http_status: "000".into(),
                message: message.into(),
            }]
        );
    }

    // parity: TS-main-taskTargetPropagationService-faaf60
    #[test]
    fn failures_before_reading_peers_return_an_unresolved_failure() {
        let runner = remote_runner(String::new(), || Ok(String::new()));
        let files = HostFileAccess::new(runner.clone(), "/home/local");
        let failing_url = |_: Option<&str>| Err("경로 없음".to_string());
        let propagation = TaskTargetPropagation::new(&files, &failing_url);

        let results = propagation.propagate_task_deletion_to_peers("/remote/repo", Some("devbox"));

        assert_unresolved_failure(&results, "경로 없음");
        assert!(runner.calls().is_empty());

        let offline_runner = Arc::new(ScriptedCommandRunner::new(|_| {
            Err(ExecGitError::new("SSH 끊김"))
        }));
        let offline_files = HostFileAccess::new(offline_runner.clone(), "/home/local");
        let propagation = TaskTargetPropagation::new(&offline_files, &own_url);

        let results = propagation.propagate_task_deletion_to_peers("/remote/repo", Some("devbox"));

        assert_unresolved_failure(&results, "SSH 끊김");
        assert!(task_cli_calls(&offline_runner).is_empty());
    }
}
