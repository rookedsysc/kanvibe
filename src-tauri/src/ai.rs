use base64::Engine;
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};
#[path = "ai_sessions.rs"]
pub mod sessions;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn read_json(path: impl AsRef<Path>) -> Value {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}
pub fn setting(app: &AppHandle, key: &str) -> Result<Option<String>, String> {
    app.state::<crate::database::Database>().setting(key)
}
pub fn set_setting(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    app.state::<crate::database::Database>()
        .set_setting(key, value)
}
pub fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("KanVibe")
        .build()
        .map_err(|e| e.to_string())
}
fn spec(
    provider: &str,
) -> Result<(&'static str, &'static str, &'static str, &'static str), String> {
    match provider {
        "claude" => Ok((
            "CLAUDE_CONFIG_DIR",
            ".claude",
            ".claude-",
            ".credentials.json",
        )),
        "codex" => Ok(("CODEX_HOME", ".codex", ".codex-", "auth.json")),
        "gemini" => Ok(("GEMINI_CLI_HOME", "", ".gemini-", "oauth_creds.json")),
        _ => Err("Unknown AI provider".into()),
    }
}
fn config(provider: &str, root: &Path) -> PathBuf {
    if provider == "gemini" {
        root.join(".gemini")
    } else {
        root.into()
    }
}
fn default_root(provider: &str) -> Result<PathBuf, String> {
    Ok(home().join(spec(provider)?.1))
}
pub fn provider_command(
    provider: &str,
    root: &Path,
    args: &[&str],
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new(provider);
    cmd.args(args)
        .stdin(Stdio::null())
        .env_clear()
        .envs(crate::runtime::shell_environment());
    let env = spec(provider)?.0;
    if root == default_root(provider)? {
        cmd.env_remove(env);
    } else {
        cmd.env(env, root);
    }
    bounded_output(&mut cmd, Duration::from_secs(30))
}
pub(crate) fn bounded_output(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("Missing command stdout")?;
    let stderr = child.stderr.take().ok_or("Missing command stderr")?;
    let read = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stream
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(bytes)
        })
    };
    let stdout = read(Box::new(stdout));
    let stderr = read(Box::new(stderr));
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() >= timeout {
            // 호출자가 프로세스 그룹을 분리한 경우 CLI가 띄운 자식까지 함께 끝낸다.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err("AI CLI timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // 자식이 출력 파이프를 물고 남아 있어도 제한 시간을 넘겨 기다리지 않는다.
    while !(stdout.is_finished() && stderr.is_finished()) {
        if started.elapsed() >= timeout {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            return Err("AI CLI timed out".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let stdout = stdout.join().map_err(|_| "Command stdout failed")??;
    let stderr = stderr.join().map_err(|_| "Command stderr failed")??;
    if stdout.len() > 1024 * 1024 || stderr.len() > 1024 * 1024 {
        return Err("AI CLI output exceeded limit".into());
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn registrations(app: &AppHandle) -> Result<Vec<Value>, String> {
    stored_registrations(&app.state::<crate::database::Database>())
}
fn stored_registrations(db: &crate::database::Database) -> Result<Vec<Value>, String> {
    Ok(db
        .setting("ai_accounts")?
        .and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| {
            spec(text(r, "provider")).is_ok()
                && !text(r, "accountRoot").is_empty()
                && valid_name(text(r, "accountName"))
        })
        .collect())
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn keychain(root: &Path) -> (Value, bool) {
    if !cfg!(target_os = "macos") {
        return (Value::Null, false);
    }
    let hash = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    let services = [
        format!("Claude Code-credentials-{}", &hash[..8]),
        "Claude Code-credentials".into(),
    ];
    let mut unreadable = false;
    for service in services {
        match bounded_output(
            Command::new("/usr/bin/security").args(["find-generic-password", "-s", &service, "-w"]),
            Duration::from_secs(10),
        ) {
            Ok(out) if out.status.success() => {
                if let Ok(v) = serde_json::from_slice(&out.stdout) {
                    return (v, false);
                }
            }
            Ok(out) => {
                unreadable |= out.status.code() != Some(44);
            }
            Err(_) => unreadable = true,
        }
    }
    (Value::Null, unreadable)
}
#[derive(Clone)]
struct Account {
    provider: String,
    root: PathBuf,
    dir: PathBuf,
    id: String,
    label: String,
    registered: bool,
}
fn discover(app: &AppHandle) -> Result<Vec<Account>, String> {
    let regs = registrations(app)?;
    let mut all = Vec::new();
    for provider in ["claude", "codex", "gemini"] {
        let (env, _, prefix, file) = spec(provider)?;
        let default = default_root(provider)?;
        let mut roots = Vec::new();
        if let Some(root) = std::env::var_os(env) {
            roots.push(PathBuf::from(root));
        }
        roots.push(default.clone());
        if let Ok(entries) = fs::read_dir(home()) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && entry.file_name().to_string_lossy().starts_with(prefix)
                {
                    roots.push(entry.path());
                }
            }
        }
        roots.extend(
            regs.iter()
                .filter(|v| text(v, "provider") == provider)
                .map(|r| PathBuf::from(text(r, "accountRoot"))),
        );
        let mut seen_roots = HashSet::new();
        let mut seen_ids = HashSet::new();
        for root in roots {
            if !seen_roots.insert(root.clone()) {
                continue;
            }
            let dir = config(provider, &root);
            let mut creds = read_json(dir.join(file));
            let registered = regs.iter().any(|r| {
                text(r, "provider") == provider && text(r, "accountRoot") == root.to_string_lossy()
            });
            let mut unreadable = false;
            if provider == "claude"
                && root == default
                && creds.pointer("/claudeAiOauth/accessToken").is_none()
            {
                let (kc, unread) = keychain(&root);
                creds = kc;
                unreadable = unread;
            }
            let token = match provider {
                "claude" => creds.pointer("/claudeAiOauth/accessToken"),
                "codex" => creds.pointer("/tokens/access_token"),
                _ => creds.get("access_token"),
            };
            if !registered && !unreadable && token.and_then(Value::as_str).is_none_or(str::is_empty)
            {
                continue;
            }
            let identity = if provider == "claude" {
                let local = read_json(dir.join(".claude.json"));
                if local.pointer("/oauthAccount/accountUuid").is_some()
                    || local.pointer("/oauthAccount/emailAddress").is_some()
                {
                    local
                } else if root == default {
                    read_json(home().join(".claude.json"))
                } else {
                    Value::Null
                }
            } else {
                Value::Null
            };
            let id = if provider == "claude" {
                identity.pointer("/oauthAccount/accountUuid")
            } else {
                creds.pointer("/tokens/account_id")
            }
            .and_then(Value::as_str)
            .unwrap_or("");
            let id = if id.is_empty() {
                root.to_string_lossy().to_string()
            } else {
                id.to_string()
            };
            if !seen_ids.insert(id.clone()) {
                continue;
            }
            let label = if provider == "claude" {
                identity
                    .pointer("/oauthAccount/emailAddress")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else if provider == "codex" {
                creds
                    .pointer("/tokens/id_token")
                    .and_then(Value::as_str)
                    .and_then(|s| s.split('.').nth(1))
                    .and_then(|s| {
                        base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .decode(s)
                            .ok()
                    })
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .and_then(|v| v.get("email").and_then(Value::as_str).map(str::to_owned))
            } else {
                None
            }
            .unwrap_or_else(|| {
                root.file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix(prefix))
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        match provider {
                            "claude" => "Claude",
                            "codex" => "Codex",
                            _ => "Gemini",
                        }
                        .into()
                    })
            });
            all.push(Account {
                provider: provider.into(),
                root,
                dir,
                id,
                label,
                registered,
            });
        }
    }
    Ok(all)
}
fn list_accounts(app: &AppHandle) -> Result<Value, String> {
    let accounts = discover(app)?;
    let results = std::thread::scope(|scope| {
        let jobs:Vec<_>=accounts.iter().map(|a|scope.spawn(move||{let provider=a.provider.as_str();let (_,_,prefix,file)=spec(provider).unwrap();let creds=read_json(a.dir.join(file));let mut logged=match provider{"claude"=>creds.pointer("/claudeAiOauth/accessToken"),"codex"=>creds.pointer("/tokens/access_token"),_=>creds.get("access_token")}.and_then(Value::as_str).is_some_and(|s|!s.is_empty());let mut label=a.label.clone();let mut plan=Value::Null;
 let args=match provider{"claude"=>vec!["auth","status","--json"],"codex"=>vec!["login","status"],_=>vec![]};if !args.is_empty(){if let Ok(out)=provider_command(provider,&a.root,&args){let stdout=String::from_utf8_lossy(&out.stdout);if provider=="claude"{if let Ok(v)=serde_json::from_str::<Value>(&stdout){logged=v["loggedIn"]==true;if let Some(s)=v["email"].as_str(){label=s.into();}plan=v["subscriptionType"].clone();}}else{let output=format!("{}{}",stdout,String::from_utf8_lossy(&out.stderr)).to_lowercase();if output.contains("logged in"){logged = !output.contains("not logged in");}}}}
 let name=a.root.file_name().and_then(|s|s.to_str()).and_then(|s|s.strip_prefix(prefix));json!({"provider":provider,"accountRoot":a.root,"accountName":name,"label":label,"isLoggedIn":logged,"planName":plan,"isRemovable":a.registered})})).collect();
        jobs.into_iter()
            .map(|j| j.join().map_err(|_| "AI account reader failed".to_string()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(json!(results))
}
fn reset_at(raw: &Value) -> Value {
    if let Some(s) = raw.as_str() {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
            .unwrap_or(Value::Null)
    } else if let Some(n) = raw.as_f64() {
        DateTime::<Utc>::from_timestamp_millis(if n > 1e12 {
            n as i64
        } else {
            (n * 1000.0) as i64
        })
        .map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
        .unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}
fn window(kind: &str, used: &Value, reset: &Value, model: Value) -> Option<Value> {
    let n = used.as_f64()?;
    if !n.is_finite() {
        return None;
    }
    Some(
        json!({"kind":kind,"modelName":model,"usedPercent":n.clamp(0.0,100.0),"resetsAt":reset_at(reset)}),
    )
}
fn windows(provider: &str, payload: &Value) -> Vec<Value> {
    let mut result = Vec::new();
    match provider {
        "claude" => {
            if let Some(limits) = payload["limits"].as_array() {
                for limit in limits {
                    let kind = match text(limit, "kind") {
                        "session" => "session",
                        "weekly_all" | "weekly_scoped" => "weekly",
                        _ => continue,
                    };
                    let model = if text(limit, "kind") == "weekly_scoped" {
                        let v = limit
                            .pointer("/scope/model/display_name")
                            .cloned()
                            .unwrap_or(Value::Null);
                        if v.as_str().is_none_or(|s| s.trim().is_empty()) {
                            continue;
                        }
                        v
                    } else {
                        Value::Null
                    };
                    if let Some(w) = window(kind, &limit["percent"], &limit["resets_at"], model) {
                        result.push(w)
                    }
                }
            }
            if result.is_empty() {
                for (key, kind) in [("five_hour", "session"), ("seven_day", "weekly")] {
                    let raw = &payload[key];
                    if let Some(w) = window(
                        kind,
                        raw.get("utilization").unwrap_or(&raw["used_percentage"]),
                        &raw["resets_at"],
                        Value::Null,
                    ) {
                        result.push(w)
                    }
                }
            }
        }
        "codex" => {
            for (seconds, kind) in [
                (18000.0, "session"),
                (604800.0, "weekly"),
                (2592000.0, "monthly"),
            ] {
                for key in ["primary_window", "secondary_window"] {
                    let raw = &payload["rate_limit"][key];
                    if raw["limit_window_seconds"]
                        .as_f64()
                        .is_some_and(|n| (n - seconds).abs() <= 60.0)
                    {
                        if let Some(w) =
                            window(kind, &raw["used_percent"], &raw["reset_at"], Value::Null)
                        {
                            result.push(w);
                            break;
                        }
                    }
                }
            }
        }
        "gemini" => {
            let buckets = payload.as_array().or_else(|| payload["buckets"].as_array());
            if let Some(buckets) = buckets {
                for b in buckets {
                    if let Some(remaining) = b["remainingFraction"].as_f64() {
                        let model = model_name(text(b, "modelId"));
                        if let Some(w) = window(
                            "model",
                            &json!(((1.0 - remaining) * 100.0).round()),
                            &b["resetTime"],
                            json!(model),
                        ) {
                            result.push(w)
                        }
                    }
                }
            }
        }
        _ => {}
    }
    result
}
fn failure_status(status: u16) -> (&'static str, &'static str) {
    match status {
        401 | 403 => ("unavailable", "expired-credentials"),
        429 => ("error", "rate-limited"),
        _ => ("error", "fetch-failed"),
    }
}
fn usage_result(a: &Account, status: &str, reason: Value, w: Vec<Value>, plan: Value) -> Value {
    json!({"provider":a.provider,"accountId":a.id,"label":a.label,"status":status,"reason":reason,"windows":w,"planName":plan,"fetchedAt":now()})
}
fn apply_claude_token(credentials: &Value, response: &Value, now: i64) -> Option<Value> {
    let token = response["access_token"]
        .as_str()
        .filter(|s| !s.trim().is_empty())?;
    let mut updated = credentials.clone();
    if !updated["claudeAiOauth"].is_object() {
        return None;
    }
    updated["claudeAiOauth"]["accessToken"] = json!(token);
    if let Some(seconds) = response["expires_in"].as_f64().filter(|n| n.is_finite()) {
        updated["claudeAiOauth"]["expiresAt"] = json!(now + (seconds * 1000.0) as i64);
    }
    if let Some(refresh) = response["refresh_token"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
    {
        updated["claudeAiOauth"]["refreshToken"] = json!(refresh);
    }
    if let Some(scope) = response["scope"].as_str().filter(|s| !s.trim().is_empty()) {
        updated["claudeAiOauth"]["scopes"] = json!(scope.split(' ').collect::<Vec<_>>());
    }
    Some(updated)
}
fn refresh_claude(credentials: &Value) -> Option<Value> {
    let refresh = credentials
        .pointer("/claudeAiOauth/refreshToken")?
        .as_str()?;
    let response = client()
        .ok()?
        .post("https://platform.claude.com/v1/oauth/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", "9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
        ])
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .ok()?;
    apply_claude_token(credentials, &response, Utc::now().timestamp_millis())
}
fn atomic_credentials(path: &Path, credentials: &Value) -> Result<(), String> {
    let temp = path.with_file_name(format!(".credentials.kanvibe-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp).map_err(|e| e.to_string())?;
        file.write_all(credentials.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn model_name(model: &str) -> String {
    match model {
        "gemini-3.1-pro" => "3.1 Pro".into(),
        "gemini-3.1-flash" => "3.1 Flash".into(),
        "gemini-3.0-pro" => "3.0 Pro".into(),
        "gemini-3.0-flash" => "3.0 Flash".into(),
        "gemini-2.5-pro" => "Pro".into(),
        "gemini-2.5-flash" => "Flash".into(),
        "gemini-2.5-flash-lite" => "Flash Lite".into(),
        _ => model
            .trim_start_matches("gemini-")
            .split('-')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}
static CLAUDE_BACKOFF: OnceLock<Mutex<i64>> = OnceLock::new();
fn read_usage(a: &Account) -> Value {
    let fail =
        |status: &str, reason: &str| usage_result(a, status, json!(reason), vec![], Value::Null);
    let provider = a.provider.as_str();
    let file = spec(provider).unwrap().3;
    let mut credentials = read_json(a.dir.join(file));
    let mut plan = Value::Null;
    let token;
    if provider == "claude" {
        let (kc, unreadable) = keychain(&a.root);
        let expires = |v: &Value| {
            v.pointer("/claudeAiOauth/expiresAt")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        };
        let keychain_selected =
            credentials.is_null() && !kc.is_null() || expires(&kc) > expires(&credentials);
        if keychain_selected {
            credentials = kc;
        }
        if credentials["claudeAiOauth"].is_object()
            && expires(&credentials) <= Utc::now().timestamp_millis() + 300000
        {
            if !keychain_selected {
                if let Some(refreshed) = refresh_claude(&credentials) {
                    if let Err(error) = atomic_credentials(&a.dir.join(file), &refreshed) {
                        eprintln!("Cannot persist refreshed AI credentials: {error}");
                    }
                    credentials = refreshed;
                } else {
                    let _ = provider_command(provider, &a.root, &["auth", "status", "--json"]);
                    let reread = read_json(a.dir.join(file));
                    if !reread.is_null() {
                        credentials = reread;
                    }
                }
            } else {
                let _ = provider_command(provider, &a.root, &["auth", "status", "--json"]);
                let (reread, _) = keychain(&a.root);
                if !reread.is_null() {
                    credentials = reread;
                }
            }
        }
        token = credentials
            .pointer("/claudeAiOauth/accessToken")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        plan = credentials
            .pointer("/claudeAiOauth/subscriptionType")
            .cloned()
            .unwrap_or(Value::Null);
        if token.is_empty() {
            return fail(
                "unavailable",
                if unreadable {
                    "keychain-unreadable"
                } else {
                    "missing-credentials"
                },
            );
        }
        if *CLAUDE_BACKOFF.get_or_init(|| Mutex::new(0)).lock().unwrap()
            > Utc::now().timestamp_millis()
        {
            return fail("error", "rate-limited");
        }
    } else if provider == "codex" {
        token = credentials
            .pointer("/tokens/access_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    } else {
        token = text(&credentials, "access_token").to_string();
    }
    if token.is_empty() {
        return fail("unavailable", "missing-credentials");
    }
    let client = match client() {
        Ok(c) => c,
        Err(_) => return fail("error", "fetch-failed"),
    };
    let request = match provider {
        "claude" => client
            .get("https://api.anthropic.com/api/oauth/usage")
            .bearer_auth(&token)
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-code/2.1.0"),
        "codex" => {
            let mut r = client
                .get("https://chatgpt.com/backend-api/wham/usage")
                .bearer_auth(&token);
            if let Some(id) = credentials
                .pointer("/tokens/account_id")
                .and_then(Value::as_str)
            {
                r = r.header("ChatGPT-Account-Id", id)
            }
            r
        }
        _ => {
            let token = if credentials["expiry_date"].as_i64().unwrap_or(0)
                <= Utc::now().timestamp_millis()
            {
                match gemini_refresh(&client, &credentials) {
                    Ok(token) => token,
                    Err(reason) => return fail("unavailable", &reason),
                }
            } else {
                token
            };
            let load = client
                .post("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist")
                .bearer_auth(&token)
                .json(&json!({}))
                .send()
                .ok()
                .and_then(|r| r.json::<Value>().ok());
            let project = load
                .as_ref()
                .and_then(|p| p["cloudaicompanionProject"].as_str());
            let Some(project) = project else {
                return fail("error", "fetch-failed");
            };
            client
                .post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota")
                .bearer_auth(&token)
                .json(&json!({"project":project}))
        }
    };
    match request.send() {
        Ok(response) if response.status().is_success() => match response.json::<Value>() {
            Ok(payload) => {
                if provider == "codex" {
                    plan = payload["plan_type"].clone();
                }
                let windows = windows(provider, &payload);
                if windows.is_empty() {
                    fail("error", "empty-response")
                } else {
                    usage_result(a, "ok", Value::Null, windows, plan)
                }
            }
            Err(_) => fail("error", "fetch-failed"),
        },
        Ok(response) => {
            if provider == "claude" && response.status().as_u16() == 429 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(300);
                *CLAUDE_BACKOFF.get_or_init(|| Mutex::new(0)).lock().unwrap() =
                    Utc::now().timestamp_millis() + delay * 1000;
            }
            let (status, reason) = failure_status(response.status().as_u16());
            fail(status, reason)
        }
        Err(_) => fail("error", "fetch-failed"),
    }
}
fn gemini_oauth_client() -> Result<(String, String), String> {
    let dirs = crate::runtime::shell_environment()
        .get("PATH")
        .map(|s| std::env::split_paths(s).collect::<Vec<_>>())
        .unwrap_or_default();
    let binary = dirs
        .into_iter()
        .chain([
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            home().join(".local/bin"),
        ])
        .map(|d| d.join("gemini"))
        .find(|p| p.exists())
        .ok_or("gemini-cli-not-found")?;
    let binary = fs::canonicalize(&binary).unwrap_or(binary);
    let mut ancestor = binary
        .parent()
        .and_then(Path::parent)
        .ok_or("gemini-cli-not-found")?;
    let mut source = None;
    for _ in 0..4 {
        let candidate =
            ancestor.join("node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js");
        if let Ok(s) = fs::read_to_string(candidate) {
            source = Some(s);
            break;
        }
        let Some(p) = ancestor.parent() else { break };
        ancestor = p;
    }
    let source = source.ok_or("gemini-cli-not-found")?;
    let extract = |key: &str| -> Option<String> {
        let tail = source.split(key).nth(1)?.split('=').nth(1)?.trim();
        let quote = tail.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        Some(tail[1..].split(quote).next()?.to_string())
    };
    let id = extract("OAUTH_CLIENT_ID").ok_or("gemini-cli-not-found")?;
    let secret = extract("OAUTH_CLIENT_SECRET").ok_or("gemini-cli-not-found")?;
    Ok((id, secret))
}
fn gemini_refresh(client: &Client, creds: &Value) -> Result<String, String> {
    let (id, secret) = gemini_oauth_client()?;
    client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", text(creds, "refresh_token")),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.json::<Value>().ok())
        .and_then(|v| v["access_token"].as_str().map(str::to_owned))
        .ok_or("expired-credentials".into())
}
fn cached_snapshot(app: &AppHandle) -> Result<Value, String> {
    Ok(setting(app, "ai_usage_snapshot_cache")?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(|v| v["version"] == 1)
        .map(|v| v["snapshot"].clone())
        .filter(|v| v["accounts"].is_array() && v["fetchedAt"].is_string())
        .unwrap_or(Value::Null))
}
static USAGE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static LAST_USAGE: OnceLock<Mutex<Option<(std::time::Instant, Value)>>> = OnceLock::new();
fn usage_snapshot(app: &AppHandle) -> Result<Value, String> {
    let _guard = USAGE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some((instant, snapshot)) = LAST_USAGE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
    {
        if instant.elapsed() < Duration::from_secs(2) {
            return Ok(snapshot.clone());
        }
    }
    let accounts = discover(app)?;
    let cached = cached_snapshot(app)?;
    let results = std::thread::scope(|scope| {
        let jobs: Vec<_> = accounts
            .iter()
            .map(|a| scope.spawn(move || read_usage(a)))
            .collect();
        jobs.into_iter()
            .map(|j| j.join().map_err(|_| "Usage reader failed".to_string()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut results = results;
    for result in &mut results {
        if result["windows"].as_array().is_some_and(Vec::is_empty)
            && ![
                "missing-credentials",
                "expired-credentials",
                "keychain-unreadable",
                "gemini-cli-not-found",
            ]
            .contains(&text(result, "reason"))
        {
            let id = text(result, "accountId");
            let hash = format!("{:x}", Sha256::digest(id.as_bytes()))[..16].to_string();
            if let Some(prior) = cached["accounts"].as_array().and_then(|v| {
                v.iter().find(|v| {
                    v["provider"] == result["provider"]
                        && (text(v, "accountId") == id || text(v, "accountId") == hash)
                })
            }) {
                let live: Vec<Value> = prior["windows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|w| {
                        w["resetsAt"].is_null()
                            || w["resetsAt"]
                                .as_str()
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .is_some_and(|d| d > Utc::now())
                    })
                    .cloned()
                    .collect();
                if !live.is_empty() {
                    result["windows"] = json!(live);
                    result["fetchedAt"] = prior["fetchedAt"].clone();
                    if result["planName"].is_null() {
                        result["planName"] = prior["planName"].clone();
                    }
                }
            }
        }
    }
    let snapshot = json!({"accounts":results,"fetchedAt":now()});
    let mut persist = snapshot.clone();
    for a in persist["accounts"].as_array_mut().unwrap() {
        a["accountId"] = json!(
            format!("{:x}", Sha256::digest(text(a, "accountId").as_bytes()))[..16].to_string()
        );
    }
    if let Err(e) = set_setting(
        app,
        "ai_usage_snapshot_cache",
        &json!({"version":1,"snapshot":persist}).to_string(),
    ) {
        eprintln!("AI usage cache: {e}");
    }
    *LAST_USAGE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|e| e.to_string())? = Some((std::time::Instant::now(), snapshot.clone()));
    Ok(snapshot)
}
/// 계정 목록은 설정 한 칸을 통째로 읽고 쓴다. 로그아웃을 기다리는 동안 추가된 계정을 낡은 목록으로 덮지 않도록 변경을 줄 세운다.
fn serialize_account_mutation<Outcome>(mutation: impl FnOnce() -> Outcome) -> Outcome {
    static ACCOUNT_MUTATION: Mutex<()> = Mutex::new(());
    let _guard = ACCOUNT_MUTATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    mutation()
}
fn add_account(
    db: &crate::database::Database,
    home: &Path,
    provider: &str,
    name: &str,
) -> Result<Value, String> {
    let (_, _, prefix, _) = spec(provider)?;
    if !valid_name(name) {
        return Ok(json!({"outcome":"invalid-name"}));
    }
    let root = home.join(format!("{prefix}{name}"));
    let mut regs = stored_registrations(db)?;
    if regs
        .iter()
        .any(|r| text(r, "accountRoot") == root.to_string_lossy())
    {
        return Ok(json!({"outcome":"already-exists"}));
    }
    fs::create_dir_all(config(provider, &root)).map_err(|e| e.to_string())?;
    regs.push(json!({"provider":provider,"accountRoot":root,"accountName":name}));
    db.set_setting("ai_accounts", &json!(regs).to_string())?;
    Ok(json!({"outcome":"ok","accountRoot":root}))
}
fn logout_account(provider: &str, root: &Path) -> bool {
    if provider == "gemini" {
        return false;
    }
    provider_command(
        provider,
        root,
        if provider == "claude" {
            &["auth", "logout"]
        } else {
            &["logout"]
        },
    )
    .is_ok_and(|out| out.status.success())
}
fn remove_account(
    db: &crate::database::Database,
    home: &Path,
    provider: &str,
    root: &Path,
    logout: impl FnOnce() -> bool,
) -> Result<Value, String> {
    let (_, _, prefix, _) = spec(provider)?;
    let mut regs = stored_registrations(db)?;
    let owned = regs.iter().any(|r| {
        text(r, "provider") == provider && text(r, "accountRoot") == root.to_string_lossy()
    });
    let did_logout = logout();
    if owned {
        let valid = root.parent() == Some(home)
            && root
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix(prefix))
                .is_some_and(valid_name);
        if !valid {
            return Err("Refusing removal outside KanVibe account roots".into());
        }
        if root.exists() {
            fs::remove_dir_all(root).map_err(|e| e.to_string())?;
        }
        regs.retain(|r| text(r, "accountRoot") != root.to_string_lossy());
        db.set_setting("ai_accounts", &json!(regs).to_string())?;
    }
    Ok(json!({"outcome":if owned||did_logout{"ok"}else{"manual-logout-required"}}))
}
pub fn dispatch(
    app: &AppHandle,
    namespace: &str,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    match (namespace, method) {
        ("aiAccounts", "getAiAccountRegistrations") => Ok(json!(registrations(app)?)),
        ("aiAccounts", "listAiAccounts") => list_accounts(app),
        ("aiAccounts", "addAiAccount") => {
            let provider = args.first().and_then(Value::as_str).unwrap_or("");
            let name = args.get(1).and_then(Value::as_str).unwrap_or("");
            serialize_account_mutation(|| {
                add_account(
                    &app.state::<crate::database::Database>(),
                    &home(),
                    provider,
                    name,
                )
            })
        }
        ("aiAccounts", "removeAiAccount") => {
            let provider = args.first().and_then(Value::as_str).unwrap_or("");
            let root = PathBuf::from(args.get(1).and_then(Value::as_str).unwrap_or(""));
            serialize_account_mutation(|| {
                remove_account(
                    &app.state::<crate::database::Database>(),
                    &home(),
                    provider,
                    &root,
                    || logout_account(provider, &root),
                )
            })
        }
        ("aiUsage", "getCachedAiUsageSnapshot") => cached_snapshot(app),
        ("aiUsage", "getAiUsageSnapshot") => usage_snapshot(app),
        _ => Err(format!("Unknown AI method {namespace}.{method}")),
    }
}
pub fn project_dispatch(app: &AppHandle, method: &str, args: Vec<Value>) -> Result<Value, String> {
    sessions::dispatch(app, method, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_account_path_traversal() {
        for name in ["", "../x", ".hidden", "a/b", "a\\b"] {
            assert!(!valid_name(name));
        }
        assert!(valid_name("work-2"));
    }
    #[test]
    fn codex_window_duration_wins_over_position() {
        let w = windows(
            "codex",
            &json!({"rate_limit":{"primary_window":{"limit_window_seconds":604800,"used_percent":15,"reset_at":2000000000},"secondary_window":{"limit_window_seconds":18000,"used_percent":130,"reset_at":2000000000}}}),
        );
        assert_eq!(w[0]["kind"], "session");
        assert_eq!(w[0]["usedPercent"], 100.0);
        assert_eq!(w[1]["kind"], "weekly");
    }
    #[test]
    fn provider_failure_retains_signin_semantics() {
        assert_eq!(failure_status(401), ("unavailable", "expired-credentials"));
        assert_eq!(failure_status(429), ("error", "rate-limited"));
    }
    #[test]
    fn claude_model_weekly_limits_remain_weekly() {
        let w = windows(
            "claude",
            &json!({"limits":[{"kind":"weekly_scoped","percent":12,"scope":{"model":{"display_name":"Sonnet"}}}]}),
        );
        assert_eq!(w[0]["kind"], "weekly");
        assert_eq!(w[0]["modelName"], "Sonnet");
    }
}
#[cfg(test)]
mod migration_regressions {
    use super::*;
    #[test]
    fn refreshed_token_preserves_unrelated_secrets_and_rotates_refresh() {
        let original = json!({"mcp":{"secret":"keep"},"claudeAiOauth":{"accessToken":"old","refreshToken":"previous","subscriptionType":"max","scopes":["a"]}});
        let updated=apply_claude_token(&original,&json!({"access_token":"fresh","refresh_token":"rotated","expires_in":60,"scope":"a b"}),1000).unwrap();
        assert_eq!(updated["mcp"], original["mcp"]);
        assert_eq!(updated["claudeAiOauth"]["refreshToken"], "rotated");
        assert_eq!(updated["claudeAiOauth"]["subscriptionType"], "max");
        assert_eq!(updated["claudeAiOauth"]["expiresAt"], 61000);
        assert_eq!(updated["claudeAiOauth"]["scopes"], json!(["a", "b"]));
    }
    #[test]
    fn absent_refresh_rotation_preserves_current_refresh() {
        let original = json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"keep"}});
        let updated =
            apply_claude_token(&original, &json!({"access_token":"fresh"}), 1000).unwrap();
        assert_eq!(updated["claudeAiOauth"]["refreshToken"], "keep");
        assert!(apply_claude_token(&original, &json!({"access_token":""}), 1000).is_none());
    }
    #[test]
    fn credentials_are_owner_only_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(&path, "old").unwrap();
        let value = json!({"claudeAiOauth":{"accessToken":"new"}});
        atomic_credentials(&path, &value).unwrap();
        assert_eq!(read_json(&path), value);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn concurrent_credential_writes_leave_one_complete_file_without_temporaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        let bodies: Vec<Value> = (0..16)
            .map(|index| json!({"index":index,"token":"x".repeat(10000)}))
            .collect();
        std::thread::scope(|scope| {
            for body in &bodies {
                let path = &path;
                scope.spawn(move || atomic_credentials(path, body).unwrap());
            }
        });
        assert!(bodies.contains(&read_json(&path)));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    fn account_names(db: &crate::database::Database) -> Vec<String> {
        stored_registrations(db)
            .unwrap()
            .iter()
            .map(|r| text(r, "accountName").to_string())
            .collect()
    }
    fn account_fixture() -> (tempfile::TempDir, crate::database::Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::Database::open(&dir.path().join("kanvibe.db")).unwrap();
        (dir, db)
    }
    #[test]
    fn concurrent_additions_keep_distinct_accounts_and_register_one_duplicate() {
        let (dir, db) = account_fixture();
        let add = |provider: &'static str, name: &'static str| {
            let (db, home) = (&db, dir.path());
            move || serialize_account_mutation(|| add_account(db, home, provider, name).unwrap())
        };
        let outcomes = std::thread::scope(|scope| {
            [
                scope.spawn(add("claude", "first")),
                scope.spawn(add("codex", "second")),
                scope.spawn(add("claude", "same")),
                scope.spawn(add("claude", "same")),
            ]
            .map(|handle| handle.join().unwrap()["outcome"].clone())
        });
        assert_eq!(outcomes[0], "ok");
        assert_eq!(outcomes[1], "ok");
        assert_eq!(
            outcomes[2..].iter().filter(|o| **o == "ok").count(),
            1,
            "동일 계정은 한 요청만 등록한다"
        );
        let mut names = account_names(&db);
        names.sort();
        assert_eq!(names, ["first", "same", "second"]);
    }
    #[test]
    fn addition_during_slow_logout_survives_removal() {
        let (dir, db) = account_fixture();
        let added = add_account(&db, dir.path(), "claude", "old").unwrap();
        let root = PathBuf::from(text(&added, "accountRoot"));
        let (logout_started, wait_logout) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let removal = scope.spawn(|| {
                serialize_account_mutation(|| {
                    remove_account(&db, dir.path(), "claude", &root, || {
                        logout_started.send(()).unwrap();
                        std::thread::sleep(Duration::from_millis(100));
                        true
                    })
                    .unwrap()
                })
            });
            wait_logout.recv().unwrap();
            let addition = scope.spawn(|| {
                serialize_account_mutation(|| add_account(&db, dir.path(), "codex", "new").unwrap())
            });
            assert_eq!(removal.join().unwrap()["outcome"], "ok");
            assert_eq!(addition.join().unwrap()["outcome"], "ok");
        });
        assert_eq!(account_names(&db), ["new"]);
    }
    #[test]
    fn failed_account_mutation_does_not_block_the_next_one() {
        let failed: Result<(), String> = serialize_account_mutation(|| Err("save failed".into()));
        assert!(failed.is_err());
        let panicked =
            std::panic::catch_unwind(|| serialize_account_mutation(|| panic!("mutation")));
        assert!(panicked.is_err());
        assert_eq!(serialize_account_mutation(|| "survived"), "survived");
    }
    #[test]
    fn gemini_model_labels_and_remaining_fraction_match_ui() {
        let w = windows(
            "gemini",
            &json!({"buckets":[{"modelId":"gemini-2.5-pro","remainingFraction":0.25,"resetTime":"2026-08-10T07:00:00.000Z"},{"modelId":"gemini-4.0-ultra","remainingFraction":1.0,"resetTime":"2026-08-10T07:00:00.000Z"}]}),
        );
        assert_eq!(w[0]["modelName"], "Pro");
        assert_eq!(w[0]["usedPercent"], 75.0);
        assert_eq!(w[1]["modelName"], "4.0 Ultra");
        assert_eq!(w[1]["usedPercent"], 0.0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod provider_environment_regressions {
    use super::*;
    #[test]
    fn finder_cli_status_and_logout_use_safe_augmented_path() {
        if let Some(root) = std::env::var_os("KANVIBE_PROVIDER_ENV_TEST") {
            for args in [&["auth", "status", "--json"][..], &["auth", "logout"][..]] {
                let out = provider_command("claude", &PathBuf::from(&root).join("account"), args)
                    .unwrap();
                assert!(out.status.success());
                let output = String::from_utf8(out.stdout).unwrap();
                assert!(output.contains("cli-found"));
                assert!(output.contains("account"));
                assert!(!output.contains("leaked"));
            }
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let cli = bin.join("claude");
        fs::write(&cli, "#!/bin/sh\nprintf 'cli-found %s' \"$CLAUDE_CONFIG_DIR\"\n[ -n \"$PORT$HOST$NODE_ENV$KANVIBE_PROVIDER_ENV_TEST\" ] && printf leaked\nexit 0\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ai::provider_environment_regressions::finder_cli_status_and_logout_use_safe_augmented_path", "--nocapture"])
            .env("HOME", dir.path()).env("PATH", "/usr/bin:/bin")
            .env("PORT", "leaked").env("HOST", "leaked").env("NODE_ENV", "leaked")
            .env("KANVIBE_PROVIDER_ENV_TEST", dir.path()).status().unwrap();
        assert!(status.success());
    }
}

#[cfg(all(test, unix))]
mod gemini_lookup_regressions {
    use super::*;

    #[test]
    fn gui_path_finds_home_bin_gemini_oauth_bundle() {
        if std::env::var_os("KANVIBE_GEMINI_LOOKUP_TEST").is_some() {
            assert_eq!(
                gemini_oauth_client().unwrap(),
                ("fixture-client".into(), "fixture-secret".into())
            );
            return;
        }
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let package = dir.path().join("lib/node_modules/@google/gemini-cli");
        let bundle = dir
            .path()
            .join("lib/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(package.join("dist")).unwrap();
        fs::create_dir_all(bundle.parent().unwrap()).unwrap();
        let cli = package.join("dist/index.js");
        fs::write(&cli, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&cli, bin.join("gemini")).unwrap();
        fs::write(
            bundle,
            "const OAUTH_CLIENT_ID = 'fixture-client';\nconst OAUTH_CLIENT_SECRET = \"fixture-secret\";",
        ).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ai::gemini_lookup_regressions::gui_path_finds_home_bin_gemini_oauth_bundle",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", dir.path())
            .env("PATH", "/usr/bin:/bin")
            .env("KANVIBE_GEMINI_LOOKUP_TEST", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated Gemini lookup failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
