//! 미러 서비스 본체와 공용 조회. TS `mobileBridgeService.ts`의 모듈 전역 상태는 [`MirrorShared`]의 필드다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};
use kanvibe_process::remote_session_dependency::SessionDependencies;
use kanvibe_services::board_notifier::{BoardEventSubscription, BoardNotifier};
use kanvibe_terminal::terminal_registry::TerminalRegistry;
use kanvibe_terminal::terminal_tabs::ZellijIdTargetingSupportCache;

use crate::error::{MirrorResult, PaneMirrorError, SurfaceUnavailableReason, unavailable};
use crate::flight::Flight;
use crate::ports::{
    HerdrMirrorQueries, MirrorProcessSpawner, MirrorTaskStore, MobileTerminalTabs,
    RemotePaneGrants, TaskTerminalAttacher,
};
use crate::subscription::PaneClaim;
use crate::viewport::ViewportGroup;

/// 미러가 기다리는 시간. 기본값은 TS 상수와 같고, 테스트는 짧은 값을 넣는다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirrorTimings {
    /// 조회 명령 상한. 짧게 끝나야 모바일 화면이 멈춘 것처럼 보이지 않는다(TS `MIRROR_COMMAND_TIMEOUT_MS`)
    pub command_timeout: Duration,
    /// 세션·탭·pane이 나타나거나 사라질 때까지 기다리는 상한과 확인 간격
    pub session_start_timeout: Duration,
    pub session_start_poll_interval: Duration,
    /// Zellij가 PTY 크기 변경으로 배치를 다시 계산하기를 기다리는 간격
    pub viewport_settle_poll_interval: Duration,
    /// Zellij 클라이언트에 넣은 휠이 pane 화면에 반영될 때까지 기다리는 시간
    pub zellij_wheel_settle: Duration,
    /// zellij 화면 폴링 주기이자 Herdr 출력이 잠잠해졌다고 보는 시간. 원격은 더 길다
    pub local_dump_interval: Duration,
    pub remote_dump_interval: Duration,
    /// 손가락을 떼었다 다시 올리는 짧은 반복이 이력 전체 재전송을 겹쳐 부르지 않게 하는 간격
    pub herdr_history_refresh_min_interval: Duration,
}

impl Default for MirrorTimings {
    fn default() -> Self {
        Self {
            command_timeout: Duration::from_millis(5_000),
            session_start_timeout: Duration::from_millis(15_000),
            session_start_poll_interval: Duration::from_millis(500),
            viewport_settle_poll_interval: Duration::from_millis(50),
            zellij_wheel_settle: Duration::from_millis(100),
            local_dump_interval: Duration::from_millis(1_000),
            remote_dump_interval: Duration::from_millis(3_000),
            herdr_history_refresh_min_interval: Duration::from_millis(1_000),
        }
    }
}

impl MirrorTimings {
    pub(crate) fn dump_interval(&self, ssh_host: Option<&str>) -> Duration {
        match ssh_host {
            Some(_) => self.remote_dump_interval,
            None => self.local_dump_interval,
        }
    }
}

/// 미러가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct PaneMirrorParts {
    pub tasks: Arc<dyn MirrorTaskStore>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub processes: Arc<dyn MirrorProcessSpawner>,
    pub terminals: Arc<TerminalRegistry>,
    pub attacher: Arc<dyn TaskTerminalAttacher>,
    pub tabs: Arc<dyn MobileTerminalTabs>,
    pub herdr: Arc<dyn HerdrMirrorQueries>,
    pub session_dependencies: Arc<SessionDependencies>,
    /// 탭 서비스와 함께 쓰는 zellij 버전 판정 캐시
    pub zellij_id_targeting_support: Arc<ZellijIdTargetingSupportCache>,
    pub remote_grants: Arc<dyn RemotePaneGrants>,
    pub board: Arc<BoardNotifier>,
    pub timings: MirrorTimings,
}

