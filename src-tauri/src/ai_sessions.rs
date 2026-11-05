use super::{home, now, text};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use tauri::{AppHandle, Manager};
#[derive(Clone)]
struct Context {
    target: String,
    repo: String,
    host: Option<String>,
    home: String,
    bounded: bool,
    recent: bool,
}
#[derive(Clone)]
struct Transcript {
    summary: Value,
    messages: Vec<Value>,
    events: Vec<Value>,
    mtime: i64,
    parent: Option<String>,
}
type FileCache = HashMap<String, (SystemTime, u64, Vec<Value>)>;
static CACHE: OnceLock<Mutex<FileCache>> = OnceLock::new();
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn command(ctx: &Context, cmd: &str) -> Result<String, String> {
    crate::git::run(cmd, ctx.host.as_deref())
}
fn content(ctx: &Context, path: &str) -> Result<String, String> {
    if ctx.host.is_some() {
        command(ctx, &format!("cat -- {}", quote(path)))
    } else {
        fs::read_to_string(path).map_err(|e| e.to_string())
    }
}
fn files(ctx: &Context, root: &str, suffix: &str) -> Vec<String> {
    if ctx.host.is_some() {
        command(
            ctx,
            &format!(
                "find {} -type f -name {} {recent} 2>/dev/null",
                quote(root),
                quote(&format!("*{suffix}")),
                recent = if ctx.recent { "-mmin -360" } else { "" }
            ),
        )
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
    } else {
        walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file() && e.path().to_string_lossy().ends_with(suffix))
            .map(|e| e.path().to_string_lossy().to_string())
            .collect()
    }
}
fn lines(ctx: &Context, path: &str) -> Vec<Value> {
    if ctx.host.is_some() {
        return content(ctx, path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
    }
    let Ok(meta) = fs::metadata(path) else {
        return vec![];
    };
    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((time, len, parsed)) = cache.lock().unwrap().get(path) {
        if *time == modified && *len == meta.len() {
            return parsed.clone();
        }
    }
    let parsed: Vec<Value> = fs::File::open(path)
        .ok()
        .into_iter()
        .flat_map(|f| BufReader::new(f).lines().map_while(Result::ok))
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    let mut cache = cache.lock().unwrap();
    if cache.len() >= 200 {
        cache.clear();
    }
    cache.insert(path.into(), (modified, meta.len(), parsed.clone()));
    parsed
}
fn window_lines(ctx: &Context, path: &str) -> Vec<Value> {
    let source = if ctx.host.is_some() {
        command(
            ctx,
            &format!(
                "head -n 60 -- {path}; tail -n 60 -- {path}",
                path = quote(path)
            ),
        )
        .unwrap_or_default()
    } else {
        let Ok(mut file) = fs::File::open(path) else {
            return vec![];
        };
        let mut head = String::new();
        {
            let mut reader = BufReader::new(&mut file);
            for _ in 0..60 {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                head.push_str(&line);
            }
        }
        let length = file.metadata().map(|m| m.len()).unwrap_or(0);
        let start = length.saturating_sub(256 * 1024);
        let _ = file.seek(SeekFrom::Start(start));
        let mut tail = Vec::new();
        let _ = file.read_to_end(&mut tail);
        let tail = String::from_utf8_lossy(&tail);
        let mut lines: Vec<_> = tail.lines().collect();
        if start > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        let from = lines.len().saturating_sub(60);
        format!("{head}\n{}", lines[from..].join("\n"))
    };
    let mut seen = HashSet::new();
    source
        .lines()
        .filter(|s| seen.insert((*s).to_string()))
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(plain)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(o) => {
            for key in [
                "text",
                "content",
                "output",
                "input",
                "message",
                "summary",
                "thinking",
                "description",
            ] {
                if let Some(v) = o.get(key) {
                    let s = plain(v);
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
            if o.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| ["tool_use", "tool_call", "function_call"].contains(&t))
            {
                format!(
                    "{}: {}",
                    o.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    o.get("input").unwrap_or(&Value::Null)
                )
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}
fn trunc(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let shortened = chars.by_ref().take(max).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}
fn iso(v: &Value) -> Value {
    if let Some(s) = v.as_str() {
        DateTime::parse_from_rfc3339(s)
            .map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
            .unwrap_or(Value::Null)
    } else if let Some(n) = v.as_i64() {
        DateTime::<Utc>::from_timestamp_millis(n)
            .map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}
fn message(role: &str, timestamp: &Value, s: String) -> Option<Value> {
    if s.trim().is_empty() {
        return None;
    }
    let normalized = s.replace('\r', "");
    let shortened = trunc(&normalized, 1200);
    Some(
        json!({"role":role,"timestamp":iso(timestamp),"text":shortened,"fullText":normalized,"isTruncated":normalized.chars().count()>1200}),
    )
}
fn matches_path(ctx: &Context, path: &str) -> bool {
    let normalize = |s: &str| {
        let mut normalized = std::path::PathBuf::new();
        for component in Path::new(s).components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                other => normalized.push(other.as_os_str()),
            }
        }
        normalized
    };
    !path.is_empty()
        && !ctx.target.is_empty()
        && normalize(path).starts_with(normalize(&ctx.target))
}
fn skip(role: &str, s: &str) -> bool {
    let s = s.trim();
    (role == "user"
        && ["<environment_context>", "<skill>", "<system-reminder>"]
            .iter()
            .any(|p| s.starts_with(p)))
        || (role == "developer"
            && [
                "<permissions instructions>",
                "<apps_instructions>",
                "<skills_instructions>",
                "<collaboration_mode>",
            ]
            .iter()
            .any(|p| s.contains(p)))
}
fn parse_jsonl(ctx: &Context, provider: &str, path: &str) -> Option<Transcript> {
    let events = if ctx.bounded && (provider == "claude" || ctx.recent) {
        window_lines(ctx, path)
    } else {
        lines(ctx, path)
    };
    let mut id = String::new();
    let mut cwd = String::new();
    let mut start = Value::Null;
    let mut updated = Value::Null;
    let mut messages = Vec::new();
    let mut seen = HashSet::new();
    let mut parent = None;
    for event in &events {
        if provider == "codex" && text(event, "type") == "session_meta" {
            let p = &event["payload"];
            cwd = text(p, "cwd").into();
            id = text(p, "id").into();
            start = iso(p.get("timestamp").unwrap_or(&event["timestamp"]));
            parent = p
                .get("parent_thread_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            continue;
        }
        if provider == "claude" {
            if id.is_empty() {
                id = text(event, "sessionId").into();
            }
            if cwd.is_empty() {
                cwd = text(event, "cwd").into();
            }
            if start.is_null() {
                start = iso(&event["timestamp"]);
            }
        }
        let timestamp = &event["timestamp"];
        let mut pairs = Vec::new();
        if provider == "claude" {
            let m = &event["message"];
            let mut role = text(m, "role");
            if text(event, "type") == "system" {
                role = "system"
            }
            if m["content"]
                .as_array()
                .is_some_and(|a| a.iter().any(|p| text(p, "type") == "tool_result"))
            {
                role = "tool"
            }
            if role.is_empty() {
                role = "unknown"
            }
            pairs.push((role.to_string(), plain(&m["content"])));
        } else {
            let p = &event["payload"];
            if text(event, "type") == "response_item" {
                let t = text(p, "type");
                let role = match t {
                    "message" => text(p, "role"),
                    "reasoning" => "reasoning",
                    "function_call" | "tool_call" | "function_call_output" | "tool_result" => {
                        "tool"
                    }
                    _ => "unknown",
                };
                let s = match t {
                    "message" => plain(&p["content"]),
                    "reasoning" => plain(
                        p.get("summary")
                            .or_else(|| p.get("content"))
                            .unwrap_or(&p["text"]),
                    ),
                    "function_call" | "tool_call" => format!(
                        "{}: {}",
                        text(p, "name"),
                        plain(p.get("arguments").unwrap_or(&p["input"]))
                    ),
                    "function_call_output" | "tool_result" => {
                        plain(p.get("output").unwrap_or(&p["content"]))
                    }
                    _ => plain(p),
                };
                pairs.push((role.into(), s));
            } else if text(event, "type") == "event_msg" && text(p, "type") == "agent_message" {
                pairs.push(("assistant".into(), plain(&p["message"])));
            }
        }
        for (role, s) in pairs {
            if skip(&role, &s) {
                continue;
            }
            let key = format!("{role}\0{timestamp}\0{s}");
            if !seen.insert(key) {
                continue;
            }
            if let Some(m) = message(&role, timestamp, s) {
                if role == "user" || role == "assistant" {
                    updated = m["timestamp"].clone();
                }
                messages.push(m);
            }
        }
    }
    if provider == "claude" && cwd.is_empty() {
        let encoded = ctx.target.replace(['/', '_'], "-");
        if path.starts_with(&format!("{}/.claude/projects/{encoded}/", ctx.home)) {
            cwd = ctx.target.clone();
        }
    }
    if !matches_path(ctx, &cwd) {
        return None;
    }
    if id.is_empty() {
        id = Path::new(path).file_stem()?.to_string_lossy().into()
    }
    let first = messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| text(m, "fullText"));
    let title = first.map(|s| trunc(s, 80));
    let mtime = if ctx.host.is_none() {
        fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    } else {
        file_mtime(ctx, path)
    };
    Some(Transcript {
        summary: json!({"id":id,"provider":provider,"startedAt":start,"updatedAt":updated,"matchedPath":cwd,"matchScope":"worktree","title":title,"firstUserPrompt":first.map(|s|trunc(s,1200)),"messageCount":messages.len(),"sourceRef":path}),
        messages,
        events,
        mtime,
        parent,
    })
}
fn parse_gemini(ctx: &Context, path: &str) -> Option<Transcript> {
    let v: Value = serde_json::from_str(&content(ctx, path).ok()?).ok()?;
    let mut cwd = ["projectPath", "cwd", "directory"]
        .iter()
        .map(|k| text(&v, k))
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    if cwd.is_empty() {
        let project_id = Path::new(path).parent()?.parent()?.file_name()?.to_str()?;
        let map: Value = content(ctx, &format!("{}/.gemini/projects.json", ctx.home))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let projects = map.get("projects").unwrap_or(&map);
        for (path, entry) in projects.as_object().into_iter().flatten() {
            if entry.as_str() == Some(project_id)
                || ["id", "hash", "projectId", "projectHash"]
                    .iter()
                    .any(|k| entry[*k].as_str() == Some(project_id))
            {
                cwd = path.clone();
                break;
            }
        }
        if cwd.is_empty() {
            let metadata = Path::new(path).parent()?.parent()?.join(".project_root");
            cwd = content(ctx, &metadata.to_string_lossy())
                .unwrap_or_default()
                .trim()
                .into();
        }
    }
    if !matches_path(ctx, &cwd) {
        return None;
    }
    let mut messages = Vec::new();
    for m in v["messages"].as_array().into_iter().flatten() {
        let role = match text(m, "type") {
            "gemini" => "assistant",
            "user" => "user",
            "info" | "warning" | "error" | "system" => "system",
            "assistant" => "assistant",
            _ => text(m, "role"),
        };
        if let Some(msg) = message(
            role,
            m.get("timestamp").unwrap_or(&Value::Null),
            plain(m.get("content").unwrap_or(&m["text"])),
        ) {
            messages.push(msg)
        }
        for thought in m["thoughts"].as_array().into_iter().flatten() {
            if let Some(msg) = message("reasoning", &m["timestamp"], plain(thought)) {
                messages.push(msg)
            }
        }
        for tool in m["toolCalls"].as_array().into_iter().flatten() {
            if let Some(msg) = message(
                "tool",
                &m["timestamp"],
                format!(
                    "{}: {}",
                    text(tool, "name"),
                    tool.get("args").unwrap_or(&Value::Null)
                ),
            ) {
                messages.push(msg)
            }
        }
    }
    let id = v
        .get("sessionId")
        .or_else(|| v.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });
    let first = messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| text(m, "fullText"));
    let title = v
        .get("title")
        .cloned()
        .unwrap_or_else(|| first.map(|s| json!(trunc(s, 80))).unwrap_or(Value::Null));
    let start = iso(v
        .get("startTime")
        .or_else(|| v.get("createdAt"))
        .unwrap_or(&v["timestamp"]));
    let updated = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user" || m["role"] == "assistant")
        .map(|m| m["timestamp"].clone())
        .unwrap_or_else(|| {
            iso(v
                .get("lastUpdated")
                .or_else(|| v.get("updatedAt"))
                .unwrap_or(&v["timestamp"]))
        });
    let mtime = updated
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0);
    Some(Transcript {
        summary: json!({"id":id,"provider":"gemini","startedAt":start,"updatedAt":updated,"matchedPath":cwd,"matchScope":"worktree","title":title,"firstUserPrompt":first.map(|s|trunc(s,1200)),"messageCount":messages.len(),"sourceRef":path}),
        messages,
        events: vec![],
        mtime,
        parent: None,
    })
}
fn opencode_message(meta: &Value, data: &Value, time: &Value) -> Option<Value> {
    let kind = text(data, "type");
    let role = match kind {
        "tool" => "tool",
        "reasoning" => "reasoning",
        _ => text(meta, "role"),
    };
    let body = match kind {
        "text" | "reasoning" => plain(&data["text"]),
        "tool" => {
            let name = data["tool"].as_str().unwrap_or("tool");
            let state = plain(&data["state"]);
            if state.is_empty() {
                format!("{name} executed")
            } else {
                format!("{name}: {state}")
            }
        }
        _ => return None,
    };
    message(role, time, body)
}
fn open_code(ctx: &Context) -> Result<Vec<Transcript>, String> {
    let path = format!("{}/.local/share/opencode/opencode.db", ctx.home);
    if ctx.host.is_some() {
        return open_code_remote(ctx, &path);
    }
    if !Path::new(&path).exists() {
        return Err("OpenCode database not found".into());
    }
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    let mut statement=conn.prepare("SELECT id,directory,title,time_created,time_updated,parent_id FROM session WHERE directory=?1 OR substr(directory,1,length(?2))=?2 ORDER BY time_updated DESC LIMIT 120").or_else(|_|conn.prepare("SELECT id,directory,title,time_created,time_updated,NULL FROM session WHERE directory=?1 OR substr(directory,1,length(?2))=?2 ORDER BY time_updated DESC LIMIT 120")).map_err(|e|e.to_string())?;
    let rows = statement
        .query_map(
            [
                ctx.target.trim_end_matches('/'),
                &format!("{}/", ctx.target.trim_end_matches('/')),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (id, cwd, title, start, updated, parent) = row.map_err(|e| e.to_string())?;
        let mut parts=conn.prepare("SELECT p.data,p.time_created,m.data FROM part p JOIN message m ON m.id=p.message_id WHERE p.session_id=?1 ORDER BY p.time_created ASC").map_err(|e|e.to_string())?;
        let rows = parts
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut messages = Vec::new();
        for row in rows {
            let (data, time, meta) = row.map_err(|e| e.to_string())?;
            let data = serde_json::from_str::<Value>(&data).unwrap_or(Value::Null);
            let meta = serde_json::from_str::<Value>(&meta).unwrap_or(Value::Null);
            if let Some(m) = opencode_message(&meta, &data, &json!(time)) {
                messages.push(m)
            }
        }
        let first = messages
            .iter()
            .find(|m| m["role"] == "user")
            .map(|m| text(m, "fullText"));
        let updated = messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user" || m["role"] == "assistant")
            .map(|m| m["timestamp"].clone())
            .unwrap_or_else(|| iso(&json!(updated)));
        result.push(Transcript{summary:json!({"id":id,"provider":"opencode","startedAt":iso(&json!(start)),"updatedAt":updated,"matchedPath":cwd,"matchScope":"worktree","title":title.or_else(||first.map(|s|trunc(s,80))),"firstUserPrompt":first.map(|s|trunc(s,1200)),"messageCount":messages.len(),"sourceRef":id}),messages,events:vec![],mtime:updated.as_str().and_then(|s|DateTime::parse_from_rfc3339(s).ok()).map(|d|d.timestamp_millis()).unwrap_or(0),parent});
    }
    Ok(result)
}
fn open_code_remote(ctx: &Context, path: &str) -> Result<Vec<Transcript>, String> {
    let script="import sqlite3,json,sys\nc=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);c.row_factory=sqlite3.Row\ns=[dict(r) for r in c.execute('SELECT * FROM session WHERE directory=? OR substr(directory,1,length(?))=? ORDER BY time_updated DESC LIMIT 120',(sys.argv[2].rstrip('/'),sys.argv[2].rstrip('/')+'/',sys.argv[2].rstrip('/')+'/'))]\nfor r in s:r['parts']=[dict(x) for x in c.execute('SELECT p.data,p.time_created,m.data AS message_data FROM part p JOIN message m ON m.id=p.message_id WHERE p.session_id=? ORDER BY p.time_created',(r['id'],))]\nprint(json.dumps(s))";
    let output = command(
        ctx,
        &format!(
            "python3 -c {} {} {}",
            quote(script),
            quote(path),
            quote(&ctx.target)
        ),
    )?;
    let rows: Vec<Value> = serde_json::from_str(&output).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r|{let messages:Vec<Value>=r["parts"].as_array().into_iter().flatten().filter_map(|p|{let data:Value=serde_json::from_str(text(p,"data")).ok()?;let meta:Value=serde_json::from_str(text(p,"message_data")).ok()?;opencode_message(&meta,&data,&p["time_created"])}).collect();let first=messages.iter().find(|m|m["role"]=="user").map(|m|text(m,"fullText"));Transcript{summary:json!({"id":r["id"],"provider":"opencode","startedAt":iso(&r["time_created"]),"updatedAt":iso(&r["time_updated"]),"matchedPath":r["directory"],"matchScope":"worktree","title":r["title"],"firstUserPrompt":first.map(|s|trunc(s,1200)),"messageCount":messages.len(),"sourceRef":r["id"]}),messages,events:vec![],mtime:r["time_updated"].as_i64().unwrap_or(0),parent:r["parent_id"].as_str().map(str::to_owned)}}).collect())
}
fn read_provider(ctx: &Context, provider: &str) -> (Vec<Transcript>, Value) {
    if provider == "opencode" {
        return match open_code(ctx) {
            Ok(t) => {
                let status = json!({"provider":provider,"available":true,"sessionCount":t.len(),"reason":null});
                (t, status)
            }
            Err(e) => (
                vec![],
                json!({"provider":provider,"available":false,"sessionCount":0,"reason":e}),
            ),
        };
    }
    let (sub, suffix) = match provider {
        "claude" => (".claude/projects", ".jsonl"),
        "codex" => (".codex/sessions", ".jsonl"),
        _ => (".gemini/tmp", ".json"),
    };
    let root = format!("{}/{sub}", ctx.home);
    let exists = if ctx.host.is_some() {
        command(ctx, &format!("test -d {} && echo yes", quote(&root))).is_ok()
    } else {
        Path::new(&root).is_dir()
    };
    if !exists {
        return (
            vec![],
            json!({"provider":provider,"available":false,"sessionCount":0,"reason":format!("{provider} directory not found")}),
        );
    }
    let mut paths = files(ctx, &root, suffix);
    if provider == "claude" {
        paths.retain(|p| !p.contains("/subagents/"));
    }
    if ctx.recent {
        let since = SystemTime::now() - std::time::Duration::from_secs(21600);
        if ctx.host.is_none() {
            paths.retain(|p| {
                fs::metadata(p)
                    .and_then(|m| m.modified())
                    .is_ok_and(|mtime| mtime >= since)
            });
        }
    }
    let transcripts: Vec<_> = paths
        .iter()
        .filter(|p| {
            provider != "gemini"
                || Path::new(p)
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|n| n == "chats")
        })
        .filter_map(|path| {
            if provider == "gemini" {
                parse_gemini(ctx, path)
            } else {
                parse_jsonl(ctx, provider, path)
            }
        })
        .collect();
    let status = json!({"provider":provider,"available":true,"sessionCount":transcripts.len(),"reason":if transcripts.is_empty(){json!(format!("No {provider} sessions matched this task"))}else{Value::Null}});
    (transcripts, status)
}
fn paginate(items: Vec<Value>, cursor: &Value, limit: usize) -> (Vec<Value>, Value) {
    let offset = cursor
        .as_str()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let total = items.len();
    let result: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    let next = offset + result.len();
    (
        result,
        if next < total {
            json!(next.to_string())
        } else {
            Value::Null
        },
    )
}
pub fn running_panes(host: Option<&str>) -> Vec<Value> {
    crate::git::run("tmux list-panes -a -F '#{pane_current_command}\t#{pane_current_path}\t#{session_name}\t#{window_id}\t#{window_name}'",host).unwrap_or_default().lines().filter_map(|line|{let fields:Vec<_>=line.split('\t').collect();if fields.len()!=5||!["claude","codex","gemini","opencode"].contains(&fields[0]){return None}Some(json!({"provider":fields[0],"worktreePath":fields[1],"sessionName":fields[2],"windowId":fields[3],"windowName":fields[4]}))}).collect()
}
fn graph_nodes(transcripts: &[Transcript], parent: &str, seen: &mut HashSet<String>) -> Vec<Value> {
    transcripts.iter().filter(|t|t.parent.as_deref()==Some(parent)).filter_map(|t|{let id=text(&t.summary,"id").to_string();if !seen.insert(id.clone()){return None}let ended=if Utc::now().timestamp_millis()-t.mtime<=90000{Value::Null}else{t.summary["updatedAt"].clone()};Some(json!({"id":id,"agentType":t.events.first().and_then(|e|e.pointer("/payload/source/subagent/thread_spawn/agent_type")).cloned().unwrap_or(Value::Null),"skill":null,"task":t.events.first().and_then(|e|e.pointer("/payload/agent_nickname")).cloned().unwrap_or_else(||t.summary["title"].clone()),"startedAt":t.summary["startedAt"],"endedAt":ended,"children":graph_nodes(transcripts,&id,seen)}))}).collect()
}
fn file_mtime(ctx: &Context, path: &str) -> i64 {
    if ctx.host.is_none() {
        fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    } else {
        command(
            ctx,
            &format!(
                "stat -c %Y {path} 2>/dev/null || stat -f %m {path}",
                path = quote(path)
            ),
        )
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|s| s * 1000)
        .unwrap_or(0)
    }
}
fn claude_children(ctx: &Context, t: &Transcript) -> HashMap<String, Transcript> {
    let source = text(&t.summary, "sourceRef");
    let id = text(&t.summary, "id");
    let Some(dir) = Path::new(source).parent() else {
        return HashMap::new();
    };
    let mut all = HashMap::new();
    for file in files(
        ctx,
        &dir.join(id).join("subagents").to_string_lossy(),
        ".jsonl",
    ) {
        let mut full = ctx.clone();
        full.bounded = false;
        if let Some(child) = parse_jsonl(&full, "claude", &file) {
            let id = child
                .events
                .iter()
                .find_map(|e| e["agentId"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    Path::new(&file)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .trim_start_matches("agent-")
                        .into()
                });
            all.insert(id, child);
        }
    }
    all
}
fn claude_graph(ctx: &Context, t: &Transcript, seen: &mut HashSet<String>) -> Vec<Value> {
    claude_graph_index(t, &claude_children(ctx, t), seen)
}
fn claude_graph_index(
    t: &Transcript,
    children: &HashMap<String, Transcript>,
    seen: &mut HashSet<String>,
) -> Vec<Value> {
    let mut result = Vec::new();
    for event in &t.events {
        for part in event
            .pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if text(part, "type") != "tool_use" || !["Task", "Agent"].contains(&text(part, "name"))
            {
                continue;
            }
            let input = &part["input"];
            let call_id = text(part, "id");
            let finished = t.events.iter().find(|event| {
                event
                    .pointer("/message/content")
                    .and_then(Value::as_array)
                    .is_some_and(|parts| parts.iter().any(|p| text(p, "tool_use_id") == call_id))
                    && event.pointer("/toolUseResult/agentId").is_some()
            });
            let id =
                finished
                    .and_then(|e| e.pointer("/toolUseResult/agentId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        children
                            .iter()
                            .find(|(_, child)| {
                                child.events.iter().find_map(|e| {
                                    e.pointer("/message/content").and_then(Value::as_str)
                                }) == Some(text(input, "prompt"))
                            })
                            .map(|(id, _)| id.clone())
                    });
            let Some(id) = id else { continue };
            if !seen.insert(id.clone()) {
                continue;
            }
            let child = children.get(&id);
            let skill =
                child.and_then(|t| t.events.iter().find_map(|e| e["attributionSkill"].as_str()));
            let end = finished.map(|e| iso(&e["timestamp"])).unwrap_or_else(|| {
                child
                    .filter(|t| t.mtime > 0 && Utc::now().timestamp_millis() - t.mtime > 90000)
                    .map(|t| iso(&json!(t.mtime)))
                    .unwrap_or(Value::Null)
            });
            result.push(json!({"id":id,"agentType":input["subagent_type"],"skill":skill,"task":input["description"].as_str().map(|s|trunc(s,40)),"startedAt":iso(&event["timestamp"]),"endedAt":end,"children":child.map(|t|claude_graph_index(t,children,seen)).unwrap_or_default()}));
        }
    }
    result
}
fn live_selection(transcripts: &[Transcript], now: i64) -> Vec<&Transcript> {
    let mut selected = Vec::new();
    for provider in ["claude", "codex", "opencode", "gemini"] {
        let mut own: Vec<_> = transcripts
            .iter()
            .filter(|t| {
                text(&t.summary, "provider") == provider
                    && t.parent.is_none()
                    && now - t.mtime <= 21600000
            })
            .collect();
        own.sort_by_key(|t| std::cmp::Reverse(t.mtime));
        let running: Vec<_> = own
            .iter()
            .copied()
            .filter(|t| now - t.mtime <= 90000)
            .collect();
        if running.is_empty() {
            if let Some(latest) = own.first() {
                selected.push(*latest)
            }
        } else {
            selected.extend(running)
        }
    }
    selected
}
pub fn dispatch(app: &AppHandle, method: &str, args: Vec<Value>) -> Result<Value, String> {
    if method == "getRunningAgentPanes" {
        return Ok(json!(running_panes(None)));
    }
    let task_id = args.first().and_then(Value::as_str).unwrap_or("");
    let db = app.state::<crate::database::Database>();
    let Some(task) = db.task(task_id)? else {
        return Ok(if method == "getTaskAiSessions" {
            json!({"isRemote":false,"targetPath":null,"repoPath":null,"sessions":[],"sources":[],"nextCursor":null})
        } else if method == "getTaskLiveAiSessions" {
            json!({"taskId":task_id,"isRemote":false,"sessions":[]})
        } else {
            Value::Null
        });
    };
    let project = if task["project"].is_object() {
        task["project"].clone()
    } else {
        db.project(text(&task, "projectId"))?.unwrap_or(Value::Null)
    };
    let target = if text(&task, "worktreePath").is_empty() {
        text(&project, "repoPath")
    } else {
        text(&task, "worktreePath")
    };
    let host = project["sshHost"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let home = if host.is_some() {
        crate::git::run("printf '%s' \"$HOME\"", host.as_deref())?
    } else {
        home().to_string_lossy().into()
    };
    let ctx = Context {
        target: target.into(),
        repo: text(&project, "repoPath").into(),
        host,
        home,
        bounded: method == "getTaskLiveAiSessions"
            || (method == "getTaskAiSessions"
                && args
                    .get(1)
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)),
        recent: method == "getTaskLiveAiSessions",
    };
    let providers = if method == "getTaskAiSessionDetail" || method == "getTaskAgentCallGraph" {
        vec![args.get(1).and_then(Value::as_str).unwrap_or("")]
    } else {
        vec!["claude", "codex", "opencode", "gemini"]
    };
    if providers
        .iter()
        .any(|p| !["claude", "codex", "opencode", "gemini"].contains(p))
    {
        return Err("Unknown AI session provider".into());
    }
    let results = std::thread::scope(|scope| {
        let jobs: Vec<_> = providers
            .iter()
            .map(|p| scope.spawn(|| read_provider(&ctx, p)))
            .collect();
        jobs.into_iter()
            .map(|j| j.join().map_err(|_| "Session reader failed".to_string()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    let sources: Vec<_> = results.iter().map(|(_, status)| status.clone()).collect();
    let mut transcripts: Vec<_> = results.into_iter().flat_map(|(t, _)| t).collect();
    transcripts.sort_by(|a, b| text(&b.summary, "updatedAt").cmp(text(&a.summary, "updatedAt")));
    match method {
        "getTaskAiSessions" => {
            let query = args
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let sessions: Vec<_> = transcripts
                .iter()
                .filter(|t| {
                    query.is_empty()
                        || t.summary.to_string().to_lowercase().contains(&query)
                        || t.messages
                            .iter()
                            .any(|m| text(m, "fullText").to_lowercase().contains(&query))
                })
                .map(|t| t.summary.clone())
                .collect();
            let (items, next) = paginate(
                sessions,
                args.get(2).unwrap_or(&Value::Null),
                args.get(3)
                    .and_then(Value::as_u64)
                    .unwrap_or(120)
                    .clamp(1, 10000) as usize,
            );
            Ok(
                json!({"isRemote":ctx.host.is_some(),"targetPath":ctx.target,"repoPath":ctx.repo,"sessions":items,"sources":sources,"nextCursor":next}),
            )
        }
        "getTaskAiSessionDetail" => {
            let id = args.get(2).and_then(Value::as_str).unwrap_or("");
            let Some(t) = transcripts.iter().find(|t| text(&t.summary, "id") == id) else {
                return Ok(Value::Null);
            };
            let query = args
                .get(6)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let roles = args.get(7).and_then(Value::as_array);
            let mut messages: Vec<_> = t
                .messages
                .iter()
                .filter(|m| {
                    (query.is_empty() || text(m, "fullText").to_lowercase().contains(&query))
                        && roles.is_none_or(|r| r.is_empty() || r.contains(&m["role"]))
                })
                .cloned()
                .collect();
            messages.sort_by(|a, b| text(b, "timestamp").cmp(text(a, "timestamp")));
            let (items, next) = paginate(
                messages,
                args.get(4).unwrap_or(&Value::Null),
                args.get(5)
                    .and_then(Value::as_u64)
                    .unwrap_or(20)
                    .clamp(1, 10000) as usize,
            );
            Ok(
                json!({"sessionId":id,"provider":t.summary["provider"],"title":t.summary["title"],"matchedPath":ctx.target,"sourceRef":t.summary["sourceRef"],"messages":items,"nextCursor":next}),
            )
        }
        "getTaskLiveAiSessions" => {
            let panes = running_panes(ctx.host.as_deref());
            let now = Utc::now().timestamp_millis();
            let mut live = Vec::new();
            let mut used_panes = HashSet::new();
            for t in live_selection(&transcripts, now) {
                let provider = text(&t.summary, "provider");
                let pane = panes.iter().find(|p| {
                    text(p, "provider") == provider
                        && (text(p, "worktreePath") == ctx.target
                            || text(p, "worktreePath")
                                .starts_with(&format!("{}/", ctx.target.trim_end_matches('/'))))
                        && !used_panes.contains(text(p, "windowId"))
                });
                if let Some(pane) = pane {
                    used_panes.insert(text(pane, "windowId").to_string());
                }
                let running = now - t.mtime <= 90000 || pane.is_some();
                let subtasks: Vec<Value> = if provider == "claude" {
                    claude_children(&ctx,t).iter().filter(|(_,child)|now-child.mtime<=90000).map(|(id,child)|json!({"id":id,"name":child.messages.first().map(|m|trunc(text(m,"fullText"),40)),"lastActiveAt":iso(&json!(child.mtime))})).collect()
                } else {
                    transcripts.iter().filter(|child|child.parent.as_deref()==Some(text(&t.summary,"id"))&&now-child.mtime<=90000).map(|child|json!({"id":child.summary["id"],"name":child.events.first().and_then(|e|e.pointer("/payload/agent_nickname")).cloned().unwrap_or_else(||child.summary["title"].clone()),"lastActiveAt":iso(&json!(child.mtime))})).collect()
                };
                let activity = t
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m["role"] == "assistant")
                    .or_else(|| t.messages.iter().rev().find(|m| m["role"] == "user"))
                    .map(|m| trunc(text(m, "fullText"), 80));
                live.push(json!({"provider":provider,"sessionId":t.summary["id"],"currentTask":activity,"state":if running{"running"}else{"idle"},"lastActiveAt":iso(&json!(t.mtime)),"runningSubtasks":subtasks,"terminalWindow":pane.map(|p|json!({"sessionName":p["sessionName"],"windowId":p["windowId"],"windowName":p["windowName"]}))}));
            }
            for pane in panes.iter().filter(|p| {
                !used_panes.contains(text(p, "windowId"))
                    && (text(p, "worktreePath") == ctx.target
                        || text(p, "worktreePath")
                            .starts_with(&format!("{}/", ctx.target.trim_end_matches('/'))))
            }) {
                live.push(json!({"provider":pane["provider"],"sessionId":null,"currentTask":null,"state":"running","lastActiveAt":null,"runningSubtasks":[],"terminalWindow":{"sessionName":pane["sessionName"],"windowId":pane["windowId"],"windowName":pane["windowName"]}}));
            }
            Ok(json!({"taskId":task_id,"isRemote":ctx.host.is_some(),"sessions":live}))
        }
        "getTaskAgentCallGraph" => {
            let provider = providers[0];
            let id = args.get(2).and_then(Value::as_str).unwrap_or("");
            let roots = if provider == "claude" {
                transcripts
                    .iter()
                    .find(|t| text(&t.summary, "id") == id)
                    .map(|t| claude_graph(&ctx, t, &mut HashSet::new()))
                    .unwrap_or_default()
            } else {
                graph_nodes(&transcripts, id, &mut HashSet::new())
            };
            let start = roots.iter().filter_map(|r| r["startedAt"].as_str()).min();
            Ok(
                json!({"provider":provider,"sessionId":id,"startedAt":start,"readAt":now(),"roots":roots}),
            )
        }
        _ => Err(format!("Unknown project AI method {method}")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_match_is_worktree_exact() {
        let ctx = Context {
            target: "/repo/work".into(),
            repo: "/repo".into(),
            host: None,
            home: "/home".into(),
            bounded: false,
            recent: false,
        };
        assert!(matches_path(&ctx, "/repo/work/"));
        assert!(!matches_path(&ctx, "/repo/work-other"));
        assert!(!matches_path(&ctx, "/repo"));
    }
    #[test]
    fn pagination_is_offset_compatible() {
        let (items, next) = paginate(vec![json!(1), json!(2), json!(3)], &json!("1"), 1);
        assert_eq!(items, vec![json!(2)]);
        assert_eq!(next, "2");
    }
    #[test]
    fn codex_filters_bootstrap_user_messages() {
        assert!(skip(
            "user",
            "<environment_context>cwd</environment_context>"
        ));
        assert!(!skip("user", "implement this"));
    }
    #[test]
    fn nested_message_text_survives() {
        assert_eq!(
            plain(&json!([{"type":"text","text":"first"},{"type":"text","text":"second"}])),
            "first\nsecond"
        );
    }
}
#[cfg(test)]
mod migration_regressions {
    use super::*;
    fn context(home: &Path) -> Context {
        Context {
            target: "/repo/worktree".into(),
            repo: "/repo".into(),
            host: None,
            home: home.to_string_lossy().into(),
            bounded: false,
            recent: false,
        }
    }
    fn write_lines(path: &Path, events: &[Value]) {
        fs::write(
            path,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    }
    #[test]
    fn claude_roles_preserve_tool_results_and_conversation_activity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({"type":"system","sessionId":"s","cwd":"/repo/worktree","timestamp":"2026-01-01T00:00:00Z","message":{"role":"system","content":"rules"}}),
                json!({"type":"user","sessionId":"s","cwd":"/repo/worktree","timestamp":"2026-01-01T00:01:00Z","message":{"role":"user","content":[{"type":"text","text":"fix the bug"}]}}),
                json!({"type":"assistant","sessionId":"s","cwd":"/repo/worktree","timestamp":"2026-01-01T00:02:00Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}),
                json!({"type":"user","sessionId":"s","cwd":"/repo/worktree","timestamp":"2026-01-01T00:03:00Z","message":{"role":"user","content":[{"type":"tool_result","content":"file"}]}}),
            ],
        );
        let t = parse_jsonl(&context(dir.path()), "claude", path.to_str().unwrap()).unwrap();
        assert_eq!(t.summary["updatedAt"], "2026-01-01T00:02:00.000Z");
        assert_eq!(
            t.messages
                .iter()
                .map(|m| text(m, "role"))
                .collect::<Vec<_>>(),
            vec!["system", "user", "assistant", "tool"]
        );
    }
    #[test]
    fn codex_reasoning_text_and_bootstrap_dedup_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"s","cwd":"/repo/worktree"}}),
                json!({"type":"response_item","timestamp":"2026-01-01T00:01:00Z","payload":{"type":"message","role":"user","content":[{"text":"<environment_context>hidden"}]}}),
                json!({"type":"response_item","timestamp":"2026-01-01T00:02:00Z","payload":{"type":"message","role":"user","content":[{"text":"implement history"}]}}),
                json!({"type":"response_item","timestamp":"2026-01-01T00:03:00Z","payload":{"type":"reasoning","text":"inspect schema"}}),
            ],
        );
        let t = parse_jsonl(&context(dir.path()), "codex", path.to_str().unwrap()).unwrap();
        assert_eq!(t.messages.len(), 2);
        assert_eq!(t.messages[1]["role"], "reasoning");
        assert_eq!(t.messages[1]["fullText"], "inspect schema");
    }
    #[test]
    fn gemini_project_root_and_thought_description_survive() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join(".gemini/tmp/hash/chats");
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            chats.parent().unwrap().join(".project_root"),
            "/repo/worktree",
        )
        .unwrap();
        let path = chats.join("session.json");
        fs::write(&path,json!({"sessionId":"g","messages":[{"type":"user","timestamp":"2026-01-01T00:01:00Z","content":"plan"},{"type":"gemini","timestamp":"2026-01-01T00:02:00Z","content":"answer","thoughts":[{"subject":"analysis","description":"compare schemas"}],"toolCalls":[{"name":"read_file","args":{"path":"x"}}]},{"type":"info","timestamp":"2026-01-01T00:03:00Z","content":"checkpoint"}]}).to_string()).unwrap();
        let t = parse_gemini(&context(dir.path()), path.to_str().unwrap()).unwrap();
        assert_eq!(t.messages.len(), 5);
        assert_eq!(t.messages.last().unwrap()["role"], "system");
        assert!(t
            .messages
            .iter()
            .any(|m| m["role"] == "reasoning" && text(m, "fullText").contains("compare schemas")));
        assert_eq!(t.summary["updatedAt"], "2026-01-01T00:02:00.000Z");
    }
}
#[cfg(test)]
mod graph_regressions {
    use super::*;
    #[test]
    fn claude_graph_links_completed_and_running_descendants_with_skills() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join(".claude/projects/-repo-worktree");
        let agents = project.join("session/subagents");
        fs::create_dir_all(&agents).unwrap();
        let call = |id: &str, prompt: &str, description: &str| json!({"type":"assistant","timestamp":"2026-08-15T00:00:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":id,"name":"Agent","input":{"subagent_type":"Explore","description":description,"prompt":prompt}}]}});
        let root = project.join("session.jsonl");
        fs::write(&root,format!("{}\n{}",call("call1","review","review task"),json!({"type":"user","timestamp":"2026-08-15T00:07:00Z","toolUseResult":{"agentId":"a1"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call1"}]}}))).unwrap();
        fs::write(agents.join("agent-a1.jsonl"),format!("{}\n{}",json!({"agentId":"a1","attributionSkill":"context-loader","message":{"role":"user","content":"review"}}),call("call2","inspect","inspect task"))).unwrap();
        fs::write(
            agents.join("agent-a2.jsonl"),
            json!({"agentId":"a2","message":{"role":"user","content":"inspect"}}).to_string(),
        )
        .unwrap();
        let ctx = Context {
            target: "/repo/worktree".into(),
            repo: "/repo".into(),
            home: dir.path().to_string_lossy().into(),
            host: None,
            bounded: false,
            recent: false,
        };
        let transcript = parse_jsonl(&ctx, "claude", root.to_str().unwrap()).unwrap();
        let nodes = claude_graph(&ctx, &transcript, &mut HashSet::new());
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["id"], "a1");
        assert_eq!(nodes[0]["skill"], "context-loader");
        assert_eq!(nodes[0]["endedAt"], "2026-08-15T00:07:00.000Z");
        assert_eq!(nodes[0]["children"][0]["id"], "a2");
        assert!(nodes[0]["children"][0]["endedAt"].is_null());
    }
    #[test]
    fn idle_selection_keeps_one_latest_per_provider_and_omits_children() {
        let transcript = |id: &str, time: i64, parent: Option<&str>| Transcript {
            summary: json!({"provider":"codex","id":id}),
            messages: vec![],
            events: vec![],
            mtime: time,
            parent: parent.map(str::to_owned),
        };
        let ts = vec![
            transcript("old", 1000, None),
            transcript("latest", 2000, None),
            transcript("child", 3000, Some("latest")),
        ];
        let selected = live_selection(&ts, 200000);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].summary["id"], "latest");
    }
}

