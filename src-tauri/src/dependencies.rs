use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[derive(Default)]
struct Registry {
    available: HashMap<String, Instant>,
    blocked: HashMap<String, String>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(Mutex::default)
}
fn script(tool: &str) -> Result<&'static str, String> {
    match tool {
        "gh" => Ok(include_str!("../scripts/install-gh.sh")),
        "tmux" => Ok(include_str!("../scripts/install-tmux.sh")),
        "zellij" => Ok(include_str!("../scripts/install-zellij.sh")),
        "herdr" => Ok(include_str!("../scripts/install-herdr.sh")),
        _ => Err("지원하지 않는 도구".into()),
    }
}
pub fn is_ssh_transport_error(error: &str) -> bool {
    error.contains("(exit 255)")
}
/// Herdr 공식 설치기는 PATH 밖인 ~/.local/bin에 설치할 수 있어서 그 경로도 설치된 것으로 본다.
fn availability_command(tool: &str) -> String {
    if tool == "herdr" {
        return r#"command -v herdr >/dev/null 2>&1 || test -x "$HOME/.local/bin/herdr""#.into();
    }
    format!("command -v {tool} >/dev/null 2>&1")
}
pub fn dispatch(namespace: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
    let (kind, tool, host) = if namespace == "githubCliDependency" {
        ("gh", Some("gh"), args.first().and_then(Value::as_str))
    } else {
        let kind = args
            .first()
            .and_then(Value::as_str)
            .ok_or("세션 유형이 필요합니다.")?;
        (
            kind,
            match kind {
                "tmux" => Some("tmux"),
                "zellij" => Some("zellij"),
                "herdr" => Some("herdr"),
                "terminal" => None,
                _ => return Err("지원하지 않는 세션 유형".into()),
            },
            args.get(1).and_then(Value::as_str),
        )
    };
    let key = format!("{}:{}", host.unwrap_or("local"), tool.unwrap_or("terminal"));
    if method.starts_with("install") {
        {
            let mut cache = registry().lock().map_err(|e| e.to_string())?;
            cache.available.remove(&key);
            cache.blocked.remove(&key);
        }
        if let Some(tool) = tool {
            let result =
                crate::runtime::run_shell_timeout(script(tool)?, host, Duration::from_secs(600))
                    .and_then(|_| crate::runtime::run_shell(&availability_command(tool), host));
            if let Err(error) = result {
                if host.is_some() && is_ssh_transport_error(&error) {
                    return Err(error);
                }
                let subject = host
                    .map(|h| format!("{h} 호스트"))
                    .unwrap_or("로컬 환경".into());
                let reason = format!("{subject}에서 {tool} 설치를 완료하지 못했습니다. {error}");
                if host.is_some() {
                    registry()
                        .lock()
                        .map_err(|e| e.to_string())?
                        .blocked
                        .insert(key.clone(), reason.clone());
                }
                return Err(reason);
            }
            registry()
                .lock()
                .map_err(|e| e.to_string())?
                .available
                .insert(key.clone(), Instant::now());
        }
    }
    let (cached, blocked) = {
        let cache = registry().lock().map_err(|e| e.to_string())?;
        (
            cache
                .available
                .get(&key)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(60)),
            cache.blocked.get(&key).cloned(),
        )
    };
    let available = if blocked.is_some() {
        false
    } else if cached || tool.is_none() {
        true
    } else {
        let tool = tool.unwrap();
        match crate::runtime::run_shell(&availability_command(tool), host) {
            Ok(_) => {
                registry()
                    .lock()
                    .map_err(|e| e.to_string())?
                    .available
                    .insert(key, Instant::now());
                true
            }
            Err(error) => {
                if host.is_some() && is_ssh_transport_error(&error) {
                    return Err(error);
                }
                false
            }
        }
    };
    Ok(if namespace == "githubCliDependency" {
        json!({"toolName":"gh","sshHost":host,"isRemote":host.is_some(),"available":available,"blockedReason":blocked})
    } else {
        json!({"sessionType":kind,"toolName":tool,"sshHost":host,"isRemote":host.is_some(),"available":available,"blockedReason":blocked})
    })
}
pub fn ensure_remote(kind: &str, host: &str) -> Result<(), String> {
    let args = vec![json!(kind), json!(host)];
    let status = dispatch(
        "sessionDependency",
        "getSessionDependencyStatus",
        args.clone(),
    )?;
    if status["available"] == true {
        return Ok(());
    }
    if let Some(reason) = status["blockedReason"].as_str() {
        return Err(reason.into());
    }
    dispatch("sessionDependency", "installSessionDependency", args).map(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_scripts_are_valid_shell() {
        for tool in ["gh", "tmux", "zellij", "herdr"] {
            assert!(std::process::Command::new("/bin/sh")
                .args(["-n", "-c", script(tool).unwrap()])
                .status()
                .unwrap()
                .success());
        }
    }
    #[test]
    fn herdr_installs_from_official_script_into_user_bin_and_rechecks_it() {
        let installer = script("herdr").unwrap();
        assert!(installer.contains("https://herdr.dev/install.sh"));
        assert!(installer.contains(r#"HERDR_INSTALL_DIR="$HOME/.local/bin" sh"#));
        assert!(installer
            .trim_end()
            .ends_with(&availability_command("herdr")));
        assert!(availability_command("herdr").contains(".local/bin/herdr"));
        assert!(
            !installer.contains(".zshrc")
                && !installer.contains(".bashrc")
                && !installer.contains(".profile")
        );
    }
    #[test]
    fn plain_terminal_needs_no_external_tool() {
        let status = dispatch(
            "sessionDependency",
            "getSessionDependencyStatus",
            vec![json!("terminal"), Value::Null],
        )
        .unwrap();
        assert_eq!(status["available"], true);
        assert!(status["toolName"].is_null());
    }
}
