//! 모바일 앱처럼 Authorization 헤더를 실어 붙는 WebSocket 클라이언트

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
    /// 닫기 코드와 사유. 닫기 프레임 없이 끊기면 1006
    Closed(u16, String),
}

pub struct WsClient {
    stream: WebSocketStream<TcpStream>,
}

/// 핸드셰이크가 거절되면 서버가 쓴 상태 코드를 돌려준다
pub async fn connect_websocket(
    address: SocketAddr,
    path: &str,
    authorization: Option<&str>,
) -> Result<WsClient, u16> {
    let stream = TcpStream::connect(address).await.expect("서버에 연결");
    let mut request = format!("ws://{address}{path}")
        .into_client_request()
        .expect("WebSocket 요청");
    if let Some(authorization) = authorization {
        request
            .headers_mut()
            .insert("authorization", authorization.parse().expect("헤더 값"));
    }
    match tokio_tungstenite::client_async(request, stream).await {
        Ok((stream, _)) => Ok(WsClient { stream }),
        Err(WsError::Http(response)) => Err(response.status().as_u16()),
        Err(error) => panic!("WebSocket 핸드셰이크 실패: {error}"),
    }
}

impl WsClient {
    pub async fn send_text(&mut self, text: &str) {
        self.stream
            .send(Message::text(text.to_string()))
            .await
            .expect("텍스트 프레임 보내기");
    }

    pub async fn send_json(&mut self, value: &Value) {
        self.send_text(&value.to_string()).await;
    }

    pub async fn send_binary(&mut self, data: impl Into<Vec<u8>>) {
        self.stream
            .send(Message::binary(data.into()))
            .await
            .expect("이진 프레임 보내기");
    }

    pub async fn next_frame(&mut self) -> Frame {
        loop {
            let message = tokio::time::timeout(FRAME_TIMEOUT, self.stream.next())
                .await
                .expect("WebSocket 프레임 대기 시간 초과");
            match message {
                Some(Ok(Message::Text(text))) => return Frame::Text(text.to_string()),
                Some(Ok(Message::Binary(data))) => return Frame::Binary(data.to_vec()),
                Some(Ok(Message::Close(frame))) => {
                    let (code, reason) = frame.map_or((1005, String::new()), |frame| {
                        (u16::from(frame.code), frame.reason.to_string())
                    });
                    return Frame::Closed(code, reason);
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return Frame::Closed(1006, String::new()),
            }
        }
    }

    pub async fn next_text(&mut self) -> String {
        match self.next_frame().await {
            Frame::Text(text) => text,
            other => panic!("텍스트 프레임을 기다렸지만 {other:?}"),
        }
    }

    pub async fn next_json(&mut self) -> Value {
        serde_json::from_str(&self.next_text().await).expect("JSON 텍스트 프레임")
    }

    /// 다음 프레임이 닫기여야 한다
    pub async fn closed(&mut self) -> (u16, String) {
        match self.next_frame().await {
            Frame::Closed(code, reason) => (code, reason),
            other => panic!("닫기를 기다렸지만 {other:?}"),
        }
    }

    /// 닫기 인사를 보내고 서버가 연결을 끝낼 때까지 기다린다
    pub async fn close(&mut self) {
        let _ = self.stream.close(None).await;
        while let Ok(Some(Ok(_))) = tokio::time::timeout(FRAME_TIMEOUT, self.stream.next()).await {}
    }
}