/// 모바일이 태스크 pane을 읽고, 비추고, 크기를 소유하고, 입력하는 길.
///
/// 메서드는 모두 블로킹이다. 서버는 `spawn_blocking`에서 부르고, 스트림과 폴링은 이 서비스가 띄운 스레드에서 돈다.
/// 스냅샷과 출력 스트림 조회는 포커스를 바꾸지 않는다. 크기 조정을 지원하는 세션은 [`PaneMirror::claim_pane_viewport`]가
/// 모바일 연결 수명 동안 크기를 소유하고 마지막 연결이 나가면 복원한다.
pub struct PaneMirror {
    pub(crate) shared: Arc<MirrorShared>,
}

/// 세션 좌표 하나. `terminalTabService`도 같은 값을 쓴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MirrorSessionTarget {
    pub session_type: SessionType,
    pub session_name: String,
    pub ssh_host: Option<String>,
}

impl MirrorSessionTarget {
    pub fn ssh_host(&self) -> Option<&str> {
        self.ssh_host.as_deref().filter(|host| !host.is_empty())
    }
}

type PendingConnection = Arc<Flight<Result<Option<KanbanTask>, String>>>;

pub(crate) struct MirrorShared {
    pub tasks: Arc<dyn MirrorTaskStore>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub processes: Arc<dyn MirrorProcessSpawner>,
    pub terminals: Arc<TerminalRegistry>,
    pub attacher: Arc<dyn TaskTerminalAttacher>,
    pub tabs: Arc<dyn MobileTerminalTabs>,
    pub herdr: Arc<dyn HerdrMirrorQueries>,
    pub session_dependencies: Arc<SessionDependencies>,
    pub zellij_id_targeting_support: Arc<ZellijIdTargetingSupportCache>,
    pub remote_grants: Arc<dyn RemotePaneGrants>,
    pub board: Arc<BoardNotifier>,
    pub timings: MirrorTimings,
    /// 세션이 없는 태스크의 연결 시도. 같은 태스크를 동시에 열어도 세션은 하나만 만든다
    pending_session_connections: Mutex<HashMap<String, PendingConnection>>,
    /// 태스크별 pane 종료 줄. 두 기기가 마지막 두 pane을 동시에 닫아도 세션이 통째로 사라지지 않게 한다
    pub pane_close_queues: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// pane별 출력 구독. 자리가 있다는 것은 그 pane이 태스크의 세션에 속한다고 이미 확인됐다는 뜻이다
    pub subscriptions: Mutex<HashMap<String, Arc<PaneClaim>>>,
    pub viewports: Mutex<HashMap<String, Arc<ViewportGroup>>>,
    pub is_shutting_down: AtomicBool,
    pub remote_session_watch: Mutex<Option<BoardEventSubscription>>,
    /// 직접 연결 권한을 준비 중인 기기. 준비 중 연결이 해제되면 표시가 켜진다
    pub pending_device_preparations: Mutex<Vec<(u64, String, Arc<AtomicBool>)>>,
    next_id: AtomicU64,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn subscription_key(task_id: &str, pane_id: &str) -> String {
    format!("{task_id}#{pane_id}")
}

impl PaneMirror {
    pub fn new(parts: PaneMirrorParts) -> Self {
        Self {
            shared: Arc::new(MirrorShared {
                tasks: parts.tasks,
                runner: parts.runner,
                processes: parts.processes,
                terminals: parts.terminals,
                attacher: parts.attacher,
                tabs: parts.tabs,
                herdr: parts.herdr,
                session_dependencies: parts.session_dependencies,
                zellij_id_targeting_support: parts.zellij_id_targeting_support,
                remote_grants: parts.remote_grants,
                board: parts.board,
                timings: parts.timings,
                pending_session_connections: Mutex::new(HashMap::new()),
                pane_close_queues: Mutex::new(HashMap::new()),
                subscriptions: Mutex::new(HashMap::new()),
                viewports: Mutex::new(HashMap::new()),
                is_shutting_down: AtomicBool::new(false),
                remote_session_watch: Mutex::new(None),
                pending_device_preparations: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }
}

impl MirrorShared {
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.is_shutting_down.load(Ordering::SeqCst)
    }

    /// 조회 명령은 짧은 상한을 두고 실행한다(TS `runMirrorCommand`)
    pub fn run_command(&self, command: &str, ssh_host: Option<&str>) -> MirrorResult<String> {
        let options = ExecGitOptions {
            timeout: Some(self.timings.command_timeout),
            ..ExecGitOptions::default()
        };
        self.runner
            .exec_git(command, ssh_host, &options)
            .map_err(|error| PaneMirrorError::Failed(error.message))
    }

    pub fn has_zellij_pane_id_support(&self, ssh_host: Option<&str>) -> bool {
        let options = ExecGitOptions {
            timeout: Some(self.timings.command_timeout),
            ..ExecGitOptions::default()
        };
        self.zellij_id_targeting_support
            .resolve_zellij_id_targeting_support(ssh_host, self.runner.as_ref(), &options)
    }

    /// 화면 조회에서만 누락된 세션 정보를 복구하고, 입력 경로에서는 기존 세션을 확인한다
    pub fn find_mirror_task(
        &self,
        task_id: &str,
        should_connect_if_missing: bool,
    ) -> MirrorResult<KanbanTask> {
        let mut task = self.tasks.find_task(task_id)?;
        if should_connect_if_missing && task.as_ref().is_some_and(can_connect_missing_session) {
            let session_type = task.as_ref().and_then(|task| task.session_type);
            task = self.connect_missing_session(task_id, session_type)?;
            if task.is_none() {
                return Err(unavailable(SurfaceUnavailableReason::SessionNotRunning));
            }
        }
        match task {
            Some(task) if task.session_type.is_some() && has_text(&task.session_name) => Ok(task),
            _ => Err(unavailable(SurfaceUnavailableReason::NoSession)),
        }
    }

    /// 같은 태스크의 동시 연결은 먼저 시작한 시도를 함께 기다린다. 끝난 시도는 지워 다음 재시도가 새로 연결하게 한다
    fn connect_missing_session(
        &self,
        task_id: &str,
        session_type: Option<SessionType>,
    ) -> MirrorResult<Option<KanbanTask>> {
        let (flight, is_owner) = {
            let mut pending = lock(&self.pending_session_connections);
            match pending.get(task_id) {
                Some(flight) => (Arc::clone(flight), false),
                None => {
                    let flight = Flight::new();
                    pending.insert(task_id.to_string(), Arc::clone(&flight));
                    (flight, true)
                }
            }
        };
        if is_owner {
            let outcome = session_type
                .map_or_else(|| self.tasks.default_session_type(), Ok)
                .and_then(|session_type| {
                    self.tasks.connect_terminal_session(task_id, session_type)
                });
            lock(&self.pending_session_connections).remove(task_id);
            flight.settle(outcome);
        }
        flight.wait().map_err(PaneMirrorError::Failed)
    }

    pub fn find_mirror_target(&self, task_id: &str) -> MirrorResult<MirrorSessionTarget> {
        Ok(to_mirror_target(&self.find_mirror_task(task_id, false)?))
    }

    /// 조건이 참이 될 때까지 세션 시작 상한 안에서 확인 간격마다 다시 본다. 상한을 넘으면 `Ok(false)`다
    pub fn poll_until(&self, mut check: impl FnMut() -> MirrorResult<bool>) -> MirrorResult<bool> {
        let deadline = Instant::now() + self.timings.session_start_timeout;
        loop {
            if check()? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(self.timings.session_start_poll_interval);
        }
    }
}

fn has_text(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.is_empty())
}

fn can_connect_missing_session(task: &KanbanTask) -> bool {
    let is_missing_session = task.session_type.is_none() || !has_text(&task.session_name);
    let has_branch = has_text(&task.branch_name) || has_text(&task.base_branch);
    is_missing_session && has_text(&task.project_id) && has_branch
}

pub(crate) fn to_mirror_target(task: &KanbanTask) -> MirrorSessionTarget {
    MirrorSessionTarget {
        session_type: task.session_type.unwrap_or(SessionType::Tmux),
        session_name: task.session_name.clone().unwrap_or_default(),
        ssh_host: task.ssh_host.clone(),
    }
}
