//! 상태 패널의 hooks 카드(TS `HooksStatusCard`와 `TaskDetailRoute`의 hook 상태 조회).
//! 상태는 상태 패널이 보이는 동안에만 한 번에 네 provider를 읽는다. 원격 task는 조회 한 번이 새 SSH 접속이라
//! provider마다 따로 묻지 않는다. 이미 읽은 상태가 있으면 그 상태를 보여 준 채로 다시 읽어 읽는 중 표시가 깜빡이지 않는다.
//! 설치 버튼은 그 provider 하나만 다시 설치하고, 성공하면 네 상태를 한 번 더 읽는다.

use std::sync::Arc;

use gpui_kit::{AppContext, Context, Task};
use kanvibe_domain::i18n::Translator;
use kanvibe_process::hooks::providers::{KanvibeHookProvider, KanvibeHookStatus};
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::project::HookInstallResult;
use serde::Deserialize;

use super::data::desktop_runtime;
use crate::services::ServiceResult;
use crate::views::settings::service_call::{call_service, failure};

pub mod render;

/// 데이터 디렉터리 없이 뜬 실행에서 프로젝트 서비스를 쓸 수 없는 사유
const PROJECT_SERVICE_UNAVAILABLE: &str =
    "데이터 디렉터리 없이 실행되어 프로젝트 서비스를 쓸 수 없습니다.";

/// 카드가 보이는 순서
pub const HOOK_TOOLS: [HookTool; 4] = [
    HookTool::Claude,
    HookTool::Gemini,
    HookTool::Codex,
    HookTool::OpenCode,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookTool {
    Claude,
    Gemini,
    Codex,
    OpenCode,
}

impl HookTool {
    pub fn title(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
        }
    }

    fn index(self) -> usize {
        HOOK_TOOLS
            .iter()
            .position(|tool| *tool == self)
            .unwrap_or(0)
    }

    fn provider(self) -> KanvibeHookProvider {
        match self {
            Self::Claude => KanvibeHookProvider::Claude,
            Self::Gemini => KanvibeHookProvider::Gemini,
            Self::Codex => KanvibeHookProvider::Codex,
            Self::OpenCode => KanvibeHookProvider::OpenCode,
        }
    }

    /// TS IPC 메서드 이름
    fn install_service(self) -> &'static str {
        match self {
            Self::Claude => "project.installTaskHooks",
            Self::Gemini => "project.installTaskGeminiHooks",
            Self::Codex => "project.installTaskCodexHooks",
            Self::OpenCode => "project.installTaskOpenCodeHooks",
        }
    }

    fn success_key(self) -> &'static str {
        match self {
            Self::Claude => "hooksInstallSuccess",
            Self::Gemini => "geminiHooksInstallSuccess",
            Self::Codex => "codexHooksInstallSuccess",
            Self::OpenCode => "openCodeHooksInstallSuccess",
        }
    }
}

/// 카드가 쓰는 provider 상태(TS 상태 객체의 `installed`, OpenCode `pluginRegistrationError`)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookToolStatus {
    pub installed: bool,
    #[serde(default)]
    pub plugin_registration_error: Option<String>,
}

impl From<&KanvibeHookStatus> for HookToolStatus {
    fn from(status: &KanvibeHookStatus) -> Self {
        Self {
            installed: status.is_installed(),
            plugin_registration_error: status.plugin_registration_error().map(str::to_string),
        }
    }
}

/// TS `getTaskHooksStatuses` 응답
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookToolStatuses {
    pub claude: Option<HookToolStatus>,
    pub gemini: Option<HookToolStatus>,
    pub codex: Option<HookToolStatus>,
    pub open_code: Option<HookToolStatus>,
}

impl HookToolStatuses {
    fn into_array(self) -> [Option<HookToolStatus>; 4] {
        [self.claude, self.gemini, self.codex, self.open_code]
    }
}

/// TS 설치 응답 `{ success, error?, status? }`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HookInstallOutcome {
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub status: Option<HookToolStatus>,
}

