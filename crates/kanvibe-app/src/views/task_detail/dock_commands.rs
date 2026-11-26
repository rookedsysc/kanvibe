//! dock의 외부 명령(TS `openPullRequest`·`openInVsCode`와 첫 조회 뒤 PR 자동 조회).
//!
//! PR 자리는 링크가 없어도 비우지 않고, 눌렀을 때 그 자리에서 PR을 찾아 연다. 조회 실패와 PR 없음은 사용자가 할 일이
//! 달라 따로 알린다. 같은 명령을 기다리는 동안 다시 눌러도 겹쳐 보내지 않는다(TS `useTrackedAction`).
//! 원격 작업은 찾기 전에 그 호스트에 `gh`가 있는지 보고, 없으면 설치할지 묻는다(TS `fetchPrUrlWithPrompt`).
//! QA 모드에서는 TS IPC 이름(`kanban.fetchAndSavePrUrl`·`editor.openTaskInVsCode`·`githubCliDependency.*`)으로 고장 주입
//! 판정을 먼저 거친다.

use gpui_kit::{
    AnyWindowHandle, App, AppContext, AsyncApp, AsyncWindowContext, Context, PromptButton,
    PromptLevel, Window,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_process::editor::{SystemEditorProcessLauncher, open_task_in_vs_code};
use serde_json::{Value, json};

use super::TaskDetailView;
use super::auto_connect::call_service;
use super::data::{TaskDetailSnapshot, desktop_runtime};
use crate::components::session_dependency_prompt::install_prompt;
use crate::qa::QaRuntime;
use crate::qa::faults::FaultOutcome;
use crate::qa::prompts::confirm_action;
use crate::services::{AppServices, apply_fault, format_service_error};
use crate::views::board::services::SessionDependencyCheck;

/// TS IPC `kanban.fetchAndSavePrUrl`
pub const FETCH_PR_URL_SERVICE: &str = "kanban.fetchAndSavePrUrl";
/// TS IPC `editor.openTaskInVsCode`
pub const OPEN_IN_VS_CODE_SERVICE: &str = "editor.openTaskInVsCode";
/// TS IPC `githubCliDependency.getGitHubCliStatus`·`installGitHubCli`
const GITHUB_CLI_STATUS_SERVICE: &str = "githubCliDependency.getGitHubCliStatus";
const GITHUB_CLI_INSTALL_SERVICE: &str = "githubCliDependency.installGitHubCli";

/// 원격 작업의 `gh` 확인에 필요한 것
struct GitHubCliCheck {
    ssh_host: String,
    locale: Locale,
    window: AnyWindowHandle,
}

impl TaskDetailView {
    /// 링크가 있으면 브라우저로 열고, 없으면 찾아서 연다
    pub(super) fn open_pull_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_opening_pull_request {
            return;
        }
        let Some(task) = self.snapshot().map(|snapshot| snapshot.task.clone()) else {
            return;
        };
        if let Some(pr_url) = task.pr_url.as_deref() {
            cx.open_url(pr_url);
            return;
        }
        self.busy.is_opening_pull_request = true;
        let github_cli_check = self.github_cli_check(window);
        cx.spawn_in(window, async move |this, cx| {
            let result = match ensure_remote_github_cli(github_cli_check, cx).await {
                Ok(true) => match this.update(cx, |_, cx| look_up_pull_request(&task.id, None, cx))
                {
                    Ok(lookup) => lookup.await,
                    Err(_) => return,
                },
                Ok(false) => Ok(None),
                Err(reason) => Err(reason),
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.busy.is_opening_pull_request = false;
                view.finish_pull_request_lookup(&task.id, result, window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_pull_request_lookup(
        &mut self,
        task_id: &str,
        result: Result<Option<String>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Err(reason) => {
                let message = self
                    .translator
                    .translate_with("pullRequestLookupFailed", &[("reason", &reason)]);
                show_alert(&message, window, cx);
            }
            Ok(None) => show_alert(
                &self.translator.translate("pullRequestNotFound"),
                window,
                cx,
            ),
            Ok(Some(pr_url)) => {
                self.remember_pull_request_url(task_id, &pr_url);
                cx.open_url(&pr_url);
            }
        }
    }

    /// 작업을 읽은 뒤 PR이 아직 없으면 조용히 찾아 둔다. 실패는 로그로만 남긴다
    pub(super) fn look_up_pull_request_automatically(&mut self, cx: &mut Context<Self>) {
        let Some(task) = self.snapshot().map(|snapshot| snapshot.task.clone()) else {
            return;
        };
        let has_branch = task
            .branch_name
            .as_deref()
            .is_some_and(|branch| !branch.is_empty());
        if !has_branch || task.pr_url.is_some() {
            return;
        }
        // gh 확인과 설치 질문은 작업을 처음 열 때만 한다. 보드 갱신으로 다시 도는 조회가 되풀이하면 원격 확인과 질문이 매번 뜬다
        let is_first_lookup_for_task =
            self.automatic_pr_lookup_task_id.as_deref() != Some(task.id.as_str());
        let window = self.window_handle.filter(|_| is_first_lookup_for_task);
        if window.is_some() {
            self.automatic_pr_lookup_task_id = Some(task.id.clone());
        }
        let github_cli_check = window.and_then(|window| self.github_cli_check_in(window));
        self._automatic_pr_lookup = Some(cx.spawn(async move |this, cx| {
            let result = match ensure_remote_github_cli(github_cli_check, cx).await {
                Ok(true) => {
                    match this.update(cx, |_, cx| look_up_pull_request(&task.id, Some(true), cx)) {
                        Ok(lookup) => lookup.await,
                        Err(_) => return,
                    }
                }
                Ok(false) => Ok(None),
                Err(reason) => Err(reason),
            };
            let _ = this.update(cx, |view, cx| match result {
                Ok(Some(pr_url)) => {
                    view.remember_pull_request_url(&task.id, &pr_url);
                    cx.notify();
                }
                Ok(None) => {}
                Err(reason) => eprintln!("PR URL 자동 조회 실패: {reason}"),
            });
        }));
    }

    fn github_cli_check(&self, window: &Window) -> Option<GitHubCliCheck> {
        self.github_cli_check_in(window.window_handle())
    }

    /// 원격 작업이면 확인할 호스트(TS `task.project?.sshHost ?? task.sshHost`)
    fn github_cli_check_in(&self, window: AnyWindowHandle) -> Option<GitHubCliCheck> {
        let ssh_host = self.snapshot().and_then(remote_ssh_host)?;
        Some(GitHubCliCheck {
            ssh_host,
            locale: self.screen.route.locale,
            window,
        })
    }

    fn remember_pull_request_url(&mut self, task_id: &str, pr_url: &str) {
        if let super::DetailLoad::Loaded(snapshot) = &mut self.load
            && snapshot.task.id == task_id
        {
            std::rc::Rc::make_mut(snapshot).task.pr_url = Some(pr_url.to_string());
        }
    }

    /// 작업 폴더를 VS Code로 연다. 실행 요청이 실패하거나 던져도 실패 알림으로 합류시킨다
    pub(super) fn open_in_vscode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_opening_vscode || self.snapshot().is_none() {
            return;
        }
        self.busy.is_opening_vscode = true;
        let open = open_task_folder_in_vs_code(&self.task_id, cx);
        cx.spawn_in(window, async move |this, cx| {
            let is_opened = open.await;
            finish_vs_code_open(this, is_opened, cx);
        })
        .detach();
        cx.notify();
    }
}

fn finish_vs_code_open(
    this: gpui_kit::WeakEntity<TaskDetailView>,
    is_opened: bool,
    cx: &mut AsyncWindowContext,
) {
    let _ = this.update_in(cx, |view, window, cx| {
        view.busy.is_opening_vscode = false;
        if !is_opened {
            show_alert(&view.translator.translate("openInVsCodeFailed"), window, cx);
        }
        cx.notify();
    });
}

fn remote_ssh_host(snapshot: &TaskDetailSnapshot) -> Option<String> {
    snapshot
        .project
        .as_ref()
        .and_then(|project| project.ssh_host.clone())
        .or_else(|| snapshot.task.ssh_host.clone())
        .filter(|host| !host.is_empty())
}

/// TS `ensureGitHubCliWithPrompt`. 로컬 작업(`None`)이나 `gh`가 있으면 `Ok(true)`, 설치를 거절하면 `Ok(false)`
async fn ensure_remote_github_cli(
    check: Option<GitHubCliCheck>,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let Some(check) = check else {
        return Ok(true);
    };
    let status = read_github_cli_status(&check.ssh_host, GITHUB_CLI_STATUS_SERVICE, cx).await?;
    if status.is_available {
        return Ok(true);
    }
    let common = Translator::new(check.locale, "common");
    let tool_name = status.tool_name.clone().unwrap_or_else(|| "gh".to_string());
    let prompt = install_prompt(&common, &status, &tool_name);
    let answer = cx
        .update_window(check.window, |_, window, cx| {
            confirm_action(&prompt, None, window, cx)
        })
        .map_err(|error| error.to_string())?;
    if !answer.await {
        return Ok(false);
    }
    let installed = read_github_cli_status(&check.ssh_host, GITHUB_CLI_INSTALL_SERVICE, cx).await?;
    if installed.is_available {
        return Ok(true);
    }
    let reason = installed
        .blocked_reason
        .unwrap_or_else(|| "unknown error".to_string());
    Err(common.translate_with(
        "sessionDependency.installFailed",
        &[("tool", &tool_name), ("error", &reason)],
    ))
}

/// `githubCliDependency.*` 호출. 던진 실패는 TS IPC 오류 문구로 돌려준다
async fn read_github_cli_status(
    ssh_host: &str,
    service: &'static str,
    cx: &mut AsyncApp,
) -> Result<SessionDependencyCheck, String> {
    let runtime = cx.update(|cx| desktop_runtime(cx));
    let host = ssh_host.to_string();
    let value = call_service(cx, service, json!([ssh_host]), move || {
        let runtime = runtime.ok_or_else(String::new)?;
        let dependency = runtime.github_cli_dependency();
        let status = match service {
            GITHUB_CLI_INSTALL_SERVICE => dependency.install_and_read_status(Some(&host)),
            _ => dependency.status(Some(&host)),
        };
        let status = status.map_err(|error| error.message)?;
        Ok(serde_json::to_value(status).unwrap_or(Value::Null))
    })
    .await
    .map_err(|reason| format_service_error(&reason))?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// `fetchAndSavePrUrl(taskId[, { isAutomatic }])`. 던진 실패는 TS IPC 오류 문구로 돌려준다
fn look_up_pull_request(
    task_id: &str,
    is_automatic: Option<bool>,
    cx: &App,
) -> gpui_kit::Task<Result<Option<String>, String>> {
    let arguments = match is_automatic {
        Some(is_automatic) => json!([task_id, { "isAutomatic": is_automatic }]),
        None => json!([task_id]),
    };
    let fault = cx
        .global::<AppServices>()
        .check_fault(FETCH_PR_URL_SERVICE, &arguments.to_string());
    let executor = cx.background_executor().clone();
    let runtime = desktop_runtime(cx);
    let task_id = task_id.to_string();
    cx.background_spawn(async move {
        if let FaultOutcome::Result(value) = &fault {
            return Ok(value.as_str().map(str::to_string));
        }
        apply_fault(fault, &executor)
            .await
            .map_err(|failure| format_service_error(&failure.reason))?;
        let Some(runtime) = runtime else {
            return Ok(None);
        };
        runtime
            .kanban()
            .fetch_and_save_pr_url(&task_id, is_automatic.unwrap_or(false))
            .map_err(|error| format_service_error(&error.message))
    })
}

/// `openTaskInVsCode(taskId)`. 성공했으면 `true`
fn open_task_folder_in_vs_code(task_id: &str, cx: &App) -> gpui_kit::Task<bool> {
    let arguments = json!([task_id]).to_string();
    let fault = cx
        .global::<AppServices>()
        .check_fault(OPEN_IN_VS_CODE_SERVICE, &arguments);
    let executor = cx.background_executor().clone();
    let runtime = desktop_runtime(cx);
    let task_id = task_id.to_string();
    cx.background_spawn(async move {
        if let FaultOutcome::Result(value) = &fault {
            return value.get("ok").and_then(Value::as_bool) == Some(true);
        }
        if apply_fault(fault, &executor).await.is_err() {
            return false;
        }
        let Some(runtime) = runtime else {
            return false;
        };
        let environment = &runtime.terminal_environment().shell_environment;
        open_task_in_vs_code(
            runtime.database(),
            &SystemEditorProcessLauncher,
            environment,
            &task_id,
        )
        .is_ok_and(|result| result.is_ok)
    })
}

/// TS `window.alert`. QA 재생에서는 기준 이미지를 찍은 Playwright처럼 띄우지 않고 넘긴다
pub(crate) fn show_alert(message: &str, window: &mut Window, cx: &mut App) {
    if cx.has_global::<QaRuntime>() {
        eprintln!("[kanvibe-qa] alert: {message}");
        return;
    }
    drop(window.prompt(
        PromptLevel::Info,
        message,
        None,
        &[PromptButton::ok("OK")],
        cx,
    ));
}
