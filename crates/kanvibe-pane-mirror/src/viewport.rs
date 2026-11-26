//! 모바일 연결이 pane 크기를 소유하는 동안의 줄(TS `claimPaneViewport`).
//!
//! 창 배치를 공유하는 pane은 같은 줄에서 크기를 바꿔야 한 기기의 퇴장이 다른 기기의 화면을 되돌리지 않는다.
//! tmux는 window, zellij는 태스크 전체, Herdr와 terminal 세션은 pane 하나가 한 줄이다.
//! 줄 안의 크기 요청, 휠, 복원은 잠금 하나로 차례를 지키고, 마지막 연결이 나가면 남은 모바일의 최신 크기나 원래 크기로 돌린다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::pane_mirror::MirrorPane;
use kanvibe_terminal::remote_terminal_protocol::{
    TerminalScrollRequest, parse_terminal_scroll_request,
};
use serde_json::Value;

use crate::error::{MirrorResult, PaneMirrorError, session_not_running};
use crate::mirror::{
    MirrorSessionTarget, MirrorShared, PaneMirror, lock, subscription_key, to_mirror_target,
};
use crate::surfaces::PaneListScope;

const MAX_VIEWPORT_DIMENSION: i64 = 500;

/// 세션 종류마다 크기를 바꾸고 되돌리는 방법
pub(crate) trait ViewportDriver: Send + Sync {
    fn resize(&self, pane_id: &str, columns: i64, rows: i64) -> MirrorResult<()>;
    fn restore(&self) -> MirrorResult<()>;
    /// 휠을 멀티플렉서에 넘기는 길이 있는 세션만 구현한다. 휠이 바꾼 화면이 평소 출력 흐름으로 오지 않으면 그 화면을 돌려준다
    fn scroll(
        &self,
        _pane_id: &str,
        _request: &TerminalScrollRequest,
    ) -> MirrorResult<Option<String>> {
        Ok(None)
    }
}

struct ViewportClient {
    id: u64,
    pane_id: String,
    size: Option<(i64, i64)>,
}

#[derive(Default)]
struct ViewportGroupState {
    /// 오래된 요청부터 앞에 있다. 크기를 다시 요청한 연결은 맨 뒤로 간다
    clients: Vec<ViewportClient>,
    /// 줄에 들어왔지만 아직 끝나지 않은 작업 수
    queued: usize,
}

pub(crate) struct ViewportGroup {
    key: String,
    task_id: String,
    target: MirrorSessionTarget,
    pane: MirrorPane,
    state: Mutex<ViewportGroupState>,
    /// 줄의 차례와 처음 작업이 만든 크기 조정 방법
    queue: Mutex<Option<Result<Arc<dyn ViewportDriver>, PaneMirrorError>>>,
}

impl ViewportGroup {
    fn has_client(&self, client_id: u64) -> bool {
        lock(&self.state)
            .clients
            .iter()
            .any(|client| client.id == client_id)
    }

    fn run<T>(
        &self,
        shared: &Arc<MirrorShared>,
        operation: impl FnOnce(&dyn ViewportDriver) -> MirrorResult<T>,
    ) -> MirrorResult<T> {
        lock(&self.state).queued += 1;
        let result = {
            let mut driver = lock(&self.queue);
            let driver = driver.get_or_insert_with(|| {
                shared.create_viewport_driver(&self.target, &self.task_id, &self.pane)
            });
            match driver {
                Ok(driver) => operation(driver.as_ref()),
                Err(error) => Err(error.clone()),
            }
        };
        lock(&self.state).queued -= 1;
        result
    }

    /// 남은 모바일이 있으면 그 최신 크기로, 없으면 원래 크기로 돌린다
    fn restore_for_remaining(&self, driver: &dyn ViewportDriver) -> MirrorResult<()> {
        let remaining = lock(&self.state)
            .clients
            .iter()
            .rev()
            .find_map(|client| client.size.map(|size| (client.pane_id.clone(), size)));
        match remaining {
            Some((pane_id, (columns, rows))) => driver.resize(&pane_id, columns, rows),
            None => driver.restore(),
        }
    }
}