impl From<HookInstallResult> for HookInstallOutcome {
    fn from(result: HookInstallResult) -> Self {
        Self {
            success: result.is_success,
            error: result.error,
            status: result.status.as_ref().map(HookToolStatus::from),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Success,
    Error,
}

/// 설치 결과 안내. 결과 없이 실패한 설치는 같은 도구로 다시 시도할 수 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallMessage {
    pub kind: MessageKind,
    pub text: String,
    pub retry_tool: Option<HookTool>,
}

/// 전체 설치 수에 따른 배지 모양
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverallHookStatus {
    Complete,
    Partial,
    Missing,
}

pub fn overall_status(installed_count: usize, total_count: usize) -> OverallHookStatus {
    if installed_count == total_count {
        OverallHookStatus::Complete
    } else if installed_count == 0 {
        OverallHookStatus::Missing
    } else {
        OverallHookStatus::Partial
    }
}

fn install_failure_text(translator: &Translator, error: Option<&str>) -> String {
    match error {
        Some(error) => translator.translate_with("hooksInstallFailed", &[("error", error)]),
        None => translator.translate("hooksInstallFailed"),
    }
}

pub struct HooksStatusCard {
    task_id: String,
    translator: Translator,
    common: Translator,
    statuses: [Option<HookToolStatus>; 4],
    is_loading: bool,
    has_loaded: bool,
    installing_tools: Vec<HookTool>,
    message: Option<InstallMessage>,
    /// 상태 패널이 지금 보이는지. 보이기 시작할 때만 다시 읽는다
    is_panel_visible: bool,
    _load: Option<Task<()>>,
    _installs: Vec<Task<()>>,
}

impl HooksStatusCard {
    pub fn new(task_id: String, translator: Translator, common: Translator) -> Self {
        Self {
            task_id,
            translator,
            common,
            statuses: Default::default(),
            is_loading: false,
            has_loaded: false,
            installing_tools: Vec::new(),
            message: None,
            is_panel_visible: false,
            _load: None,
            _installs: Vec::new(),
        }
    }

    pub fn installed_count(&self) -> usize {
        self.statuses
            .iter()
            .filter(|status| status.as_ref().is_some_and(|status| status.installed))
            .count()
    }

    /// 네 provider 상태를 한 번에 읽는다. 처음 읽을 때만 읽는 중을 알리고, 실패하면 보이던 상태를 남긴다
    pub fn load_statuses(&mut self, cx: &mut Context<Self>) {
        if !self.has_loaded {
            self.is_loading = true;
        }
        let request = request_statuses(&self.task_id, cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |card, cx| {
                if let Ok(statuses) = result {
                    card.has_loaded = true;
                    card.statuses = statuses.unwrap_or_default().into_array();
                }
                card.is_loading = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 설치 뒤 네 상태를 한 번 더 읽는다. 받은 값만 덮고 읽지 못하면 그대로 둔다.
    /// 같은 슬롯을 쓰므로 진행 중이던 첫 읽기를 대신한다. 그래서 읽는 중 표시도 여기서 끈다
    fn refresh_after_install(&mut self, cx: &mut Context<Self>) {
        let request = request_statuses(&self.task_id, cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |card, cx| {
                if let Ok(latest) = result {
                    card.has_loaded = true;
                    let latest = latest.unwrap_or_default().into_array();
                    for (current, latest) in card.statuses.iter_mut().zip(latest) {
                        if latest.is_some() {
                            *current = latest;
                        }
                    }
                }
                card.is_loading = false;
                cx.notify();
            });
        }));
    }

    fn install(&mut self, tool: HookTool, cx: &mut Context<Self>) {
        if self.installing_tools.contains(&tool) {
            return;
        }
        self.message = None;
        self.installing_tools.push(tool);
        let runtime = desktop_runtime(cx);
        let task_id = self.task_id.clone();
        let arguments = serde_json::json!([task_id]).to_string();
        let request = call_service(cx, tool.install_service(), arguments, move || {
            install_tool(runtime, &task_id, tool)
        });
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |card, cx| card.finish_install(tool, result, cx));
        });
        self._installs.push(task);
        cx.notify();
    }

    fn finish_install(
        &mut self,
        tool: HookTool,
        result: ServiceResult<HookInstallOutcome>,
        cx: &mut Context<Self>,
    ) {
        self.installing_tools
            .retain(|installing| *installing != tool);
        match result {
            Ok(outcome) => {
                let is_success = outcome.success;
                self.apply_install_outcome(tool, outcome);
                if is_success {
                    self.refresh_after_install(cx);
                }
            }
            // 연결이 끊기는 등 결과 없이 실패해도 설치 중 표시만 사라지지 않게 사유와 다시 시도를 남긴다
            Err(failure) => {
                self.message = Some(InstallMessage {
                    kind: MessageKind::Error,
                    text: install_failure_text(&self.translator, Some(&failure.display_message())),
                    retry_tool: Some(tool),
                });
            }
        }
        cx.notify();
    }

    fn apply_install_outcome(&mut self, tool: HookTool, outcome: HookInstallOutcome) {
        let installed_status = outcome.status.filter(|_| outcome.success);
        let Some(status) = installed_status else {
            self.message = Some(InstallMessage {
                kind: MessageKind::Error,
                text: install_failure_text(&self.translator, outcome.error.as_deref()),
                retry_tool: None,
            });
            return;
        };
        self.message = Some(match status.installed {
            true => InstallMessage {
                kind: MessageKind::Success,
                text: self.translator.translate(tool.success_key()),
                retry_tool: None,
            },
            false => InstallMessage {
                kind: MessageKind::Error,
                text: self.translator.translate("hooksInstallIncomplete"),
                retry_tool: None,
            },
        });
        self.statuses[tool.index()] = Some(status);
    }
}

