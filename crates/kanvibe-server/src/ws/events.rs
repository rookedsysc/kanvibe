//! `/api/mobile/events` 소켓: 연결 알림, 서버 이벤트 방송, 명령 응답

use std::sync::Arc;

use bytes::Bytes;
use serde_json::json;

use super::SocketBehavior;
use super::commands::{SUPPORTED_COMMANDS, run_mobile_command};
use super::socket::SocketHandle;
use crate::server::ServerState;

struct EventsSocket {
    socket: Arc<SocketHandle>,
    state: Arc<ServerState>,
}

/// 앱은 `commands`에 없는 명령을 보내지 않고 업데이트 안내를 띄운다
pub fn attach_events_socket(
    socket: Arc<SocketHandle>,
    state: Arc<ServerState>,
) -> Arc<dyn SocketBehavior> {
    state.sockets.add_event_socket(socket.id);
    socket.send_json(&json!({ "type": "connected", "commands": SUPPORTED_COMMANDS }));
    Arc::new(EventsSocket { socket, state })
}

impl SocketBehavior for EventsSocket {
    /// 이진 프레임은 무시한다. 명령은 서로 기다리지 않고 각자 응답한다
    fn on_message(&self, data: Bytes, is_binary: bool) {
        if is_binary {
            return;
        }
        let socket = self.socket.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            if let Some(result) = run_mobile_command(&state, &data, &socket).await {
                socket.send_json(&result);
            }
        });
    }

    /// 닫힌 소켓의 구독이 남으면 아무도 받지 않는 pane을 계속 읽는다
    fn on_close(&self) {
        let bridge = self.state.bridge.clone();
        let owner = self.socket.id;
        tokio::task::spawn_blocking(move || bridge.clear_pane_chat_subscriptions(owner));
    }
}
