//! 미러 테스트. TS `mobileBridgeService.test.ts`의 pane 미러 묶음을 옮긴다.

mod fakes;
mod herdr;
mod lifecycle;
mod pane_operations;
mod process;
mod remote;
mod surfaces;
mod terminal;
mod viewport;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::remote_session_dependency::SessionDependencies;
use kanvibe_process::shell_environment::ShellPlatform;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::terminal_registry::{TerminalRegistry, TerminalRegistryDependencies};
use kanvibe_terminal::terminal_tabs::ZellijIdTargetingSupportCache;

use crate::mirror::{MirrorTimings, PaneMirror, PaneMirrorParts};
use crate::subscription::PaneStreamSubscription;
use fakes::*;

/// 이 세션이 가진 pane은 `%19` 하나뿐이다. `%3`은 같은 tmux 서버의 다른 세션에 있는 pane을 가리킨다
pub const MIRROR_PANE_LINE: &str = "%19\t@10\tbash\t0\t0\t80\t24\tfirst";

/// 스냅샷 "snapshot"이 모바일에 나갈 때의 모양. 커서 줄이 없으니 커서는 옮기지 않는다
pub const SNAPSHOT_REDRAW: &str = "\x1b[H\x1b[2J\x1b[3Jsnapshot";

pub fn test_timings() -> MirrorTimings {
    MirrorTimings {
        command_timeout: Duration::from_millis(1_500),
        session_start_timeout: Duration::from_millis(400),
        session_start_poll_interval: Duration::from_millis(10),
        viewport_settle_poll_interval: Duration::from_millis(1),
        zellij_wheel_settle: Duration::from_millis(1),
        local_dump_interval: Duration::from_millis(250),
        remote_dump_interval: Duration::from_millis(250),
        herdr_history_refresh_min_interval: Duration::from_secs(30),
    }
}

pub struct Harness {
    pub mirror: PaneMirror,
    pub runner: Arc<ScriptedRunner>,
    pub tasks: Arc<FakeTasks>,
    pub herdr: Arc<FakeHerdr>,
    pub attacher: Arc<FakeAttacher>,
    pub processes: Arc<FakeProcesses>,
    pub grants: Arc<FakeGrants>,
    pub ptys: Arc<FakePtySpawner>,
    pub registry: Arc<TerminalRegistry>,
    pub board: Arc<BoardNotifier>,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_timings(test_timings())
    }

    pub fn with_timings(timings: MirrorTimings) -> Self {
        let runner = Arc::new(ScriptedRunner::default());
        let ptys = Arc::new(FakePtySpawner::default());
        let parent: BTreeMap<String, String> = [
            ("HOME", "/home/mirror-user"),
            ("SHELL", "/bin/zsh"),
            ("PATH", "/usr/bin:/bin"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
            &parent,
            ShellPlatform::Other,
            "/fallback",
        ));
        let peers = Arc::new(UnusedTerminalPeers);
        let registry = TerminalRegistry::new(TerminalRegistryDependencies {
            runner: runner.clone(),
            environment,
            pty_spawner: ptys.clone(),
            herdr: peers.clone(),
            herdr_relays: peers.clone(),
            remote_hosts: peers.clone(),
            events: peers,
        });
        let tasks = Arc::new(FakeTasks::default());
        let herdr = Arc::new(FakeHerdr::default());
        let attacher = FakeAttacher::new();
        *attacher.registry.lock().unwrap() = Some(Arc::clone(&registry));
        let tabs = Arc::new(FakeTabs {
            tasks: Arc::clone(&tasks),
            runner: Arc::clone(&runner),
            registry: Mutex::new(Some(Arc::clone(&registry))),
        });
        let processes = Arc::new(FakeProcesses::default());
        let grants = Arc::new(FakeGrants::default());
        let board = BoardNotifier::new();
        let mirror = PaneMirror::new(PaneMirrorParts {
            tasks: tasks.clone(),
            runner: runner.clone(),
            processes: processes.clone(),
            terminals: Arc::clone(&registry),
            attacher: attacher.clone(),
            tabs,
            herdr: herdr.clone(),
            session_dependencies: Arc::new(SessionDependencies::new(runner.clone())),
            zellij_id_targeting_support: Arc::new(ZellijIdTargetingSupportCache::new()),
            remote_grants: grants.clone(),
            board: Arc::clone(&board),
            timings,
        });
        Self {
            mirror,
            runner,
            tasks,
            herdr,
            attacher,
            processes,
            grants,
            ptys,
            registry,
            board,
        }
    }

    pub fn commands(&self) -> Vec<String> {
        self.runner.commands()
    }

    pub fn set_task(&self, task: KanbanTask) {
        self.tasks.set(Some(task));
    }

    /// 세션 pane 목록과 스냅샷만 답하는 tmux 태스크
    pub fn stub_tmux_mirror(&self) {
        self.set_task(tmux_task());
        self.runner.replace(|command, _| {
            Some(Ok(match command.contains("list-panes") {
                true => MIRROR_PANE_LINE.to_string(),
                false => "snapshot".to_string(),
            }))
        });
    }

    pub fn subscribe(&self, task_id: &str, pane_id: &str) -> Subscriber {
        self.try_subscribe(task_id, pane_id).expect("구독")
    }

    pub fn try_subscribe(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> crate::error::MirrorResult<Subscriber> {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let end_count = Arc::new(AtomicUsize::new(0));
        let output = Arc::clone(&chunks);
        let ended = Arc::clone(&end_count);
        let handle = self.mirror.subscribe_to_pane(
            task_id,
            pane_id,
            Arc::new(move |chunk| output.lock().unwrap().push(chunk)),
            Arc::new(move || {
                ended.fetch_add(1, Ordering::SeqCst);
            }),
        )?;
        Ok(Subscriber {
            handle,
            chunks,
            end_count,
        })
    }
}

pub fn tmux_task() -> KanbanTask {
    task("task-1", Some(SessionType::Tmux), Some("kanvibe-task"))
}

pub struct Subscriber {
    pub handle: PaneStreamSubscription,
    pub chunks: Arc<Mutex<Vec<String>>>,
    pub end_count: Arc<AtomicUsize>,
}

impl Subscriber {
    pub fn chunks(&self) -> Vec<String> {
        self.chunks.lock().unwrap().clone()
    }

    pub fn ended(&self) -> usize {
        self.end_count.load(Ordering::SeqCst)
    }
}

/// 서버가 `Arc`로 나눠 쥐고 블로킹 스레드에서 부르는 타입은 모두 스레드 사이로 옮길 수 있어야 한다
#[test]
fn server_facing_types_can_cross_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PaneMirror>();
    assert_send_sync::<crate::PaneChatRuntime>();
    assert_send_sync::<PaneStreamSubscription>();
    assert_send_sync::<crate::PaneViewportClaim>();
    assert_send_sync::<crate::TaskSessionWatch>();
}