/// 모바일 연결 하나가 쥔 크기 소유권. `release`를 부르거나 버리면 반납한다.
/// 버릴 때의 반납은 복원 명령이 끝날 때까지 막히므로, 비동기 문맥에서는 `release`를 블로킹 스레드에서 먼저 부른다
pub struct PaneViewportClaim {
    shared: Weak<MirrorShared>,
    group: Arc<ViewportGroup>,
    client_id: u64,
    pane_id: String,
    is_released: AtomicBool,
}

impl PaneViewportClaim {
    /// 크기를 바꾼다. tmux와 zellij는 크기를 바꾼 화면이 출력 흐름으로 다시 오지 않아 바뀐 화면을 함께 돌려준다
    pub fn resize(&self, columns: i64, rows: i64) -> MirrorResult<Option<String>> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(None);
        };
        if self.is_released.load(Ordering::SeqCst) || !self.group.has_client(self.client_id) {
            return Ok(None);
        }
        let is_valid = [columns, rows]
            .iter()
            .all(|value| (1..=MAX_VIEWPORT_DIMENSION).contains(value));
        if !is_valid {
            return Err(PaneMirrorError::Failed(
                "터미널 크기가 유효하지 않습니다.".to_string(),
            ));
        }
        self.move_to_latest((columns, rows));
        let session_type = self.group.target.session_type;
        self.group.run(&shared, |driver| {
            if !self.group.has_client(self.client_id) {
                return Ok(None);
            }
            driver.resize(&self.pane_id, columns, rows)?;
            match session_type {
                SessionType::Tmux | SessionType::Zellij => shared
                    .read_pane_snapshot(&self.group.target, &self.pane_id)
                    .map(Some),
                _ => Ok(None),
            }
        })
    }

    /// 모바일 휠 한 칸. 크기 요청과 같은 줄에 세워 제어 연결이 열리고 반납되는 사이에 끼어들지 않게 한다
    pub fn scroll(&self, record: &Value) -> MirrorResult<Option<String>> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(None);
        };
        let Some(request) = parse_terminal_scroll_request(record) else {
            return Ok(None);
        };
        if self.is_released.load(Ordering::SeqCst) || !self.group.has_client(self.client_id) {
            return Ok(None);
        }
        self.group
            .run(&shared, |driver| driver.scroll(&self.pane_id, &request))
    }

    pub fn release(&self) -> MirrorResult<()> {
        if self.is_released.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        lock(&self.group.state)
            .clients
            .retain(|client| client.id != self.client_id);
        let Some(shared) = self.shared.upgrade() else {
            return Ok(());
        };
        let result = self
            .group
            .run(&shared, |driver| self.group.restore_for_remaining(driver));
        shared.forget_idle_viewport_group(&self.group);
        result
    }

    fn move_to_latest(&self, size: (i64, i64)) {
        let mut state = lock(&self.group.state);
        if let Some(index) = state
            .clients
            .iter()
            .position(|client| client.id == self.client_id)
        {
            let mut client = state.clients.remove(index);
            client.size = Some(size);
            state.clients.push(client);
        }
    }
}

impl Drop for PaneViewportClaim {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            eprintln!("[kanvibe] Mobile viewport restore failed: {error}");
        }
    }
}

