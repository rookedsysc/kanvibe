//! 터미널 레지스트리(`kanvibe_terminal::terminal_registry`)와 hook 터널이 기다리는 trait를 daemon 연결로 구현한다.

use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};

use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_remote_agent::protocol::{HostRecord, record_integer, record_text};
use kanvibe_terminal::pty::PtyEventCallbacks;
use kanvibe_terminal::terminal_registry::{
    RemoteHostConnection, RemoteHostConnections, RemoteHostPty, RemoteHostPtySession,
    RemoteHostSocketForward,
};
use serde_json::{Value, json};

use crate::connection::{
    HostConnection, RemoteHostConnectionError, RemotePtyAttachment, START_TIMEOUT,
};
use crate::hook_tunnel::{HookTunnelHostConnection, HookTunnelHostTransport, HostHookHandler};
use crate::pool::RemoteHostConnectionPool;

impl HostConnection {
    /// daemon PTY를 열거나 같은 key의 세션에 다시 붙는다
    pub fn open_pty(
        self: &Arc<Self>,
        key: &str,
        command: &str,
        columns: u16,
        rows: u16,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<RemotePtyAttachment>, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let sender = Arc::downgrade(self);
        let send = Arc::new(move |record: Value| match sender.upgrade() {
            Some(connection) => connection.send(record),
            None => Err("Host transport disconnected or backpressured".to_string()),
        });
        let attachment = RemotePtyAttachment::start(&id, send, columns, rows, callbacks);
        let receiver = Arc::clone(&attachment);
        if let Err(error) = self.subscribe(
            &id,
            Arc::new(move |record: &HostRecord| receiver.receive(record)),
        ) {
            attachment.finish_detached(error.clone());
            return Err(error);
        }
        let (connection, unsubscribe_id) = (Arc::downgrade(self), id.clone());
        attachment.set_on_finished(Box::new(move || {
            if let Some(connection) = connection.upgrade() {
                connection.unsubscribe(&unsubscribe_id);
            }
        }));
        let open = json!({ "type": "pty.open", "id": id, "key": key, "command": command, "cols": columns, "rows": rows });
        if let Err(error) = self.send(open) {
            self.unsubscribe(&id);
            attachment.finish_detached(error.clone());
            return Err(error);
        }
        attachment.wait_ready(START_TIMEOUT)?;
        Ok(attachment)
    }

    /// 응답 레코드 하나를 기다리는 PTY 요청(`pty.list`, `pty.close`)
    fn request_pty(&self, record: Value, expected: &str) -> Result<HostRecord, String> {
        let id = record["id"].as_str().unwrap_or_default().to_string();
        let (sender, responses) = channel();
        let expected_type = expected.to_string();
        self.subscribe(
            &id,
            Arc::new(move |record: &HostRecord| {
                let kind = record_text(record, "type").unwrap_or_default();
                if kind == expected_type || kind == "pty.error" || kind == "disconnected" {
                    let _ = sender.send(record.clone());
                }
            }),
        )?;
        let response =
            self.send(record)
                .and_then(|()| match responses.recv_timeout(START_TIMEOUT) {
                    Ok(response) => Ok(response),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                        Err("PTY request timed out; outcome may be unknown".to_string())
                    }
                });
        self.unsubscribe(&id);
        let response = response?;
        if record_text(&response, "type") != Some(expected) {
            return Err(record_text(&response, "error")
                .unwrap_or("Host disconnected; PTY request outcome may be unknown")
                .to_string());
        }
        Ok(response)
    }

    pub fn list_pty_sessions(&self) -> Result<Vec<RemoteHostPtySession>, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let response = self.request_pty(json!({ "type": "pty.list", "id": id }), "pty.sessions")?;
        let sessions = response
            .get("sessions")
            .and_then(Value::as_array)
            .ok_or("Invalid PTY session list")?;
        sessions
            .iter()
            .map(|session| {
                let session = session.as_object().ok_or("Invalid PTY session list")?;
                let is_valid = record_integer(session, "pid").is_some();
                match (
                    record_text(session, "sessionId"),
                    record_text(session, "key"),
                    is_valid,
                ) {
                    (Some(session_id), Some(key), true) => Ok(RemoteHostPtySession {
                        key: key.into(),
                        session_id: session_id.into(),
                    }),
                    _ => Err("Invalid PTY session list".to_string()),
                }
            })
            .collect()
    }

    pub fn close_pty_session(&self, session_id: &str) -> Result<(), String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.request_pty(
            json!({ "type": "pty.close", "id": id, "sessionId": session_id }),
            "pty.closed",
        )
        .map(|_| ())
    }
}

