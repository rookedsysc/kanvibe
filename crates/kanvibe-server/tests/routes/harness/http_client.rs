//! 응답 바이트를 그대로 보는 작은 HTTP/1.1 클라이언트.
//!
//! 요청마다 `Connection: close`로 연결 하나를 쓰고 끝까지 읽는다. 아무 바이트도 받지 못하고 연결이 끝나면
//! 응답 없이 끊긴 것이다(Node `request.destroy()`와 같은 결과).

use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct HttpCall {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

impl HttpCall {
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn get(path: &str) -> Self {
        Self::new("GET", path)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn auth(self, authorization: &str) -> Self {
        self.header("Authorization", authorization)
    }

    /// `JSON.stringify(value)` 본문
    pub fn json(self, value: &Value) -> Self {
        self.header("Content-Type", "application/json")
            .body(value.to_string())
    }

    pub fn body(mut self, raw: impl Into<Vec<u8>>) -> Self {
        self.body = Some(raw.into());
        self
    }

    fn encode(&self) -> Vec<u8> {
        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n",
            self.method, self.path
        );
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if let Some(body) = &self.body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        let mut encoded = head.into_bytes();
        encoded.extend_from_slice(self.body.as_deref().unwrap_or_default());
        encoded
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpReply {
    /// 빈 본문은 `null`
    pub fn json(&self) -> Value {
        if self.body.is_empty() {
            return Value::Null;
        }
        serde_json::from_str(&self.body)
            .unwrap_or_else(|error| panic!("JSON 본문이 아니다({error}): {}", self.body))
    }

    /// TS 테스트의 `{ status, body }`
    pub fn pair(&self) -> (u16, Value) {
        (self.status, self.json())
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Exchange {
    Reply(HttpReply),
    ClosedWithoutResponse,
}

impl Exchange {
    pub fn reply(self) -> HttpReply {
        match self {
            Self::Reply(reply) => reply,
            Self::ClosedWithoutResponse => panic!("응답 없이 연결이 끊겼다"),
        }
    }
}

/// 서버가 본문을 다 읽기 전에 끊을 수 있으므로 쓰기 실패는 무시하고 받은 바이트로 판단한다
pub async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    call: &HttpCall,
) -> Exchange {
    let _ = stream.write_all(&call.encode()).await;
    let _ = stream.flush().await;
    let received = read_until_closed(&mut stream).await;
    if received.is_empty() {
        return Exchange::ClosedWithoutResponse;
    }
    Exchange::Reply(parse_reply(&received))
}

pub async fn read_until_closed<S: AsyncRead + Unpin>(stream: &mut S) -> Vec<u8> {
    let mut received = Vec::new();
    let reading = tokio::time::timeout(READ_TIMEOUT, stream.read_to_end(&mut received)).await;
    assert!(reading.is_ok(), "서버가 연결을 끝내지 않았다");
    received
}

fn parse_reply(received: &[u8]) -> HttpReply {
    let mut header_slots = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut header_slots);
    let status = response.parse(received).expect("HTTP 응답 머리");
    let httparse::Status::Complete(head_length) = status else {
        panic!(
            "응답 머리가 끝나지 않았다: {}",
            String::from_utf8_lossy(received)
        );
    };
    let headers = response
        .headers
        .iter()
        .map(|header| {
            (
                header.name.to_string(),
                String::from_utf8_lossy(header.value).into_owned(),
            )
        })
        .collect();
    HttpReply {
        status: response.code.unwrap_or_default(),
        headers,
        body: String::from_utf8_lossy(&received[head_length..]).into_owned(),
    }
}