#[cfg(test)]
mod descendant_regressions {
    use super::*;
    fn context(home: &Path) -> Context {
        Context {
            target: "/repo/worktree".into(),
            repo: "/repo".into(),
            host: None,
            home: home.to_string_lossy().into(),
            bounded: false,
            recent: false,
        }
    }
    #[test]
    fn path_matching_accepts_descendants_and_normalizes_components() {
        let ctx = context(Path::new("/home"));
        assert!(matches_path(&ctx, "/repo/worktree/packages/api"));
        assert!(matches_path(&ctx, "/repo/./worktree/packages/../api/"));
        assert!(!matches_path(&ctx, "/repo/worktree-other/api"));
        assert!(!matches_path(&ctx, "/repo/worktree/../other"));
        assert!(!matches_path(&ctx, ""));
    }
    #[test]
    fn claude_discovery_keeps_nested_project_with_direct_project_present() {
        let dir = tempfile::tempdir().unwrap();
        for (encoded, cwd, id) in [
            ("-repo-worktree", "/repo/worktree", "root"),
            (
                "-repo-worktree-packages-api",
                "/repo/worktree/packages/api",
                "nested",
            ),
            ("-repo-worktree-other", "/repo/worktree-other", "other"),
        ] {
            let project = dir.path().join(".claude/projects").join(encoded);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join(format!("{id}.jsonl")), json!({"type":"user", "sessionId":id, "cwd":cwd, "message":{"role":"user", "content":"implement"}}).to_string()).unwrap();
        }
        let (sessions, _) = read_provider(&context(dir.path()), "claude");
        let ids: HashSet<_> = sessions.iter().map(|t| text(&t.summary, "id")).collect();
        assert_eq!(ids, HashSet::from(["root", "nested"]));
    }
    #[test]
    fn opencode_reads_descendants_without_matching_neighbor_repository() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(".local/share/opencode/opencode.db");
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = rusqlite::Connection::open(db).unwrap();
        conn.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER,parent_id TEXT); CREATE TABLE part(data TEXT,time_created INTEGER,session_id TEXT,message_id TEXT); CREATE TABLE message(id TEXT,data TEXT);").unwrap();
        for (id, cwd) in [
            ("root", "/repo/worktree"),
            ("nested", "/repo/worktree/packages/api"),
            ("other", "/repo/worktree-other"),
        ] {
            conn.execute(
                "INSERT INTO session VALUES (?1,?2,NULL,0,0,NULL)",
                [id, cwd],
            )
            .unwrap();
        }
        let sessions = open_code(&context(dir.path())).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|t| t.summary["id"] == "nested"));
    }
}