impl PaneMirror {
    pub fn claim_pane_viewport(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> MirrorResult<PaneViewportClaim> {
        let shared = &self.shared;
        ensure_running(shared)?;
        let task = shared.find_mirror_task(task_id, false)?;
        let target = to_mirror_target(&task);
        let panes = match target.session_type {
            SessionType::Terminal => {
                shared.refresh_remote_terminal_tabs(&task)?;
                shared
                    .list_terminal_mirror_tabs(task_id)
                    .into_iter()
                    .flat_map(|tab| tab.panes)
                    .collect()
            }
            _ => shared.list_mirror_panes(&target, &PaneListScope::selected(pane_id))?,
        };
        ensure_running(shared)?;
        let pane = panes
            .into_iter()
            .find(|candidate| candidate.id == pane_id)
            .ok_or_else(session_not_running)?;
        let group = shared.join_viewport_group(task_id, &target, pane);
        let client_id = shared.next_id();
        lock(&group.state).clients.push(ViewportClient {
            id: client_id,
            pane_id: pane_id.to_string(),
            size: None,
        });
        Ok(PaneViewportClaim {
            shared: Arc::downgrade(shared),
            group,
            client_id,
            pane_id: pane_id.to_string(),
            is_released: AtomicBool::new(false),
        })
    }
}

fn ensure_running(shared: &MirrorShared) -> MirrorResult<()> {
    match shared.is_shutting_down() {
        true => Err(PaneMirrorError::Failed(
            "데스크탑이 종료 중입니다.".to_string(),
        )),
        false => Ok(()),
    }
}

impl MirrorShared {
    fn join_viewport_group(
        &self,
        task_id: &str,
        target: &MirrorSessionTarget,
        pane: MirrorPane,
    ) -> Arc<ViewportGroup> {
        let scope = match target.session_type {
            SessionType::Zellij => "zellij".to_string(),
            SessionType::Tmux => pane.tab_id.clone(),
            _ => pane.id.clone(),
        };
        let key = subscription_key(task_id, &scope);
        let mut viewports = lock(&self.viewports);
        let group = viewports.entry(key.clone()).or_insert_with(|| {
            Arc::new(ViewportGroup {
                key,
                task_id: task_id.to_string(),
                target: target.clone(),
                pane,
                state: Mutex::new(ViewportGroupState::default()),
                queue: Mutex::new(None),
            })
        });
        Arc::clone(group)
    }

    /// 아무도 쓰지 않고 뒤에 기다리는 작업도 없는 줄만 지운다. 그 사이 다시 붙은 연결이 있으면 줄을 그대로 둔다
    fn forget_idle_viewport_group(&self, group: &Arc<ViewportGroup>) {
        let mut viewports = lock(&self.viewports);
        let is_idle = {
            let state = lock(&group.state);
            state.clients.is_empty() && state.queued == 0
        };
        let is_current = viewports
            .get(&group.key)
            .is_some_and(|current| Arc::ptr_eq(current, group));
        if is_idle && is_current {
            viewports.remove(&group.key);
        }
    }

    pub fn has_viewport_group(&self, key: &str) -> bool {
        lock(&self.viewports).contains_key(key)
    }

    /// 앱을 끌 때 모바일이 소유하던 크기를 모두 원래대로 돌린다
    pub fn restore_all_viewports(self: &Arc<Self>) {
        let groups: Vec<Arc<ViewportGroup>> = lock(&self.viewports).values().cloned().collect();
        for group in groups {
            lock(&group.state).clients.clear();
            if let Err(error) = group.run(self, |driver| driver.restore()) {
                eprintln!("[kanvibe] Mobile viewport restore failed: {error}");
            }
            self.forget_idle_viewport_group(&group);
        }
    }

    fn create_viewport_driver(
        self: &Arc<Self>,
        target: &MirrorSessionTarget,
        task_id: &str,
        pane: &MirrorPane,
    ) -> MirrorResult<Arc<dyn ViewportDriver>> {
        match target.session_type {
            SessionType::Terminal => Ok(crate::viewport_drivers::terminal_driver(
                self, task_id, &pane.id,
            )),
            SessionType::Herdr => Ok(crate::herdr_viewport::herdr_driver(
                self, target, task_id, &pane.id,
            )),
            SessionType::Zellij => {
                crate::viewport_drivers::zellij_driver(self, target, task_id, &pane.id)
            }
            SessionType::Tmux => crate::viewport_drivers::tmux_driver(self, target, &pane.tab_id),
        }
    }
}