fn request_statuses(
    task_id: &str,
    cx: &mut Context<HooksStatusCard>,
) -> Task<ServiceResult<Option<HookToolStatuses>>> {
    let runtime = desktop_runtime(cx);
    let task_id = task_id.to_string();
    let arguments = serde_json::json!([task_id]).to_string();
    call_service(cx, "project.getTaskHooksStatuses", arguments, move || {
        read_statuses(runtime, &task_id)
    })
}

fn read_statuses(
    runtime: Option<Arc<DesktopRuntime>>,
    task_id: &str,
) -> ServiceResult<Option<HookToolStatuses>> {
    let runtime = runtime.ok_or_else(|| failure(PROJECT_SERVICE_UNAVAILABLE))?;
    let statuses = runtime
        .project()
        .get_task_hooks_statuses(task_id)
        .map_err(|error| failure(error.message))?;
    Ok(statuses.map(|statuses| HookToolStatuses {
        claude: Some((&statuses.claude).into()),
        gemini: Some((&statuses.gemini).into()),
        codex: Some((&statuses.codex).into()),
        open_code: Some((&statuses.open_code).into()),
    }))
}

fn install_tool(
    runtime: Option<Arc<DesktopRuntime>>,
    task_id: &str,
    tool: HookTool,
) -> ServiceResult<HookInstallOutcome> {
    let runtime = runtime.ok_or_else(|| failure(PROJECT_SERVICE_UNAVAILABLE))?;
    runtime
        .project()
        .install_task_hooks(task_id, tool.provider())
        .map(HookInstallOutcome::from)
        .map_err(|error| failure(error.message))
}

impl super::TaskDetailView {
    /// 상태 패널이 보일 때마다 hook 상태를 다시 읽는다. 패널이 닫혀 있는 창은 원격 접속을 쌓지 않는다.
    /// 프로젝트 없는 작업은 읽을 대상이 없어 빈 상태(0/4)만 보인다
    pub(super) fn sync_hooks_card(&mut self, is_visible: bool, cx: &mut Context<Self>) {
        let Some(task) = self.snapshot().map(|snapshot| snapshot.task.clone()) else {
            return;
        };
        let is_current = self
            .hooks_card
            .as_ref()
            .is_some_and(|card| card.read(cx).task_id == task.id);
        if !is_current {
            let (translator, common) = (self.translator, self.common);
            let task_id = task.id.clone();
            self.hooks_card = Some(cx.new(|_| HooksStatusCard::new(task_id, translator, common)));
        }
        let Some(card) = self.hooks_card.clone() else {
            return;
        };
        let should_load = is_visible && task.project_id.is_some();
        card.update(cx, |card, cx| {
            let was_visible = std::mem::replace(&mut card.is_panel_visible, is_visible);
            if should_load && !was_visible {
                card.load_statuses(cx);
            }
        });
    }

    /// 패널이 열려 있는 동안의 새로 고침은 백그라운드 설치·복구 결과를 반영하려고 다시 묻는다
    pub(super) fn reload_visible_hook_statuses(&mut self, cx: &mut Context<Self>) {
        let has_project = self
            .snapshot()
            .is_some_and(|snapshot| snapshot.task.project_id.is_some());
        if let Some(card) = self.hooks_card.clone().filter(|_| has_project) {
            card.update(cx, |card, cx| {
                if card.is_panel_visible {
                    card.load_statuses(cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::qa::AppClock;
    use gpui_kit::{AppContext, TestAppContext};
    use kanvibe_domain::i18n::{Locale, Translator};
    use kanvibe_domain::platform::ShortcutPlatform;

    use super::{HookInstallOutcome, HookTool, HookToolStatus, HooksStatusCard};
    use crate::services::AppServices;

    #[gpui_kit::gpui::test]
    fn an_install_finishing_before_the_first_read_clears_the_loading_line(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(AppServices::new(
                None,
                None,
                AppClock::system(),
                ShortcutPlatform::current(),
            ))
        });
        let translator = Translator::new(Locale::Ko, "taskDetail");
        let card = cx.new(|_| HooksStatusCard::new("task-1".into(), translator, translator));

        card.update(cx, |card, cx| {
            card.load_statuses(cx);
            let outcome = HookInstallOutcome {
                success: true,
                error: None,
                status: Some(HookToolStatus {
                    installed: true,
                    plugin_registration_error: None,
                }),
            };
            card.finish_install(HookTool::Claude, Ok(outcome), cx);
        });
        cx.run_until_parked();

        card.read_with(cx, |card, _| {
            assert!(!card.is_loading);
            assert_eq!(card.installed_count(), 1);
        });
    }
}
