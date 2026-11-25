//! WebSocket 하나의 보내기 쪽과 소켓 목록(TS `ws` 소켓 객체와 `attachMobileStreamServer`의 기기 지도).
//!
//! 보내기는 소켓마다 작업 하나가 채널을 비우며 순서대로 쓴다. 블로킹 서비스 스레드의 콜백도 채널에 넣기만 하면 되므로
//! 출력 콜백이 어느 스레드에서 불려도 프레임 순서가 섞이지 않는다.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

/// 보내기 작업이 처리할 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    Text(String),
    Close(u16, String),
    Ping,
    /// 닫기 인사 없이 연결을 끊는다(TS `terminate`)
    Terminate,
}

pub struct SocketHandle {
    pub id: u64,
    sender: UnboundedSender<Outgoing>,
    /// TS `readyState === OPEN`. 닫기를 보냈거나 연결이 끝나면 거짓이다
    is_open: AtomicBool,
    /// 하트비트 사이에 pong을 받았는지
    is_alive: AtomicBool,
}

impl SocketHandle {
    pub fn new(id: u64, sender: UnboundedSender<Outgoing>) -> Self {
        Self {
            id,
            sender,
            is_open: AtomicBool::new(true),
            is_alive: AtomicBool::new(true),
        }
    }

    pub fn is_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    pub fn send_text(&self, text: String) {
        if self.is_open() {
            let _ = self.sender.send(Outgoing::Text(text));
        }
    }

    pub fn send_json(&self, payload: &Value) {
        self.send_text(payload.to_string());
    }

    /// 처음 한 번만 닫기 프레임을 보낸다. 그 뒤의 보내기는 TS처럼 버려진다
    pub fn close(&self, code: u16, reason: &str) {
        if self.is_open.swap(false, Ordering::SeqCst) {
            let _ = self.sender.send(Outgoing::Close(code, reason.to_string()));
        }
    }

    pub fn terminate(&self) {
        self.is_open.store(false, Ordering::SeqCst);
        let _ = self.sender.send(Outgoing::Terminate);
    }

    pub fn mark_ended(&self) {
        self.is_open.store(false, Ordering::SeqCst);
    }

    pub fn mark_alive(&self) {
        self.is_alive.store(true, Ordering::SeqCst);
    }

    /// 하트비트 한 번. 지난 주기에 pong이 없었으면 끊고, 있었으면 다시 묻는다
    fn beat(&self) {
        if !self.is_alive.swap(false, Ordering::SeqCst) {
            self.terminate();
            return;
        }
        if self.is_open() {
            let _ = self.sender.send(Outgoing::Ping);
        }
    }
}

/// 서버의 모든 모바일 소켓. 기기별 목록은 연결 해제 때 그 기기의 소켓만 닫는 데 쓴다
#[derive(Default)]
pub struct SocketHub {
    next_id: AtomicU64,
    sockets: Mutex<HashMap<u64, Arc<SocketHandle>>>,
    device_sockets: Mutex<HashMap<String, HashSet<u64>>>,
    event_sockets: Mutex<HashSet<u64>>,
}

impl SocketHub {
    pub fn next_socket_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn register(&self, device_id: &str, socket: Arc<SocketHandle>) {
        let id = socket.id;
        lock(&self.sockets).insert(id, socket);
        lock(&self.device_sockets)
            .entry(device_id.to_string())
            .or_default()
            .insert(id);
    }

    /// 빈 목록을 남기면 스트림을 여닫기만 반복해도 지도가 기기 수만큼 자란다
    pub fn forget(&self, device_id: &str, socket_id: u64) {
        lock(&self.sockets).remove(&socket_id);
        lock(&self.event_sockets).remove(&socket_id);
        let mut device_sockets = lock(&self.device_sockets);
        if let Some(ids) = device_sockets.get_mut(device_id) {
            ids.remove(&socket_id);
            if ids.is_empty() {
                device_sockets.remove(device_id);
            }
        }
    }

    pub fn add_event_socket(&self, socket_id: u64) {
        lock(&self.event_sockets).insert(socket_id);
    }

    /// 열린 이벤트 소켓 모두에 같은 메시지를 보낸다
    pub fn broadcast_event(&self, event: &Value) {
        let message = event.to_string();
        for socket in self.collect(|hub| lock(&hub.event_sockets).iter().copied().collect()) {
            socket.send_text(message.clone());
        }
    }

    pub fn close_device_sockets(&self, device_id: &str, code: u16, reason: &str) {
        let ids: Vec<u64> = lock(&self.device_sockets)
            .get(device_id)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default();
        for socket in self.collect(|_| ids.clone()) {
            socket.close(code, reason);
        }
    }

    pub fn close_all(&self, code: u16, reason: &str) {
        for socket in self.collect(|hub| lock(&hub.sockets).keys().copied().collect()) {
            socket.close(code, reason);
        }
    }

    pub fn heartbeat(&self) {
        for socket in self.collect(|hub| lock(&hub.sockets).keys().copied().collect()) {
            socket.beat();
        }
    }

    fn collect(&self, ids: impl FnOnce(&Self) -> Vec<u64>) -> Vec<Arc<SocketHandle>> {
        let ids = ids(self);
        let sockets = lock(&self.sockets);
        ids.iter()
            .filter_map(|id| sockets.get(id).cloned())
            .collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_is_sent_once_and_later_sends_are_dropped() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let socket = SocketHandle::new(1, sender);
        socket.close(4001, "device-unpaired");
        socket.close(1001, "server-shutdown");
        socket.send_text("late".into());
        assert_eq!(
            receiver.try_recv(),
            Ok(Outgoing::Close(4001, "device-unpaired".into()))
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn heartbeat_terminates_sockets_that_missed_a_pong() {
        let hub = SocketHub::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        hub.register("device", Arc::new(SocketHandle::new(1, sender)));
        hub.heartbeat();
        hub.heartbeat();
        assert_eq!(receiver.try_recv(), Ok(Outgoing::Ping));
        assert_eq!(receiver.try_recv(), Ok(Outgoing::Terminate));
    }

    #[test]
    fn forgetting_the_last_socket_drops_the_device_entry() {
        let hub = SocketHub::default();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        hub.register("device", Arc::new(SocketHandle::new(7, sender)));
        hub.forget("device", 7);
        assert!(lock(&hub.device_sockets).is_empty());
    }
}