/// `RemoteHostConnection`의 메서드는 `&self`라 연결의 `Arc`를 따로 쥔다
pub struct RegistryHostConnection(pub Arc<HostConnection>);

impl RemoteHostConnection for RegistryHostConnection {
    fn open_pty(
        &self,
        key: &str,
        command: &str,
        columns: u16,
        rows: u16,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn RemoteHostPty>, String> {
        let attachment = self.0.open_pty(key, command, columns, rows, callbacks)?;
        Ok(attachment)
    }

    fn list_pty_sessions(&self) -> Result<Vec<RemoteHostPtySession>, String> {
        self.0.list_pty_sessions()
    }

    fn close_pty_session(&self, session_id: &str) -> Result<(), String> {
        self.0.close_pty_session(session_id)
    }

    fn forward_socket(
        &self,
        remote_socket_path: &str,
    ) -> Result<Arc<dyn RemoteHostSocketForward>, String> {
        let forward = self.0.forward_socket(remote_socket_path)?;
        Ok(forward)
    }
}

/// 터미널 레지스트리가 쓰는 연결 조회. daemon을 쓸 수 없는 호스트만 순수 ssh 경로로 가도록 `Ok(None)`이고, 연결 오류는 그대로 돌려준다
pub struct RegistryHostConnections(pub Arc<RemoteHostConnectionPool>);

impl RemoteHostConnections for RegistryHostConnections {
    fn connection(&self, ssh_host: &str) -> Result<Option<Arc<dyn RemoteHostConnection>>, String> {
        let connection = self
            .0
            .connection(ssh_host)
            .map_err(|error| error.to_string())?;
        Ok(connection.map(|connection| {
            Arc::new(RegistryHostConnection(connection)) as Arc<dyn RemoteHostConnection>
        }))
    }
}

impl HookTunnelHostConnection for HostConnection {
    fn set_hook_handler(&self, handler: HostHookHandler) {
        HostConnection::set_hook_handler(self, handler);
    }

    fn close(&self) {
        HostConnection::close(self);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        HostConnection::on_close(self, callback);
    }

    fn has_replaced_stale_peer(&self) -> bool {
        self.replaced_stale_peer
    }
}

/// hook 터널이 쓰는 daemon 연결 풀
pub struct TunnelHostTransport(pub Arc<RemoteHostConnectionPool>);

impl HookTunnelHostTransport for TunnelHostTransport {
    fn configure(&self, namespace: &str, client_id: &str) -> Result<(), String> {
        self.0.configure_transport(namespace, client_id)
    }

    fn namespace(&self) -> Option<String> {
        self.0.namespace()
    }

    fn endpoint_key(&self, config: &SshHostConfig) -> Result<String, String> {
        self.0.endpoint_key(config)
    }

    fn connection(
        &self,
        ssh_host: &str,
    ) -> Result<Option<Arc<dyn HookTunnelHostConnection>>, RemoteHostConnectionError> {
        let connection = self.0.connection(ssh_host)?;
        Ok(connection.map(|connection| connection as Arc<dyn HookTunnelHostConnection>))
    }

    fn stop(&self) {
        self.0.stop_connections();
    }
}
