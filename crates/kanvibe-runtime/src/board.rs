//! 보드·프로젝트 서비스와 그 둘을 쓰는 서비스(hook 상태, diff, 알림 목록, 백그라운드 sync, 세션 도구 확인) 조립.
//! 포트마다 kanvibe-services·kanvibe-process의 실제 어댑터를 넘긴다.

use std::sync::Arc;
use std::time::Instant;

use kanvibe_db::Database;
use kanvibe_process::github_cli_dependency::GitHubCliDependency;
use kanvibe_process::github_project_icon::{
    GitHubProjectIconResolver, IconHttpClient, UreqIconHttpClient,
};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::HostKanvibeStateStore;
use kanvibe_process::task_state_service::TaskStateService;
use kanvibe_process::task_target_propagation::{HookServerUrlResolver, TaskTargetPropagation};
use kanvibe_services::background_task_sync::timer::ThreadSyncTimer;
use kanvibe_services::background_task_sync::{
    BackgroundTaskSync, DatabaseBackgroundSyncSettings, ServiceBackgroundSyncStages,
};
use kanvibe_services::diff::DiffService;
use kanvibe_services::hook_status::{AiPaneChatSessionBindingRecorder, HookStatusService};
use kanvibe_services::kanban::ports::{
    KanvibeStateWriters, ProcessTaskHookInstallation, ProcessTaskWorktrees,
};
use kanvibe_services::kanban::{KanbanService, KanbanServiceParts, KanbanTimings};
use kanvibe_services::notification_store::{DatabaseNotificationListStorage, NotificationStore};
use kanvibe_services::project::adapters::{
    GitHubProjectIconSource, HostBranchSessions, HostProjectRepositoryProbe, HostProjectStateFiles,
    InstallerProjectHookOperations, LocalTaskAiSessionReader,
};
use kanvibe_services::project::{ProjectService, ProjectServiceParts};
use kanvibe_services::session_dependency::SessionDependencyService;
use kanvibe_services::terminal_ports::TaskTerminalSessions;

use crate::ai::AiParts;
use crate::config::RuntimeConfig;
use crate::foundation::{Foundation, leak_for_process_lifetime};
use crate::hooks::{HookParts, HookServerAddress};
use crate::qa_network::TableIconHttpClient;
use crate::terminal::TerminalParts;

/// 앱 수명 동안 살아 있는 빌린 참조. [`leak_for_process_lifetime`]의 설명대로 런타임마다 한 번 만든다
#[derive(Clone, Copy)]
pub(crate) struct ProcessLifetimeReferences {
    database: &'static Database,
    files: &'static HostFileAccess,
    state_store: &'static HostKanvibeStateStore<'static>,
    hook_server_url: &'static HookServerUrlResolver,
}

pub(crate) fn leak_process_lifetime_references(
    foundation: &Foundation,
    address: &Arc<HookServerAddress>,
) -> ProcessLifetimeReferences {
    let database: &'static Arc<Database> = leak_for_process_lifetime(foundation.database.clone());
    let files: &'static Arc<HostFileAccess> = leak_for_process_lifetime(foundation.files.clone());
    let address = Arc::clone(address);
    let hook_server_url: Box<HookServerUrlResolver> = Box::new(move |ssh_host: Option<&str>| {
        address.url(ssh_host).map_err(|error| error.to_string())
    });
    ProcessLifetimeReferences {
        database,
        files,
        state_store: leak_for_process_lifetime(HostKanvibeStateStore::new(files)),
        hook_server_url: Box::leak(hook_server_url),
    }
}

pub(crate) struct BoardParts {
    pub kanban: Arc<KanbanService>,
    pub project: Arc<ProjectService>,
    pub task_target_propagation: Arc<TaskTargetPropagation<'static>>,
    pub hook_status: Arc<HookStatusService>,
    pub diff: Arc<DiffService>,
    pub notification_store: Arc<NotificationStore>,
    pub background_task_sync: Arc<BackgroundTaskSync>,
    pub session_dependency: Arc<SessionDependencyService>,
    /// 원격 PR 조회 전 `gh` 설치 확인·설치(TS `githubCliDependency` 서비스)
    pub github_cli_dependency: Arc<GitHubCliDependency>,
}

/// 조립 재료. 보드 부분은 바닥·터미널·hook·AI 부분이 모두 생긴 뒤에 만든다
pub(crate) struct BoardInputs<'a> {
    pub config: &'a RuntimeConfig,
    pub foundation: &'a Foundation,
    pub terminal: &'a TerminalParts,
    pub hooks: &'a HookParts,
    pub ai: &'a AiParts,
    pub references: ProcessLifetimeReferences,
}

