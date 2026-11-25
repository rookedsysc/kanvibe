//! 원격 Herdr pane의 직접 연결 권한과 데스크탑 중계, 태스크 세션 변경 감시.
//!
//! 원격 Herdr pane은 모바일이 원격 agent에 바로 붙는다. 같은 pane을 데스크탑 미러가 이미 비추고 있으면
//! 네이티브 제어 연결이 둘이 되므로 권한을 만들지 않고 미러로 남긴다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kanvibe_domain::task::SessionType;
use kanvibe_services::board_notifier::{BoardEvent, BoardEventSubscription};
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};

use crate::error::{MirrorResult, PaneMirrorError, session_not_running};
use crate::mirror::{MirrorShared, PaneMirror, lock, subscription_key, to_mirror_target};
use crate::ports::{RelayClose, RelayOutput, RemotePaneRelay};

/// 태스크 세션 변경 감시. 버리면 감시가 끝난다
pub struct TaskSessionWatch {
    _subscription: BoardEventSubscription,
}

/// 준비 중인 권한이 아직 유효한지 지켜보는 표시들. 버리면 기기 감시에서 빠진다
struct PreparationWatch<'a> {
    shared: &'a MirrorShared,
    watch_id: u64,
    is_session_changed: Arc<AtomicBool>,
    is_device_unpaired: Arc<AtomicBool>,
    _session_watch: BoardEventSubscription,
}

impl PreparationWatch<'_> {
    fn is_invalidated(&self) -> bool {
        self.is_session_changed.load(Ordering::SeqCst)
            || self.is_device_unpaired.load(Ordering::SeqCst)
    }
}

impl Drop for PreparationWatch<'_> {
    fn drop(&mut self) {
        lock(&self.shared.pending_device_preparations)
            .retain(|(watch_id, _, _)| *watch_id != self.watch_id);
    }
}

impl PaneMirror {
    /// 태스크 세션이 바뀌면 알린다. 이전 세션을 비추던 기기 스트림을 끊어 새 세션으로 다시 붙게 하는 데 쓴다
    pub fn watch_task_session_change(
        &self,
        task_id: &str,
        on_changed: Arc<dyn Fn() + Send + Sync>,
    ) -> TaskSessionWatch {
        let task_id = task_id.to_string();
        let subscription = self.shared.board.subscribe(move |event| {
            if matches!(event, BoardEvent::TaskSessionChanged(payload) if payload.task_id == task_id) {
                on_changed();
            }
        });
        TaskSessionWatch {
            _subscription: subscription,
        }
    }

    /// 기기 연결이 해제됐다. 그 기기의 원격 직접 연결 권한을 모두 폐기하고, 준비 중인 권한은 만들지 않게 한다
    pub fn handle_mobile_device_unpaired(&self, device_id: &str) {
        self.shared.remote_grants.revoke_device(device_id);
        for (_, watched_device_id, is_unpaired) in
            lock(&self.shared.pending_device_preparations).iter()
        {
            if watched_device_id == device_id {
                is_unpaired.store(true, Ordering::SeqCst);
            }
        }
    }

    /// 검증한 원격 Herdr pane만 그 기기의 직접 연결 권한을 준비한다. 다른 세션이면 `Ok(None)`이다
    pub fn prepare_mobile_pane_connection(
        &self,
        task_id: &str,
        pane_id: &str,
        device_id: &str,
    ) -> MirrorResult<Option<RemoteTerminalGrant>> {
        let shared = &self.shared;
        ensure_not_shutting_down(shared)?;
        let key = subscription_key(task_id, pane_id);
        // 기존 미러가 같은 네이티브 제어 연결을 마지막 연결이 반납할 때까지 쥐고 있다
        if shared.is_pane_mirrored(&key) {
            return Ok(None);
        }
        let watch = self.watch_preparation(task_id, device_id);
        let task = shared.find_mirror_task(task_id, false)?;
        let target = to_mirror_target(&task);
        let Some(ssh_host) = target
            .ssh_host()
            .filter(|_| target.session_type == SessionType::Herdr)
        else {
            return Ok(None);
        };
        shared.watch_remote_session_changes();
        let size =
            shared
                .herdr
                .read_task_pane_size(&target.session_name, pane_id, Some(ssh_host))?;
        let Some(size) = size.filter(|_| !watch.is_invalidated()) else {
            return Err(session_not_running());
        };
        let grant = shared
            .remote_grants
            .prepare_grant(&RemoteTerminalGrantInput {
                ssh_host: ssh_host.to_string(),
                device_id: device_id.to_string(),
                task_id: task_id.to_string(),
                session_name: target.session_name.clone(),
                pane_id: pane_id.to_string(),
                cols: u32::try_from(size.cols).ok(),
                rows: u32::try_from(size.rows).ok(),
            })?;
        shared.keep_grant_if_still_valid(grant, &watch, &key)
    }

