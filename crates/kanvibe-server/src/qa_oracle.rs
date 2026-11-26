//! 계약 재생기의 서버 전용 오라클(README `--oracle-cmd`)을 QA 실행기에 묻는 Unix 소켓.
//!
//! 데스크탑 설정 화면만 할 수 있는 일(페어링 코드 발급, hook 설치 표 발급)을 재생기가 부를 수 있게 한다.
//! 네트워크 포트가 아니라 0600 Unix 소켓이라 같은 사용자의 재생기만 닿는다. QA 실행기에서만 연다.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

/// 오라클 이름과 인자를 받아 JSON 값을 돌려주는 쪽
pub type OracleHandler = Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;

/// 요청 한 줄(`{"name", "args"}`)마다 응답 한 줄(`{"ok", "value"|"error"}`)을 쓴다
pub fn serve_oracle_socket(path: &Path, handler: OracleHandler) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("kanvibe-qa-oracle".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                answer_oracle_request(stream, &handler);
            }
        })?;
    Ok(())
}

fn answer_oracle_request(stream: UnixStream, handler: &OracleHandler) {
    let mut line = String::new();
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    if BufReader::new(reader_stream).read_line(&mut line).is_err() {
        return;
    }
    let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = request.get("args").cloned().unwrap_or_else(|| json!({}));
    let response = match handler(name, &args) {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    let mut stream = stream;
    let _ = writeln!(stream, "{response}");
}

/// 재생기가 부르는 쪽: 응답 값을 그대로 돌려준다
pub fn ask_oracle(path: &Path, name: &str, args: &Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    writeln!(stream, "{}", json!({ "name": name, "args": args }))
        .map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let response: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
    if response.get("ok") == Some(&Value::Bool(true)) {
        return Ok(response.get("value").cloned().unwrap_or(Value::Null));
    }
    Err(response
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("oracle failed")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_round_trips_values_and_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oracle.sock");
        let handler: OracleHandler = Arc::new(|name, args| match name {
            "echo" => Ok(args.clone()),
            _ => Err(format!("unknown oracle {name}")),
        });
        serve_oracle_socket(&path, handler).unwrap();
        assert_eq!(
            ask_oracle(&path, "echo", &json!({ "a": 1 })),
            Ok(json!({ "a": 1 }))
        );
        assert_eq!(
            ask_oracle(&path, "nope", &json!({})),
            Err("unknown oracle nope".to_string())
        );
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