pub(crate) fn build_board(inputs: &BoardInputs<'_>) -> BoardParts {
    let foundation = inputs.foundation;
    let task_target_propagation = Arc::new(TaskTargetPropagation::new(
        inputs.references.files,
        inputs.references.hook_server_url,
    ));
    let kanban = build_kanban(inputs, &task_target_propagation);
    let project = build_project(inputs);
    let background_task_sync = Arc::new(BackgroundTaskSync::new(
        Arc::new(ServiceBackgroundSyncStages {
            database: Arc::clone(&foundation.database),
            files: Arc::clone(&foundation.files),
            board: Arc::clone(&foundation.board),
            kanban: Arc::clone(&kanban),
            project: Arc::clone(&project),
        }),
        Arc::new(DatabaseBackgroundSyncSettings {
            database: Arc::clone(&foundation.database),
        }),
        Arc::clone(&foundation.board),
        Arc::new(ThreadSyncTimer::default()),
    ));
    BoardParts {
        hook_status: build_hook_status(inputs),
        diff: Arc::new(DiffService::new(
            Arc::clone(&foundation.database),
            Arc::clone(&foundation.files),
        )),
        notification_store: Arc::new(NotificationStore::new(Arc::new(
            DatabaseNotificationListStorage::new(Arc::clone(&foundation.database)),
        ))),
        session_dependency: Arc::new(SessionDependencyService::with_shared_dependencies(
            Arc::clone(&inputs.terminal.session_dependencies),
            task_terminal_sessions(inputs.terminal),
        )),
        github_cli_dependency: Arc::new(GitHubCliDependency::new(Arc::clone(&foundation.runner))),
        kanban,
        project,
        task_target_propagation,
        background_task_sync,
    }
}

fn task_terminal_sessions(terminal: &TerminalParts) -> Arc<dyn TaskTerminalSessions> {
    terminal.task_sessions.clone()
}

fn build_kanban(
    inputs: &BoardInputs<'_>,
    task_target_propagation: &Arc<TaskTargetPropagation<'static>>,
) -> Arc<KanbanService> {
    let foundation = inputs.foundation;
    let references = inputs.references;
    let terminal = task_terminal_sessions(inputs.terminal);
    Arc::new(KanbanService::new(KanbanServiceParts {
        database: Arc::clone(&foundation.database),
        files: Arc::clone(&foundation.files),
        board: Arc::clone(&foundation.board),
        worktrees: Arc::new(ProcessTaskWorktrees::new(
            Arc::clone(&foundation.files),
            Arc::clone(&terminal),
            Arc::clone(&foundation.database),
        )),
        terminal,
        hooks: Arc::new(ProcessTaskHookInstallation::new(Arc::clone(
            &inputs.hooks.installer,
        ))),
        state_files: Arc::new(KanvibeStateWriters::new(
            references.state_store,
            references.database,
        )),
        peers: task_target_propagation.clone(),
        clock: Arc::new(Instant::now),
        timings: KanbanTimings::default(),
    }))
}

fn build_project(inputs: &BoardInputs<'_>) -> Arc<ProjectService> {
    let foundation = inputs.foundation;
    let references = inputs.references;
    let hooks = inputs.hooks;
    let app_clock = inputs.config.clock;
    let icons = GitHubProjectIconSource::new(
        GitHubProjectIconResolver::new(icon_http_client(inputs.config)),
        Arc::clone(&foundation.runner),
    );
    ProjectService::new(ProjectServiceParts {
        database: Arc::clone(&foundation.database),
        board: Arc::clone(&foundation.board),
        repositories: Arc::new(HostProjectRepositoryProbe::new(Arc::clone(
            &foundation.files,
        ))),
        sessions: Arc::new(HostBranchSessions::new(
            Arc::clone(&foundation.files),
            task_terminal_sessions(inputs.terminal),
            Arc::clone(&foundation.database),
        )),
        hooks: Arc::new(InstallerProjectHookOperations::new(
            Arc::clone(&hooks.installer),
            hooks.context.clone(),
            Arc::clone(&hooks.open_code_registration),
        )),
        state_files: Arc::new(HostProjectStateFiles::new(
            references.state_store,
            references.database,
        )),
        icons: Arc::new(icons),
        ai_sessions: Arc::new(LocalTaskAiSessionReader::new(
            inputs.ai.session_host.clone(),
            Arc::clone(&inputs.ai.running_agent_panes),
            // 실행 중 세션의 경과 시간은 앱 시계(QA 고정 시각)로 잰다
            Arc::new(move || app_clock.now_ms() as f64),
        )),
    })
}

/// QA 응답 표가 있으면 아이콘 요청도 표로만 답한다
fn icon_http_client(config: &RuntimeConfig) -> Box<dyn IconHttpClient> {
    match &config.qa_network_responses {
        Some(table) => Box::new(TableIconHttpClient(Arc::clone(table))),
        None => Box::new(UreqIconHttpClient),
    }
}

/// hook 상태 변경은 `.kanvibe/status.json`에도 남기고, 에이전트 대화 정보를 pane chat 바인딩에 묶는다
fn build_hook_status(inputs: &BoardInputs<'_>) -> Arc<HookStatusService> {
    let foundation = inputs.foundation;
    let references = inputs.references;
    Arc::new(HookStatusService::new(
        Arc::clone(&foundation.database),
        Arc::clone(&foundation.board),
        Arc::new(TaskStateService::new(
            references.state_store,
            references.database,
        )),
        Arc::new(AiPaneChatSessionBindingRecorder {
            bindings: Arc::clone(&inputs.ai.pane_chat_session_bindings),
            local_host: inputs.ai.session_host.clone(),
        }),
    ))
}