    /// 권한으로 원격 pane을 데스크탑이 중계한다. 권한이 없으면 먼저 준비하고, 원격 Herdr pane이 아니면 `Ok(None)`이다
    pub fn open_remote_mobile_pane_stream(
        &self,
        task_id: &str,
        pane_id: &str,
        device_id: &str,
        grant_id: Option<&str>,
        on_output: RelayOutput,
        on_close: RelayClose,
    ) -> MirrorResult<Option<Box<dyn RemotePaneRelay>>> {
        let grant_id = match grant_id {
            Some(grant_id) => grant_id.to_string(),
            None => match self.prepare_mobile_pane_connection(task_id, pane_id, device_id)? {
                Some(grant) => grant.grant_id,
                None => return Ok(None),
            },
        };
        let relay = self
            .shared
            .remote_grants
            .attach_relay(&grant_id, device_id, task_id, pane_id, on_output, on_close)?;
        Ok(Some(relay))
    }

    fn watch_preparation(&self, task_id: &str, device_id: &str) -> PreparationWatch<'_> {
        let shared = &self.shared;
        let is_session_changed = Arc::new(AtomicBool::new(false));
        let session_flag = Arc::clone(&is_session_changed);
        let session_watch = self.watch_task_session_change(
            task_id,
            Arc::new(move || session_flag.store(true, Ordering::SeqCst)),
        );
        let is_device_unpaired = Arc::new(AtomicBool::new(false));
        let watch_id = shared.next_id();
        lock(&shared.pending_device_preparations).push((
            watch_id,
            device_id.to_string(),
            Arc::clone(&is_device_unpaired),
        ));
        PreparationWatch {
            shared,
            watch_id,
            is_session_changed,
            is_device_unpaired,
            _session_watch: session_watch._subscription,
        }
    }
}

fn ensure_not_shutting_down(shared: &MirrorShared) -> MirrorResult<()> {
    match shared.is_shutting_down() {
        true => Err(PaneMirrorError::Failed(
            "데스크탑이 종료 중입니다.".to_string(),
        )),
        false => Ok(()),
    }
}

impl MirrorShared {
    /// 준비하는 사이 세션이 바뀌었거나 기기 연결이 해제됐거나 데스크탑 미러가 같은 pane을 쥐었으면 권한을 폐기한다
    fn keep_grant_if_still_valid(
        &self,
        grant: Option<RemoteTerminalGrant>,
        watch: &PreparationWatch<'_>,
        key: &str,
    ) -> MirrorResult<Option<RemoteTerminalGrant>> {
        let is_invalidated = watch.is_invalidated() || self.is_shutting_down();
        if !is_invalidated && !self.is_pane_mirrored(key) {
            return Ok(grant);
        }
        if let Some(grant) = &grant {
            self.remote_grants.revoke_grant(&grant.grant_id);
        }
        match is_invalidated {
            true => Err(session_not_running()),
            false => Ok(None),
        }
    }

    fn is_pane_mirrored(&self, key: &str) -> bool {
        self.has_pane_subscription(key) || self.has_viewport_group(key)
    }

    /// 세션이 바뀐 태스크의 원격 권한은 모두 폐기한다. 처음 권한을 준비할 때 한 번만 건다
    fn watch_remote_session_changes(&self) {
        let mut watch = lock(&self.remote_session_watch);
        if watch.is_some() {
            return;
        }
        let grants = Arc::clone(&self.remote_grants);
        *watch = Some(self.board.subscribe(move |event| {
            if let BoardEvent::TaskSessionChanged(payload) = event {
                grants.revoke_task(&payload.task_id);
            }
        }));
    }
}
