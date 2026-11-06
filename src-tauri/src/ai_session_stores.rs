use super::{
    command, content, files, head_lines, iso, lines, matches_path, message, normalize_path, plain,
    quote, text, trunc, Context, Transcript, PREVIEW_TEXT_LENGTH,
};
use base64::Engine;
use chrono::DateTime;
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const HEADER_SCOPED_PROVIDERS: [&str; 9] = [
    "qoder",
    "codebuddy",
    "pi",
    "omp",
    "prime-agent",
    "openclaw",
    "copilot",
    "droid",
    "muse",
];
const ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS: i64 = 2_000;
const MUSE_REPEATED_USER_WINDOW_MS: i64 = 60_000;
const MESSAGE_ROLES: [&str; 4] = ["user", "assistant", "tool", "system"];

struct FileSource {
    provider: &'static str,
    roots: Vec<String>,
    suffix: &'static str,
    accepts: fn(&str) -> bool,
}

#[derive(Default)]
struct DecodedSession {
    id: String,
    cwd: Option<String>,
    title: Option<String>,
    started: Option<String>,
    updated: Option<String>,
    messages: Vec<Value>,
}

impl DecodedSession {
    fn touch(&mut self, time: &Value) {
        let Some(time) = iso(time).as_str().map(str::to_owned) else {
            return;
        };
        if self.started.as_deref().is_none_or(|s| time.as_str() < s) {
            self.started = Some(time.clone());
        }
        if self.updated.as_deref().is_none_or(|s| time.as_str() > s) {
            self.updated = Some(time);
        }
    }
    fn add(&mut self, role: &str, content: &Value, time: &Value) {
        self.add_text(role, plain(content), time);
    }
    fn add_text(&mut self, role: &str, body: String, time: &Value) {
        self.touch(time);
        if let Some(m) = message(role, time, body) {
            self.messages.push(m);
        }
    }
    fn add_message(&mut self, value: &Value) {
        let role = text(value, "role");
        if MESSAGE_ROLES.contains(&role) {
            self.add(
                role,
                &value["content"],
                or(&value["timestamp"], &value["ts"]),
            );
        }
    }
    fn fill_cwd(&mut self, cwd: Option<String>) {
        if self.cwd.is_none() {
            self.cwd = cwd;
        }
    }
}

fn string(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn first_string(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| string(&v[*k]))
}
fn or<'a>(value: &'a Value, fallback: &'a Value) -> &'a Value {
    if value.is_null() {
        fallback
    } else {
        value
    }
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn epoch_millis(v: &Value) -> Option<i64> {
    iso(v)
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
}
fn join(base: &str, parts: &[&str]) -> String {
    let mut path = PathBuf::from(base);
    for part in parts {
        path.push(part);
    }
    path.to_string_lossy().into()
}
fn base_name(file: &str) -> &str {
    Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
}
fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into())
        .unwrap_or_default()
}
fn parent(file: &str) -> String {
    Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().into())
        .unwrap_or_default()
}
fn has_component(file: &str, names: &[&str]) -> bool {
    file.split('/').any(|part| names.contains(&part))
}
fn is_within(file: &str, root: &str) -> bool {
    let file = normalize_path(file);
    let root = normalize_path(root);
    file != root && file.starts_with(&root)
}
fn real_within(file: &str, root: &str) -> bool {
    match (fs::canonicalize(file), fs::canonicalize(root)) {
        (Ok(file), Ok(root)) => file != root && file.starts_with(root),
        _ => false,
    }
}
fn path_exists(ctx: &Context, path: &str) -> bool {
    if ctx.host.is_some() {
        command(ctx, &format!("test -e {} && echo yes", quote(path))).is_ok()
    } else {
        Path::new(path).exists()
    }
}
fn directory_files(ctx: &Context, directory: &str, suffix: &str) -> Vec<String> {
    if ctx.host.is_some() {
        return command(
            ctx,
            &format!(
                "find {} -maxdepth 1 -type f -name {} 2>/dev/null",
                quote(directory),
                quote(&format!("*{suffix}"))
            ),
        )
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    }
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path().to_string_lossy().to_string())
        .filter(|p| p.ends_with(suffix))
        .collect()
}
fn status(provider: &str, available: bool, count: usize, reason: Value) -> Value {
    json!({"provider":provider,"available":available,"sessionCount":count,"reason":reason})
}
fn summary_transcript(summary: Value, messages: Vec<Value>) -> Transcript {
    let mtime = epoch_millis(&summary["updatedAt"]).unwrap_or(0);
    Transcript {
        summary,
        messages,
        events: vec![],
        mtime,
        parent: None,
        detail_title: None,
        is_log_ordered: true,
    }
}
fn first_user_preview(messages: &[Value]) -> Option<Value> {
    messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| m["text"].clone())
}
fn first_user_text(messages: &[Value]) -> Option<String> {
    messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| text(m, "fullText").to_owned())
}

/// 로컬 설치에서만 Agent별 저장 위치 환경 변수를 따르고, 원격은 기본 위치만 본다.
fn configured(remote: bool, name: &str, fallback: String) -> String {
    if remote {
        return fallback;
    }
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && Path::new(v).is_absolute())
        .unwrap_or(fallback)
}
fn agent_sessions(directory: String) -> String {
    match base_name(&directory) {
        ".pi" | ".omp" => join(&directory, &["agent", "sessions"]),
        "agent" => join(&directory, &["sessions"]),
        _ => directory,
    }
}
fn accept_any(_: &str) -> bool {
    true
}
fn no_subagents(file: &str) -> bool {
    !has_component(file, &["subagents"])
}
fn graph_file(file: &str) -> bool {
    !has_component(file, &["tasks", "artifacts", "subagents"])
}
fn session_prefixed(file: &str) -> bool {
    base_name(file).starts_with("session_")
}
fn is_uuid_json(file: &str) -> bool {
    let Some(id) = base_name(file).strip_suffix(".json") else {
        return false;
    };
    let groups: Vec<_> = id.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

fn file_sources(home: &str, remote: bool) -> Vec<FileSource> {
    let root = |parts: &[&str]| join(home, parts);
    let env = |name: &str, fallback: String| configured(remote, name, fallback);
    let pi_root = agent_sessions(env(
        "PI_CODING_AGENT_DIR",
        root(&[".pi", "agent", "sessions"]),
    ));
    let prime_agent = env(
        "PRIME_AGENT_SESSION_DIR",
        env(
            "PRIME_AGENT_CODING_AGENT_SESSION_DIR",
            join(
                &env("PRIME_AGENT_CODING_AGENT_DIR", root(&[".prime", "agent"])),
                &["sessions"],
            ),
        ),
    );
    let devin = env("DEVIN_HOME", root(&[".local", "share", "devin", "cli"]));
    let source = |provider, roots, suffix, accepts| FileSource {
        provider,
        roots,
        suffix,
        accepts,
    };
    vec![
        source(
            "qoder",
            vec![root(&[".qoder", "projects"])],
            ".jsonl",
            no_subagents as fn(&str) -> bool,
        ),
        source(
            "codebuddy",
            vec![root(&[".codebuddy", "projects"])],
            ".jsonl",
            no_subagents,
        ),
        source(
            "hermes",
            vec![join(&env("HERMES_HOME", root(&[".hermes"])), &["sessions"])],
            ".json",
            session_prefixed,
        ),
        source("pi", vec![pi_root.clone()], ".jsonl", graph_file),
        source(
            "omp",
            vec![
                agent_sessions(env(
                    "OMP_CODING_AGENT_DIR",
                    root(&[".omp", "agent", "sessions"]),
                )),
                pi_root,
            ],
            ".jsonl",
            graph_file,
        ),
        source("prime-agent", vec![prime_agent], ".jsonl", accept_any),
        source(
            "cursor",
            vec![root(&[".cursor", "projects"])],
            ".jsonl",
            |f| has_component(f, &["agent-transcripts"]),
        ),
        source(
            "antigravity",
            vec![root(&[".gemini", "antigravity-cli", "brain"])],
            ".jsonl",
            |f| {
                f.ends_with("/.system_generated/logs/transcript.jsonl")
                    || f.ends_with("/.system_generated/logs/transcript_full.jsonl")
            },
        ),
        source(
            "rovo",
            vec![root(&[".rovodev", "sessions"])],
            ".json",
            |f| base_name(f) == "metadata.json",
        ),
        source(
            "copilot",
            vec![join(
                &env("COPILOT_HOME", root(&[".copilot"])),
                &["session-state"],
            )],
            ".jsonl",
            accept_any,
        ),
        source(
            "grok",
            vec![join(&env("GROK_HOME", root(&[".grok"])), &["sessions"])],
            ".json",
            |f| base_name(f) == "summary.json",
        ),
        source(
            "openclaw",
            vec![
                join(
                    &env("OPENCLAW_STATE_DIR", root(&[".openclaw"])),
                    &["agents"],
                ),
                root(&[".clawdbot", "agents"]),
            ],
            ".jsonl",
            |f| has_component(f, &["sessions"]),
        ),
        source(
            "devin",
            vec![
                join(&devin, &["transcripts"]),
                join(&devin, &["agent_logs"]),
            ],
            ".json",
            accept_any,
        ),
        source(
            "droid",
            vec![
                root(&[".factory", "sessions"]),
                root(&[".factory", "projects"]),
            ],
            ".jsonl",
            accept_any,
        ),
        source(
            "cline",
            vec![env(
                "CLINE_SESSION_DATA_DIR",
                root(&[".cline", "data", "sessions"]),
            )],
            ".json",
            |f| base_name(f) == format!("{}.json", base_name(&parent(f))),
        ),
        source(
            "kimi",
            vec![join(
                &env("KIMI_CODE_HOME", root(&[".kimi-code"])),
                &["sessions"],
            )],
            ".json",
            |f| base_name(f) == "state.json" && base_name(&parent(f)).starts_with("session_"),
        ),
        source(
            "muse",
            vec![join(
                &env("XDG_DATA_HOME", root(&[".local", "share"])),
                &["muse", "sessions"],
            )],
            ".jsonl",
            |f| base_name(f) == "session.jsonl",
        ),
        source(
            "jcode",
            vec![join(&env("JCODE_HOME", root(&[".jcode"])), &["sessions"])],
            ".json",
            session_prefixed,
        ),
        source(
            "kiro",
            vec![join(
                &env("KIRO_HOME", root(&[".kiro"])),
                &["sessions", "cli"],
            )],
            ".json",
            is_uuid_json,
        ),
    ]
}

/// 파일 기반 Agent 저장소 하나를 읽는다. 다른 프로바이더면 None을 돌려준다.
pub(super) fn read_file_provider(
    ctx: &Context,
    provider: &str,
) -> Option<(Vec<Transcript>, Value)> {
    let source = file_sources(&ctx.home, ctx.host.is_some())
        .into_iter()
        .find(|s| s.provider == provider)?;
    if !source.roots.iter().any(|root| path_exists(ctx, root)) {
        let reason = json!("Session directory not found");
        return Some((vec![], status(provider, false, 0, reason)));
    }
    let mut scope = Scope::new(ctx);
    let mut broken = 0;
    let mut sessions = Vec::new();
    for file in discover(ctx, &source) {
        if !allowed_source(ctx, &file, &source) {
            continue;
        }
        // 손상된 파일 하나가 같은 Agent의 다른 기록까지 숨기지 않게 한다.
        match read_session(&mut scope, &source, &file) {
            Ok(Some(t)) => sessions.push(t),
            Ok(None) => {}
            Err(_) => broken += 1,
        }
    }
    let reason = if broken > 0 {
        json!(format!("{broken} session files could not be read"))
    } else {
        Value::Null
    };
    let status = status(provider, true, sessions.len(), reason);
    Some((sessions, status))
}

fn discover(ctx: &Context, source: &FileSource) -> Vec<String> {
    let mut seen = HashSet::new();
    let found: Vec<String> = source
        .roots
        .iter()
        .flat_map(|root| files(ctx, root, source.suffix))
        .filter(|f| seen.insert(f.clone()) && (source.accepts)(f))
        .collect();
    if source.provider != "antigravity" {
        return found;
    }
    // full transcript가 있으면 짧은 preview 파일을 중복 세션으로 올리지 않는다.
    found
        .iter()
        .filter(|f| {
            !f.ends_with("/transcript.jsonl")
                || !seen.contains(&f.replace("transcript.jsonl", "transcript_full.jsonl"))
        })
        .cloned()
        .collect()
}

fn allowed_source(ctx: &Context, file: &str, source: &FileSource) -> bool {
    file.ends_with(source.suffix)
        && (source.accepts)(file)
        && source
            .roots
            .iter()
            .any(|root| is_within(file, root) && (ctx.host.is_some() || real_within(file, root)))
}

struct Scope<'a> {
    ctx: &'a Context,
    optional_files: HashMap<String, Value>,
    devin_indexes: HashMap<String, HashMap<String, Value>>,
    cursor_metadata_files: Option<Vec<String>>,
}

impl<'a> Scope<'a> {
    fn new(ctx: &'a Context) -> Self {
        Scope {
            ctx,
            optional_files: HashMap::new(),
            devin_indexes: HashMap::new(),
            cursor_metadata_files: None,
        }
    }
    /// 빈 파일은 Null, JSONL은 읽을 수 있는 줄의 배열로 돌려준다.
    fn read_parsed(&self, file: &str) -> Result<Value, String> {
        if self.ctx.host.is_none()
            && fs::symlink_metadata(file)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Session file cannot be a symbolic link".into());
        }
        if file.ends_with(".jsonl") {
            let records = lines(self.ctx, file);
            return Ok(if records.is_empty() {
                Value::Null
            } else {
                Value::Array(records)
            });
        }
        let raw = content(self.ctx, file)?;
        if raw.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    }
    fn optional(&mut self, file: &str) -> Result<Value, String> {
        if let Some(cached) = self.optional_files.get(file) {
            return Ok(cached.clone());
        }
        let value = if path_exists(self.ctx, file) {
            self.read_parsed(file)?
        } else {
            Value::Null
        };
        self.optional_files.insert(file.into(), value.clone());
        Ok(value)
    }
    fn cursor_metadata(&mut self, transcript: &str) -> Result<Value, String> {
        let directory = parent(transcript);
        let id = if base_name(&directory) == "agent-transcripts" {
            stem(transcript)
        } else {
            base_name(&directory).to_owned()
        };
        if self.cursor_metadata_files.is_none() {
            let chats = join(&self.ctx.home, &[".cursor", "chats"]);
            self.cursor_metadata_files = Some(files(self.ctx, &chats, ".json"));
        }
        let matches: Vec<String> = self
            .cursor_metadata_files
            .iter()
            .flatten()
            .filter(|f| base_name(f) == "meta.json" && base_name(&parent(f)) == id)
            .cloned()
            .collect();
        if matches.len() != 1 {
            return Ok(json!({}));
        }
        self.optional(&matches[0])
    }
    fn devin_entry(&mut self, file: &str, raw: &Value) -> Result<Value, String> {
        let db = join(&parent(&parent(file)), &["sessions.db"]);
        if !self.devin_indexes.contains_key(&db) {
            let index = read_devin_index(self.ctx, &db)?;
            self.devin_indexes.insert(db.clone(), index);
        }
        let id = string(&raw["session_id"])
            .or_else(|| string(&raw["sessionId"]))
            .unwrap_or_else(|| stem(file));
        Ok(self.devin_indexes[&db]
            .get(&id)
            .cloned()
            .unwrap_or_else(|| json!({})))
    }
    fn kimi_sidecar(&mut self, file: &str, raw: &Value) -> Result<Value, String> {
        let directory = parent(file);
        let sessions_root = parent(&parent(&directory));
        let index = self.optional(&join(&parent(&sessions_root), &["session_index.jsonl"]))?;
        let session_id = base_name(&directory);
        let entry = index
            .as_array()
            .and_then(|records| records.iter().rev().find(|r| r["sessionId"] == session_id));
        let primary = raw["agents"]
            .as_object()
            .and_then(|agents| {
                agents
                    .iter()
                    .find(|(_, a)| a["type"] == "main" && a["parentAgentId"].is_null())
                    .map(|(id, _)| id.clone())
            })
            .unwrap_or_else(|| "main".into());
        // state.json의 agent ID가 경로 밖을 지시하지 못하게 한다.
        if primary.is_empty()
            || !primary
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Ok(json!({}));
        }
        let wire = join(&directory, &["agents", &primary, "wire.jsonl"]);
        Ok(json!({"workDir":entry.map(|e|e["workDir"].clone()),"lines":self.optional(&wire)?}))
    }
    fn sidecar(&mut self, provider: &str, file: &str, raw: &Value) -> Result<Value, String> {
        let directory = parent(file);
        Ok(match provider {
            "grok" => json!({"lines":self.optional(&join(&directory, &["chat_history.jsonl"]))?}),
            "rovo" => self.optional(&join(&directory, &["session_context.json"]))?,
            "cline" => {
                let messages = format!("{}.messages.json", file.trim_end_matches(".json"));
                self.optional(&messages)?
            }
            "kiro" => json!({"lines":self.optional(&format!("{file}l"))?}),
            "cursor" => self.cursor_metadata(file)?,
            "antigravity" => self.antigravity_workspace(file, raw)?,
            "devin" => self.devin_entry(file, raw)?,
            "kimi" => self.kimi_sidecar(file, raw)?,
            _ => json!({}),
        })
    }
    fn antigravity_workspace(&mut self, file: &str, raw: &Value) -> Result<Value, String> {
        let root = join(&self.ctx.home, &[".gemini", "antigravity-cli"]);
        let id = antigravity_id(file);
        let history = self.optional(&join(&root, &["history.jsonl"]))?;
        let records = history.as_array().cloned().unwrap_or_default();
        let mut paths: HashSet<String> = records
            .iter()
            .filter(|r| r["conversationId"] == id.as_str())
            .filter_map(|r| string(&r["workspace"]))
            .collect();
        let metadata = self.optional(&join(&root, &["cache", "conversation_metadata.json"]))?;
        let projects = self.optional(&join(&root, &["cache", "projects.json"]))?;
        let last = self.optional(&join(&root, &["cache", "last_conversations.json"]))?;
        for (workspace, conversation) in last.as_object().into_iter().flatten() {
            if *conversation == id.as_str() {
                paths.insert(workspace.clone());
            }
        }
        let target = string(&metadata["conversations"][id.as_str()]["summary"]["ProjectID"]);
        if let Some(target) = target {
            for (key, value) in projects.as_object().into_iter().flatten() {
                if *key == target {
                    if let Some(v) = value.as_str().filter(|v| Path::new(v).is_absolute()) {
                        paths.insert(v.into());
                    }
                }
                if *value == target.as_str() && Path::new(key).is_absolute() {
                    paths.insert(key.clone());
                }
            }
        }
        if paths.is_empty() {
            paths.extend(antigravity_prompt_workspaces(file, raw, &records));
        }
        // 동일 ID가 서로 다른 작업에 연결되면 임의의 cwd를 택하지 않는다.
        Ok(if paths.len() == 1 {
            json!({"workspace":paths.into_iter().next()})
        } else {
            json!({})
        })
    }
}

fn antigravity_id(file: &str) -> String {
    base_name(&parent(&parent(&parent(file)))).to_owned()
}

/// 메타데이터가 없으면 첫 사용자 입력과 history.jsonl 기록의 시각·본문이 맞는 작업 경로를 쓴다.
fn antigravity_prompt_workspaces(file: &str, raw: &Value, records: &[Value]) -> Vec<String> {
    let transcript = decode("antigravity", file, raw, &Value::Null);
    let Some(first) = transcript.messages.iter().find(|m| m["role"] == "user") else {
        return vec![];
    };
    let Some(first_time) = epoch_millis(&first["timestamp"]) else {
        return vec![];
    };
    records
        .iter()
        .filter(|r| {
            r["display"] == first["fullText"]
                && epoch_millis(&r["timestamp"])
                    .is_some_and(|t| (t - first_time).abs() <= ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS)
        })
        .filter_map(|r| string(&r["workspace"]))
        .collect()
}

fn read_session(
    scope: &mut Scope,
    source: &FileSource,
    file: &str,
) -> Result<Option<Transcript>, String> {
    let ctx = scope.ctx;
    let provider = source.provider;
    let outside = |cwd: Option<String>| cwd.is_some_and(|c| !matches_path(ctx, &c));
    // 다른 작업 기록은 첫 줄의 메타데이터만으로 걸러 본문을 읽지 않는다.
    if HEADER_SCOPED_PROVIDERS.contains(&provider) {
        let head = Value::Array(head_lines(ctx, file, 1));
        if outside(decode(provider, file, &head, &Value::Null).cwd) {
            return Ok(None);
        }
    }
    if provider == "cursor" && outside(string(&scope.cursor_metadata(file)?["cwd"])) {
        return Ok(None);
    }
    let raw = scope.read_parsed(file)?;
    if raw.is_null() {
        return Ok(None);
    }
    if (provider == "pi" || provider == "omp")
        && !matches_graph_writer(provider, file, &raw, &ctx.home, ctx.host.is_some())
    {
        return Ok(None);
    }
    let sidecar = scope.sidecar(provider, file, &raw)?;
    if provider == "devin" && truthy(&sidecar["hidden"]) {
        return Ok(None);
    }
    let session = decode(provider, file, &raw, &sidecar);
    if !session.cwd.as_deref().is_some_and(|c| matches_path(ctx, c)) {
        return Ok(None);
    }
    Ok(Some(file_transcript(session, provider, file)))
}

/// Pi 디렉터리에는 OMP가 쓴 기록도 섞이므로 작성 도구의 흔적으로 한쪽에만 올린다.
fn matches_graph_writer(provider: &str, file: &str, raw: &Value, home: &str, remote: bool) -> bool {
    let sources = file_sources(home, remote);
    let first_root = |name: &str| {
        sources
            .iter()
            .find(|s| s.provider == name)
            .map(|s| s.roots[0].clone())
            .unwrap_or_default()
    };
    let (omp_root, pi_root) = (first_root("omp"), first_root("pi"));
    let in_omp_root = is_within(file, &omp_root) && omp_root != pi_root;
    let omp_records = raw.as_array().is_some_and(|records| {
        records.iter().any(|r| {
            r["runtime"].is_object()
                || (r["type"] == "title" && r["v"] == 1)
                || (r["type"] == "model_change"
                    && r.get("modelId").is_none()
                    && r["model"].is_string())
        })
    });
    let omp_writer = in_omp_root || omp_records;
    if provider == "omp" {
        omp_writer
    } else {
        !omp_writer
    }
}

fn file_transcript(session: DecodedSession, provider: &str, file: &str) -> Transcript {
    let first = first_user_text(&session.messages);
    let title = session
        .title
        .as_deref()
        .or(first.as_deref())
        .map(|s| trunc(s, 80));
    // 상세 제목은 자르지 않은 원래 제목, 없으면 첫 사용자 메시지의 240자 미리보기다.
    let detail_title = session
        .title
        .clone()
        .map(Value::from)
        .or_else(|| first_user_preview(&session.messages))
        .unwrap_or(Value::Null);
    let summary = json!({"id":session.id,"provider":provider,"matchedPath":session.cwd,"matchScope":"worktree","startedAt":session.started,"updatedAt":session.updated,"title":title,"firstUserPrompt":first.map(|s|trunc(&s,PREVIEW_TEXT_LENGTH)),"messageCount":session.messages.len(),"sourceRef":file});
    Transcript {
        detail_title: Some(detail_title),
        ..summary_transcript(summary, session.messages)
    }
}

fn decode(provider: &str, file: &str, raw: &Value, sidecar: &Value) -> DecodedSession {
    let records: Vec<&Value> = match raw {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    let mut s = DecodedSession {
        id: stem(file),
        ..Default::default()
    };
    match provider {
        "qoder" => decode_qoder(&mut s, &records),
        "codebuddy" => decode_codebuddy(&mut s, &records),
        "pi" | "omp" | "prime-agent" | "openclaw" => decode_pi(&mut s, &records),
        "copilot" => decode_copilot(&mut s, &records),
        "cursor" => decode_cursor(&mut s, file, &records, sidecar),
        "droid" => decode_droid(&mut s, &records),
        "hermes" | "jcode" => decode_hermes(&mut s, provider, raw),
        "rovo" => decode_rovo(&mut s, file, raw, sidecar),
        "grok" => decode_grok(&mut s, file, raw, sidecar),
        "devin" => decode_devin(&mut s, raw, sidecar),
        "cline" => decode_cline(&mut s, file, raw, sidecar),
        "kiro" => decode_kiro(&mut s, raw, sidecar),
        "kimi" => decode_kimi(&mut s, file, raw, sidecar),
        "antigravity" => decode_antigravity(&mut s, file, &records, sidecar),
        "muse" => decode_muse(&mut s, file, &records),
        _ => {}
    }
    s
}

fn decode_qoder(s: &mut DecodedSession, records: &[&Value]) {
    for r in records {
        s.id = string(&r["sessionId"]).unwrap_or(std::mem::take(&mut s.id));
        s.fill_cwd(string(&r["cwd"]));
        match text(r, "type") {
            "workspace-directories" => s.fill_cwd(string(&r["directories"][0])),
            "summary" => s.title = string(&r["summary"]).or(s.title.take()),
            kind @ ("user" | "assistant") => s.add(kind, &r["message"]["content"], &r["timestamp"]),
            _ => {}
        }
    }
}

fn decode_codebuddy(s: &mut DecodedSession, records: &[&Value]) {
    for r in records {
        s.id = string(&r["sessionId"]).unwrap_or(std::mem::take(&mut s.id));
        s.fill_cwd(string(&r["cwd"]));
        match text(r, "type") {
            "summary" => s.title = string(&r["summary"]).or(s.title.take()),
            "ai-title" => s.title = string(&r["aiTitle"]).or(s.title.take()),
            "message" => {
                let role = text(r, "role");
                // skipRun은 실제로 보내지 않은 주입 문맥이므로 History 대화에서 제외한다.
                if !["user", "assistant"].contains(&role)
                    || (role == "user" && r["providerData"]["skipRun"] == true)
                {
                    continue;
                }
                let content = match r["content"].as_array() {
                    Some(parts) => Value::Array(
                        parts
                            .iter()
                            .filter(|p| {
                                ["text", "input_text", "output_text"].contains(&text(p, "type"))
                            })
                            .cloned()
                            .collect(),
                    ),
                    None => r["content"].clone(),
                };
                s.add(role, &content, &r["timestamp"]);
            }
            _ => {}
        }
    }
}

fn decode_pi(s: &mut DecodedSession, records: &[&Value]) {
    for r in records {
        match text(r, "type") {
            "session" => {
                s.id = string(&r["id"]).unwrap_or(std::mem::take(&mut s.id));
                s.fill_cwd(string(&r["cwd"]));
                s.touch(&r["timestamp"]);
            }
            "title" => s.title = string(&r["title"]).or(s.title.take()),
            "message" => {
                let m = &r["message"];
                match text(m, "role") {
                    "toolResult" => s.add("tool", &m["content"], &r["timestamp"]),
                    role @ ("user" | "assistant") => s.add(role, &m["content"], &r["timestamp"]),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn decode_copilot(s: &mut DecodedSession, records: &[&Value]) {
    for r in records {
        let data = &r["data"];
        match text(r, "type") {
            "session.start" => {
                s.id = string(&data["sessionId"]).unwrap_or(std::mem::take(&mut s.id));
                let context = &data["context"];
                s.fill_cwd(string(&context["cwd"]).or_else(|| string(&context["gitRoot"])));
                s.touch(&data["startTime"]);
            }
            "session.info" if s.cwd.is_none() => {
                s.cwd = string(&data["message"]).and_then(|m| {
                    m.strip_prefix("Folder ")
                        .and_then(|m| m.strip_suffix(" has been added to trusted folders."))
                        .map(|folder| folder.trim().to_owned())
                });
            }
            "user.message" => s.add(
                "user",
                or(&data["content"], &data["transformedContent"]),
                &r["timestamp"],
            ),
            "assistant.message" => s.add("assistant", &data["content"], &r["timestamp"]),
            _ => {}
        }
    }
}

fn decode_cursor(s: &mut DecodedSession, file: &str, records: &[&Value], sidecar: &Value) {
    let directory = parent(file);
    if base_name(&directory) != "agent-transcripts" {
        s.id = base_name(&directory).to_owned();
    }
    s.cwd = string(&sidecar["cwd"]);
    s.title = string(&sidecar["title"]);
    s.touch(&sidecar["createdAtMs"]);
    s.touch(&sidecar["updatedAtMs"]);
    for r in records {
        let role = text(r, "role");
        if role == "user" || role == "assistant" {
            s.add(
                role,
                or(&r["message"]["content"], &r["content"]),
                &r["timestamp"],
            );
        }
    }
}

fn decode_droid(s: &mut DecodedSession, records: &[&Value]) {
    for r in records {
        let kind = text(r, "type");
        if ["session_start", "session", "session_meta"].contains(&kind) {
            s.id = string(&r["id"]).unwrap_or(std::mem::take(&mut s.id));
            s.fill_cwd(string(&r["cwd"]));
        }
        s.id = string(&r["session_id"])
            .or_else(|| string(&r["sessionId"]))
            .unwrap_or(std::mem::take(&mut s.id));
        s.fill_cwd(string(&r["cwd"]));
        let m = &r["message"];
        let role = or(&r["role"], &m["role"]).as_str().unwrap_or("");
        if role == "user" || role == "assistant" {
            s.add(role, or(&m["content"], &r["content"]), &r["timestamp"]);
        }
        if kind == "completion" {
            s.add("assistant", &r["finalText"], &r["timestamp"]);
        }
    }
}

fn decode_hermes(s: &mut DecodedSession, provider: &str, document: &Value) {
    let hermes = provider == "hermes";
    let id_keys: &[&str] = if hermes {
        &["session_id"]
    } else {
        &["id", "session_id"]
    };
    let cwd_keys: &[&str] = if hermes {
        &["cwd"]
    } else {
        &["working_dir", "working_directory"]
    };
    s.id = first_string(document, id_keys).unwrap_or(std::mem::take(&mut s.id));
    s.cwd = first_string(document, cwd_keys);
    s.title = first_string(document, &["custom_title", "title"]);
    s.touch(or(&document["session_start"], &document["created_at"]));
    s.touch(or(&document["last_updated"], &document["updated_at"]));
    for m in document["messages"].as_array().into_iter().flatten() {
        if !hermes && is_jcode_system_message(m) {
            continue;
        }
        s.add_message(m);
    }
}

fn is_jcode_system_message(m: &Value) -> bool {
    let body = plain(&m["content"]);
    let body = body.trim_start();
    ["system", "background_task"].contains(&text(m, "display_role"))
        || m["role"] == "system"
        || body.starts_with("[Scheduled task]")
        || body.starts_with("<system-reminder>")
}

fn decode_rovo(s: &mut DecodedSession, file: &str, document: &Value, sidecar: &Value) {
    s.id = base_name(&parent(file)).to_owned();
    s.cwd = first_string(
        document,
        &[
            "workspace_path",
            "workspacePath",
            "workspace",
            "cwd",
            "working_directory",
            "workingDirectory",
            "project_path",
            "projectPath",
        ],
    );
    s.title = first_string(document, &["title", "name", "summary"]);
    s.touch(or(&document["created_at"], &document["createdAt"]));
    s.touch(or(&document["updated_at"], &document["updatedAt"]));
    for m in sidecar["messages"].as_array().into_iter().flatten() {
        s.add_message(m);
    }
    for entry in sidecar["message_history"].as_array().into_iter().flatten() {
        let role = match (entry["role"].as_str(), text(entry, "kind")) {
            (Some(role), _) => role,
            (None, "request") => "user",
            (None, "response") => "assistant",
            _ => continue,
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let parts: Vec<Value> = entry["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                let kind = text(p, "part_kind");
                kind == "text" || (role == "user" && kind == "user-prompt")
            })
            .map(|p| or(&p["content"], &p["text"]).clone())
            .collect();
        s.add(role, &Value::Array(parts), &entry["timestamp"]);
    }
}

fn decode_grok(s: &mut DecodedSession, file: &str, document: &Value, sidecar: &Value) {
    let info = &document["info"];
    s.id = string(&info["id"]).unwrap_or_else(|| base_name(&parent(file)).to_owned());
    s.cwd = string(&info["cwd"]);
    s.title = first_string(document, &["generated_title", "session_summary"]);
    s.touch(&document["created_at"]);
    s.touch(or(&document["updated_at"], &document["last_active_at"]));
    for r in sidecar["lines"].as_array().into_iter().flatten() {
        let kind = text(r, "type");
        if kind == "user" || kind == "assistant" {
            s.add(kind, &r["content"], &r["timestamp"]);
        }
    }
}

fn decode_devin(s: &mut DecodedSession, document: &Value, sidecar: &Value) {
    s.id =
        first_string(document, &["session_id", "sessionId"]).unwrap_or(std::mem::take(&mut s.id));
    s.cwd =
        string(&document["working_directory"]).or_else(|| string(&sidecar["working_directory"]));
    s.title = string(&sidecar["title"]);
    s.touch(&sidecar["created_at"]);
    s.touch(&sidecar["last_activity_at"]);
    for step in document["steps"].as_array().into_iter().flatten() {
        let source = text(step, "source");
        let metadata = &step["metadata"];
        if source == "system" {
            continue;
        }
        let is_user = source == "user" || metadata["is_user_input"] == true;
        if !is_user
            && source != "agent"
            && step["role"] != "assistant"
            && !truthy(&step["tool_calls"])
        {
            continue;
        }
        let content = or(
            or(&step["message"]["content"], &step["message"]),
            or(&step["text"], &step["content"]),
        );
        let role = if is_user { "user" } else { "assistant" };
        s.add(
            role,
            content,
            or(&step["timestamp"], &metadata["created_at"]),
        );
    }
}

fn decode_cline(s: &mut DecodedSession, file: &str, document: &Value, sidecar: &Value) {
    s.id = string(&document["session_id"]).unwrap_or_else(|| base_name(&parent(file)).to_owned());
    s.cwd = first_string(document, &["cwd", "workspace_root"]);
    s.title = string(&document["prompt"]);
    s.touch(&document["started_at"]);
    s.touch(&sidecar["updated_at"]);
    for m in sidecar["messages"].as_array().into_iter().flatten() {
        s.add_message(m);
    }
}

fn decode_kiro_blocks(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|block| {
            let data = &block["data"];
            match text(block, "kind") {
                "text" => vec![json!({"text":data})],
                "json" => vec![json!({"text":data.to_string()})],
                "toolResult" => decode_kiro_blocks(&data["content"]),
                "toolUse" => {
                    let input = if data["input"].is_null() { json!({}) } else { data["input"].clone() };
                    vec![json!({"text":format!("{}: {}", data["name"].as_str().unwrap_or("tool"), input)})]
                }
                _ => vec![],
            }
        })
        .collect()
}

fn decode_kiro(s: &mut DecodedSession, document: &Value, sidecar: &Value) {
    s.cwd = string(&document["cwd"]);
    s.title = string(&document["title"]);
    s.touch(&document["created_at"]);
    s.touch(&document["updated_at"]);
    for r in sidecar["lines"].as_array().into_iter().flatten() {
        let data = &r["data"];
        let role = match text(r, "kind") {
            "Prompt" => "user",
            "AssistantMessage" => "assistant",
            "ToolResults" => "tool",
            _ => continue,
        };
        let blocks = Value::Array(decode_kiro_blocks(&data["content"]));
        s.add(role, &blocks, &data["meta"]["timestamp"]);
    }
}

fn decode_kimi(s: &mut DecodedSession, file: &str, document: &Value, sidecar: &Value) {
    s.id = base_name(&parent(file)).to_owned();
    s.cwd = string(&sidecar["workDir"]);
    s.title = first_string(document, &["title", "lastPrompt"]);
    s.touch(&document["createdAt"]);
    s.touch(&document["updatedAt"]);
    // 응답은 여러 content.part 이벤트로 나뉘므로 step.end나 다음 사용자 입력까지 이어 붙인다.
    let mut pending = String::new();
    for r in sidecar["lines"].as_array().into_iter().flatten() {
        match text(r, "type") {
            "context.append_message" => {
                let m = &r["message"];
                if m["role"] == "user" && m["origin"]["kind"] == "user" {
                    flush_kimi(s, &mut pending);
                    s.add("user", &m["content"], &Value::Null);
                }
            }
            "context.append_loop_event" => {
                let event = &r["event"];
                let part = &event["part"];
                if event["type"] == "content.part" && part["type"] == "text" {
                    pending.push_str(text(part, "text"));
                }
                if event["type"] == "step.end" {
                    flush_kimi(s, &mut pending);
                }
            }
            _ => {}
        }
    }
    flush_kimi(s, &mut pending);
}

fn flush_kimi(s: &mut DecodedSession, pending: &mut String) {
    if !pending.is_empty() {
        s.add_text("assistant", std::mem::take(pending), &Value::Null);
    }
}

fn strip_user_envelope(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return value.clone();
    };
    let Some(start) = raw.find("<USER_REQUEST>") else {
        return value.clone();
    };
    let body = &raw[start + "<USER_REQUEST>".len()..];
    json!(body
        .find("</USER_REQUEST>")
        .map_or(body, |end| &body[..end]))
}

fn decode_antigravity(s: &mut DecodedSession, file: &str, records: &[&Value], sidecar: &Value) {
    s.id = antigravity_id(file);
    s.cwd = string(&sidecar["workspace"]);
    for r in records {
        let source = text(r, "source");
        let kind = text(r, "type");
        if ["USER_EXPLICIT", "USER"].contains(&source) && ["USER_INPUT", "REQUEST"].contains(&kind)
        {
            s.add(
                "user",
                &strip_user_envelope(&r["content"]),
                &r["created_at"],
            );
        }
        if source == "MODEL" && kind == "PLANNER_RESPONSE" {
            s.add("assistant", &r["content"], &r["created_at"]);
        }
    }
}

fn muse_entries(envelope: &Value) -> Vec<Value> {
    let Some(children) = envelope["children"].as_array() else {
        return vec![envelope.clone()];
    };
    children
        .iter()
        .filter_map(|child| match &child["record_json"] {
            Value::String(raw) => serde_json::from_str(raw).ok(),
            other => Some(other.clone()),
        })
        .collect()
}

fn decode_muse(s: &mut DecodedSession, file: &str, records: &[&Value]) {
    s.id = base_name(&parent(file)).to_owned();
    let mut last_user: Option<(String, Option<i64>)> = None;
    for record in records.iter().flat_map(|envelope| muse_entries(envelope)) {
        let payload = &record["payload"];
        let value = &payload["record"];
        // 마이크로초 단위 기록 시각은 밀리초로 낮춘다.
        let recorded_at = match record["recorded_at"].as_f64() {
            Some(t) if t >= 1e14 => json!((t / 1000.0).floor() as i64),
            _ => record["recorded_at"].clone(),
        };
        match text(&record, "payload_type") {
            "runtime.session.metadata" => s.fill_cwd(string(&value["workspace_root"])),
            "runtime.session.route_facts" => s.fill_cwd(string(&value["cwd"])),
            "runtime.user_intent.accepted" => {
                let prompt = muse_intent_text(payload);
                if !prompt.is_empty() {
                    s.add_text("user", prompt.clone(), &recorded_at);
                    last_user = Some((prompt, epoch_millis(&recorded_at)));
                }
            }
            "runtime.session" => {
                let event = &payload["event"];
                if event["kind"] == "started" {
                    if let Some(prompt) = event["prompt"].as_str() {
                        // runtime.started는 accepted intent를 반복하므로 같은 분의 fallback만 제외한다.
                        let time = epoch_millis(&recorded_at);
                        let repeated = last_user.as_ref().is_some_and(|(text, at)| {
                            text == prompt
                                && matches!((at, time), (Some(a), Some(b)) if (a - b).abs() < MUSE_REPEATED_USER_WINDOW_MS)
                        });
                        if !repeated {
                            s.add_text("user", prompt.into(), &recorded_at);
                        }
                    }
                }
                if event["kind"] == "assistant_message_committed" {
                    s.add("assistant", &event["text"], &recorded_at);
                }
            }
            _ => {}
        }
    }
}

fn muse_intent_text(payload: &Value) -> String {
    let refill = payload["refill_blocks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let blocks = if refill.is_empty() {
        payload["model_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
            .collect()
    } else {
        refill
    };
    blocks
        .iter()
        .find(|b| string(&b["text"]).is_some())
        .map(plain)
        .unwrap_or_default()
}

fn sql_value(value: rusqlite::types::ValueRef) -> Value {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Null | ValueRef::Blob(_) => Value::Null,
    }
}
fn open_read_only(path: &str) -> Result<rusqlite::Connection, String> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = rusqlite::Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}
fn table_columns(conn: &rusqlite::Connection, table: &str) -> Result<HashSet<String>, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(columns)
}
fn has_columns(columns: &HashSet<String>, required: &[&str]) -> bool {
    required.iter().all(|c| columns.contains(*c))
}
fn run_python(ctx: &Context, script: &str, argument: &str) -> Result<Value, String> {
    let output = command(
        ctx,
        &format!("python3 -c {} {}", quote(script), quote(argument)),
    )?;
    serde_json::from_str(output.trim())
        .map_err(|_| "Invalid Agent History database response".into())
}

const READ_DEVIN_INDEX: &str = r#"
import json, sqlite3, sys
from urllib.parse import quote
connection = sqlite3.connect("file:" + quote(sys.argv[1], safe="/") + "?mode=ro", uri=True, timeout=2)
try:
    connection.row_factory = sqlite3.Row
    columns = {row[1] for row in connection.execute("PRAGMA table_info(sessions)")}
    if "id" not in columns:
        print("[]")
    else:
        selected = [name for name in ["id", "working_directory", "title", "created_at", "last_activity_at", "hidden"] if name in columns]
        print(json.dumps([dict(row) for row in connection.execute("SELECT " + ",".join(selected) + " FROM sessions")]))
finally:
    connection.close()
"#;
const DEVIN_INDEX_COLUMNS: [&str; 6] = [
    "id",
    "working_directory",
    "title",
    "created_at",
    "last_activity_at",
    "hidden",
];

/// Devin transcript 옆 sessions.db에서 cwd·제목·숨김 여부를 읽는다. 조회 한 번 안에서만 공유해 변경을 바로 반영한다.
fn read_devin_index(ctx: &Context, db: &str) -> Result<HashMap<String, Value>, String> {
    if !path_exists(ctx, db) {
        return Ok(HashMap::new());
    }
    let rows = if ctx.host.is_some() {
        run_python(ctx, READ_DEVIN_INDEX, db)?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        read_devin_rows(db)?
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| Some((row["id"].as_str()?.to_owned(), row)))
        .collect())
}

fn read_devin_rows(db: &str) -> Result<Vec<Value>, String> {
    let conn = open_read_only(db)?;
    let columns = table_columns(&conn, "sessions")?;
    if !columns.contains("id") {
        return Ok(vec![]);
    }
    let selected: Vec<&str> = DEVIN_INDEX_COLUMNS
        .into_iter()
        .filter(|c| columns.contains(*c))
        .collect();
    let mut statement = conn
        .prepare(&format!("SELECT {} FROM sessions", selected.join(",")))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let mut record = Map::new();
            for (i, name) in selected.iter().enumerate() {
                record.insert((*name).into(), sql_value(row.get_ref(i)?));
            }
            Ok(Value::Object(record))
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

pub(super) const SQLITE_PROVIDERS: [&str; 2] = ["opencode2", "zcode"];

const SQLITE_AGENT_READ_SCRIPT: &str = r#"
import base64,json,sqlite3,sys
from pathlib import Path
r=json.loads(base64.b64decode(sys.argv[1]).decode('utf-8'))
db=sqlite3.connect(Path(r['dbPath']).as_uri()+'?mode=ro',uri=True,timeout=5)
try:
    cols=lambda t:{row[1] for row in db.execute('PRAGMA table_info('+t+')')}
    p=r['provider'];table='session_v2' if p=='opencode2' else 'session';sc=cols(table)
    if not {'id','directory','time_created','time_updated'}<=sc:
        print(json.dumps({'schemaAvailable':False,'sessions':[]}));sys.exit(0)
    if p=='opencode2':
        if not {'session_id','type','data','time_created','seq'}<=cols('session_message'):raise ValueError('OpenCode 2 transcript schema is unreadable')
        src,role,content,visible,order='session_message m','m.type','m.data','1=1','m.time_created ASC,m.seq ASC'
        count='SELECT COUNT(*) FROM session_message m WHERE m.session_id=?'
    else:
        mc,pc=cols('message'),cols('part')
        if not ({'id','session_id','data','time_created'}<=mc and {'id','message_id','data','time_created'}<=pc):raise ValueError('ZCode transcript schema is unreadable')
        src,role,content='message m JOIN part p ON p.message_id=m.id',"json_extract(m.data,'$.role')",'p.data'
        visible="COALESCE(json_extract(m.data,'$.semantics.transcriptVisibility'),'visible')!='hidden'"
        order=('m.sequence IS NULL ASC,m.sequence ASC,' if 'sequence' in mc else '')+'m.time_created ASC,m.rowid ASC,'+('p.sequence IS NULL ASC,p.sequence ASC,' if 'sequence' in pc else '')+'p.time_created ASC,p.id ASC'
        count='SELECT COUNT(*) FROM message m WHERE m.session_id=? AND '+visible
    title='s.title' if 'title' in sc else 'NULL'
    w=r['worktree'];prefix=w.rstrip('/')+'/'
    sessions=[]
    for row in db.execute('SELECT s.id,s.directory,'+title+',s.time_created,s.time_updated FROM '+table+' s WHERE s.directory=? OR substr(s.directory,1,length(?))=? ORDER BY s.time_updated DESC,s.id ASC',(w,prefix,prefix)):
        sid=row[0]
        rows=[{'role':m[0],'data':m[1],'time_created':m[2]} for m in db.execute('SELECT '+role+','+content+',m.time_created FROM '+src+' WHERE m.session_id=? AND '+visible+' ORDER BY '+order,(sid,))]
        sessions.append({'id':sid,'directory':row[1],'title':row[2],'time_created':row[3],'time_updated':row[4],'message_count':db.execute(count,(sid,)).fetchone()[0],'messages':rows})
    print(json.dumps({'schemaAvailable':True,'sessions':sessions},ensure_ascii=False))
finally:
    db.close()
"#;

struct TranscriptQuery {
    source: &'static str,
    role: &'static str,
    content: &'static str,
    visible: &'static str,
    order: String,
    count: String,
}

/// 실제 저장소 스키마에서 대화 행의 출처·역할·정렬 기준을 정한다. 필수 열이 없으면 읽지 않는다.
fn transcript_query(
    conn: &rusqlite::Connection,
    provider: &str,
) -> Result<TranscriptQuery, String> {
    if provider == "opencode2" {
        let columns = table_columns(conn, "session_message")?;
        if !has_columns(
            &columns,
            &["session_id", "type", "data", "time_created", "seq"],
        ) {
            return Err("OpenCode 2 transcript schema is unreadable".into());
        }
        return Ok(TranscriptQuery {
            source: "session_message m",
            role: "m.type",
            content: "m.data",
            visible: "1=1",
            order: "m.time_created ASC,m.seq ASC".into(),
            count: "SELECT COUNT(*) FROM session_message m WHERE m.session_id=?1".into(),
        });
    }
    let message_columns = table_columns(conn, "message")?;
    let part_columns = table_columns(conn, "part")?;
    if !has_columns(
        &message_columns,
        &["id", "session_id", "data", "time_created"],
    ) || !has_columns(&part_columns, &["id", "message_id", "data", "time_created"])
    {
        return Err("ZCode transcript schema is unreadable".into());
    }
    let visible =
        "COALESCE(json_extract(m.data,'$.semantics.transcriptVisibility'),'visible')!='hidden'";
    let sequence = |columns: &HashSet<String>, alias: &str| {
        if columns.contains("sequence") {
            format!("{alias}.sequence IS NULL ASC,{alias}.sequence ASC,")
        } else {
            String::new()
        }
    };
    Ok(TranscriptQuery {
        source: "message m JOIN part p ON p.message_id=m.id",
        role: "json_extract(m.data,'$.role')",
        content: "p.data",
        visible,
        order: format!(
            "{}m.time_created ASC,m.rowid ASC,{}p.time_created ASC,p.id ASC",
            sequence(&message_columns, "m"),
            sequence(&part_columns, "p")
        ),
        count: format!("SELECT COUNT(*) FROM message m WHERE m.session_id=?1 AND {visible}"),
    })
}

/// 원격 Python 응답과 같은 모양으로 로컬 SQLite 저장소를 읽는다.
fn read_sqlite_local(provider: &str, db: &str, worktree: &str) -> Result<Value, String> {
    let conn = open_read_only(db)?;
    let table = if provider == "opencode2" {
        "session_v2"
    } else {
        "session"
    };
    let session_columns = table_columns(&conn, table)?;
    if !has_columns(
        &session_columns,
        &["id", "directory", "time_created", "time_updated"],
    ) {
        return Ok(json!({"schemaAvailable":false,"sessions":[]}));
    }
    let plan = transcript_query(&conn, provider)?;
    let title = if session_columns.contains("title") {
        "s.title"
    } else {
        "NULL"
    };
    let prefix = format!("{}/", worktree.trim_end_matches('/'));
    let mut statement = conn
        .prepare(&format!("SELECT s.id,s.directory,{title},s.time_created,s.time_updated FROM {table} s WHERE s.directory=?1 OR substr(s.directory,1,length(?2))=?2 ORDER BY s.time_updated DESC,s.id ASC"))
        .map_err(|e| e.to_string())?;
    let rows: Vec<Vec<Value>> = statement
        .query_map([worktree, prefix.as_str()], |row| {
            (0..5).map(|i| row.get_ref(i).map(sql_value)).collect()
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let message_sql = format!(
        "SELECT {},{},m.time_created FROM {} WHERE m.session_id=?1 AND {} ORDER BY {}",
        plan.role, plan.content, plan.source, plan.visible, plan.order
    );
    let mut sessions = Vec::new();
    for row in rows {
        let id = row[0].as_str().unwrap_or_default().to_owned();
        let count: i64 = conn
            .query_row(&plan.count, [&id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let mut messages = conn.prepare(&message_sql).map_err(|e| e.to_string())?;
        let messages: Vec<Value> = messages
            .query_map([&id], |r| {
                Ok(json!({"role":sql_value(r.get_ref(0)?),"data":sql_value(r.get_ref(1)?),"time_created":sql_value(r.get_ref(2)?)}))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        sessions.push(json!({"id":id,"directory":row[1],"title":row[2],"time_created":row[3],"time_updated":row[4],"message_count":count,"messages":messages}));
    }
    Ok(json!({"schemaAvailable":true,"sessions":sessions}))
}

fn read_sqlite_database(ctx: &Context, provider: &str, db: &str) -> Result<Value, String> {
    let worktree = normalize_path(&ctx.target).to_string_lossy().to_string();
    if ctx.host.is_none() {
        return read_sqlite_local(provider, db, &worktree);
    }
    let request = json!({"dbPath":db,"provider":provider,"worktree":worktree}).to_string();
    let payload = base64::engine::general_purpose::STANDARD.encode(request);
    let value = run_python(ctx, SQLITE_AGENT_READ_SCRIPT, &payload)?;
    if !value["sessions"].is_array() {
        return Err("Invalid Agent History database response".into());
    }
    Ok(value)
}

fn sqlite_databases(ctx: &Context, provider: &str) -> Vec<String> {
    if provider == "zcode" {
        let db = join(&ctx.home, &[".zcode", "cli", "db", "db.sqlite"]);
        return if path_exists(ctx, &db) {
            vec![db]
        } else {
            vec![]
        };
    }
    let directory = join(&ctx.home, &[".local", "share", "opencode"]);
    let mut databases: Vec<String> = directory_files(ctx, &directory, ".db")
        .into_iter()
        .filter(|f| is_opencode_database(base_name(f)))
        .collect();
    databases.sort_by(|a, b| {
        (base_name(b) == "opencode.db")
            .cmp(&(base_name(a) == "opencode.db"))
            .then_with(|| a.cmp(b))
    });
    databases
}

fn is_opencode_database(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix("opencode")
        .and_then(|n| n.strip_suffix(".db"))
    else {
        return false;
    };
    middle.is_empty()
        || middle.strip_prefix('-').is_some_and(|tag| {
            !tag.is_empty()
                && tag
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
        })
}

fn decode_sqlite_content(provider: &str, raw: &Value) -> (String, Option<&'static str>) {
    let Some(record) = raw
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .filter(Value::is_object)
    else {
        return (String::new(), None);
    };
    if provider == "opencode2" {
        let content = or(&record["content"], &record["text"]);
        let Some(parts) = content.as_array() else {
            return (plain(content), None);
        };
        let body = parts
            .iter()
            .map(|part| {
                if part["type"] == "tool" {
                    format!(
                        "{}: {}",
                        part["name"].as_str().unwrap_or("tool"),
                        plain(&part["state"])
                    )
                } else {
                    plain(part)
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return (body, None);
    }
    match text(&record, "type") {
        "reasoning" => (plain(&record["text"]), Some("reasoning")),
        "tool" => {
            let state = &record["state"];
            let detail = or(or(&state["output"], &state["input"]), state);
            let name = record["tool"].as_str().unwrap_or("tool");
            (format!("{name}: {}", plain(detail)), Some("tool"))
        }
        "text" => (plain(&record["text"]), None),
        _ => (String::new(), None),
    }
}

fn sqlite_role(role: &str) -> &str {
    if [
        "user",
        "assistant",
        "system",
        "developer",
        "reasoning",
        "tool",
    ]
    .contains(&role)
    {
        role
    } else {
        "unknown"
    }
}

fn sqlite_transcript(provider: &str, db: &str, session: &Value) -> Transcript {
    let rows = session["messages"].as_array().cloned().unwrap_or_default();
    let messages: Vec<Value> = rows
        .iter()
        .filter_map(|row| {
            let (body, decoded_role) = decode_sqlite_content(provider, &row["data"]);
            let role = decoded_role.unwrap_or_else(|| sqlite_role(text(row, "role")));
            message(role, &row["time_created"], body)
        })
        .collect();
    let first = rows
        .iter()
        .find(|row| row["role"] == "user")
        .map(|row| decode_sqlite_content(provider, &row["data"]).0)
        .filter(|s| !s.is_empty());
    let title = session["title"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| first.as_deref().map(|s| trunc(s, 80)));
    let summary = json!({"id":session["id"],"provider":provider,"matchScope":"worktree","matchedPath":session["directory"],"title":title,"firstUserPrompt":first.map(|s|trunc(&s,PREVIEW_TEXT_LENGTH)),"startedAt":iso(&session["time_created"]),"updatedAt":iso(&session["time_updated"]),"messageCount":session["message_count"],"sourceRef":db});
    summary_transcript(summary, messages)
}

/// OpenCode 2와 ZCode의 SQLite History를 읽는다. 설치별 DB 하나의 손상이 나머지를 가리지 않는다.
pub(super) fn read_sqlite_provider(ctx: &Context, provider: &str) -> (Vec<Transcript>, Value) {
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let mut available = false;
    let mut failures = Vec::new();
    for db in sqlite_databases(ctx, provider) {
        let result = match read_sqlite_database(ctx, provider, &db) {
            Ok(result) => result,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        available |= result["schemaAvailable"] == true;
        for session in result["sessions"].as_array().into_iter().flatten() {
            let id = text(session, "id");
            if matches_path(ctx, text(session, "directory")) && seen.insert(id.to_owned()) {
                sessions.push(sqlite_transcript(provider, &db, session));
            }
        }
    }
    let reason = if !failures.is_empty() {
        json!(failures.join("; "))
    } else if available {
        Value::Null
    } else {
        json!("Agent History database or schema not found")
    };
    let status = status(provider, available, sessions.len(), reason);
    (sessions, status)
}

fn is_storage_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn legacy_storage(ctx: &Context) -> String {
    let data_home = configured(
        ctx.host.is_some(),
        "XDG_DATA_HOME",
        join(&ctx.home, &[".local", "share"]),
    );
    join(&data_home, &["opencode", "storage"])
}

/// 오래된 설치의 부분 쓰기·삭제 파일 하나가 나머지 대화를 가리지 않도록 실패는 None으로 둔다.
fn read_legacy_json(ctx: &Context, file: &str, root: &str) -> Option<Value> {
    if !is_within(file, root) || !file.ends_with(".json") {
        return None;
    }
    if ctx.host.is_none() {
        let is_link = fs::symlink_metadata(file).ok()?.file_type().is_symlink();
        if is_link || !real_within(file, root) {
            return None;
        }
    }
    serde_json::from_str::<Value>(&content(ctx, file).ok()?)
        .ok()
        .filter(Value::is_object)
}

fn legacy_message_text(ctx: &Context, message: &Value, id: &str, root: &str) -> String {
    let inline = plain(&message["content"]);
    if !inline.is_empty() || !is_storage_id(id) {
        return inline;
    }
    let mut part_files = directory_files(ctx, &join(root, &["part", id]), ".json");
    part_files.sort();
    part_files
        .iter()
        .filter_map(|file| read_legacy_json(ctx, file, root))
        .filter(|part| part["messageID"].is_null() || part["messageID"] == id)
        .map(|part| plain(&part))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_legacy_session(ctx: &Context, file: &str, root: &str) -> Option<Transcript> {
    if !is_within(file, &join(root, &["session"])) {
        return None;
    }
    let metadata = read_legacy_json(ctx, file, root)?;
    let id = metadata["id"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| stem(file));
    let directory = text(&metadata, "directory");
    if !is_storage_id(&id) || !matches_path(ctx, directory) {
        return None;
    }
    let mut stored: Vec<(String, Value)> =
        directory_files(ctx, &join(root, &["message", &id]), ".json")
            .into_iter()
            .filter_map(|f| read_legacy_json(ctx, &f, root).map(|v| (f, v)))
            .collect();
    stored.sort_by_key(|(f, v)| {
        (
            epoch_millis(&v["time"]["created"]).unwrap_or(i64::MAX),
            f.clone(),
        )
    });
    let messages: Vec<Value> = stored
        .iter()
        .filter(|(_, v)| v["sessionID"].is_null() || v["sessionID"] == id.as_str())
        .filter_map(|(f, v)| {
            let role = text(v, "role");
            if !MESSAGE_ROLES.contains(&role) {
                return None;
            }
            let message_id = string(&v["id"]).unwrap_or_else(|| stem(f));
            message(
                role,
                &v["time"]["created"],
                legacy_message_text(ctx, v, &message_id, root),
            )
        })
        .collect();
    let first = first_user_text(&messages);
    let title = string(&metadata["title"]).or_else(|| first.as_deref().map(|s| trunc(s, 80)));
    let updated = messages
        .iter()
        .filter_map(|m| m["timestamp"].as_str())
        .max()
        .map(|s| json!(s))
        .unwrap_or_else(|| iso(&metadata["time"]["updated"]));
    let summary = json!({"id":id,"provider":"opencode","title":title,"firstUserPrompt":first.map(|s|trunc(&s,PREVIEW_TEXT_LENGTH)),"matchedPath":directory,"matchScope":"worktree","sourceRef":file,"startedAt":iso(&metadata["time"]["created"]),"updatedAt":updated,"messageCount":messages.len()});
    Some(summary_transcript(summary, messages))
}

/// SQLite 이전 OpenCode가 storage/ 아래 JSON 파일로 남긴 대화를 읽는다.
pub(super) fn read_legacy_open_code(ctx: &Context) -> Result<Vec<Transcript>, String> {
    let root = legacy_storage(ctx);
    let session_root = join(&root, &["session"]);
    if !path_exists(ctx, &session_root) {
        return Err("Legacy OpenCode storage not found".into());
    }
    Ok(files(ctx, &session_root, ".json")
        .iter()
        .filter_map(|file| read_legacy_session(ctx, file, &root))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    const CWD: &str = "/projects/history-fixture";
    const AT: &str = "2026-11-01T10:00:00Z";
    const LATER: &str = "2026-11-01T10:01:00Z";
    const USER: &str = "Please explain the change";
    const ANSWER: &str = "The change is complete";

    fn context(home: &Path, target: &str) -> Context {
        Context {
            target: target.into(),
            repo: target.into(),
            host: None,
            home: home.to_string_lossy().into(),
            bounded: false,
            recent: false,
            query: String::new(),
        }
    }
    fn put(home: &Path, relative: &str, value: &Value, jsonl: bool) -> String {
        let file = home.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let body = match (jsonl, value.as_array()) {
            (true, Some(items)) => items
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            _ => value.to_string(),
        };
        fs::write(&file, body).unwrap();
        file.to_string_lossy().into()
    }
    fn read(home: &Path, provider: &str) -> (Vec<Transcript>, Value) {
        read_file_provider(&context(home, CWD), provider).unwrap()
    }
    fn texts(t: &Transcript) -> Vec<String> {
        t.messages
            .iter()
            .map(|m| text(m, "fullText").to_owned())
            .collect()
    }

    struct Fixture {
        provider: &'static str,
        file: &'static str,
        value: Value,
        jsonl: bool,
        sidecars: Vec<(&'static str, Value, bool)>,
    }
    fn fixture(provider: &'static str, file: &'static str, value: Value, jsonl: bool) -> Fixture {
        Fixture {
            provider,
            file,
            value,
            jsonl,
            sidecars: vec![],
        }
    }
    fn fixtures() -> Vec<Fixture> {
        let pi = |provider, file| {
            fixture(
                provider,
                file,
                json!([{"type":"session","id":"p","cwd":CWD,"timestamp":AT},{"type":"message","message":{"role":"user","content":USER},"timestamp":AT},{"type":"message","message":{"role":"assistant","content":ANSWER},"timestamp":LATER}]),
                true,
            )
        };
        let with = |mut f: Fixture, sidecars: Vec<(&'static str, Value, bool)>| {
            f.sidecars = sidecars;
            f
        };
        vec![
            fixture(
                "qoder",
                ".qoder/projects/task/q.jsonl",
                json!([{"type":"workspace-directories","directories":[CWD]},{"type":"user","sessionId":"q","timestamp":AT,"message":{"role":"user","content":USER}},{"type":"assistant","sessionId":"q","timestamp":LATER,"message":{"role":"assistant","content":ANSWER}}]),
                true,
            ),
            fixture(
                "codebuddy",
                ".codebuddy/projects/task/cb.jsonl",
                json!([{"type":"message","sessionId":"cb","cwd":CWD,"role":"user","content":USER,"timestamp":AT},{"type":"message","sessionId":"cb","role":"assistant","content":[{"type":"output_text","text":ANSWER}],"timestamp":LATER}]),
                true,
            ),
            fixture(
                "hermes",
                ".hermes/sessions/session_h.json",
                json!({"session_id":"h","cwd":CWD,"session_start":AT,"messages":[{"role":"user","content":USER,"timestamp":AT},{"role":"assistant","content":ANSWER,"timestamp":LATER}]}),
                false,
            ),
            pi("pi", ".pi/agent/sessions/p.jsonl"),
            pi("omp", ".omp/agent/sessions/p.jsonl"),
            pi("prime-agent", ".prime/agent/sessions/p.jsonl"),
            pi("openclaw", ".openclaw/agents/main/sessions/p.jsonl"),
            with(
                fixture(
                    "cursor",
                    ".cursor/projects/task/agent-transcripts/cu/cu.jsonl",
                    json!([{"role":"user","message":{"content":USER},"timestamp":AT},{"role":"assistant","message":{"content":ANSWER},"timestamp":LATER}]),
                    true,
                ),
                vec![(
                    ".cursor/chats/hash/cu/meta.json",
                    json!({"agentId":"cu","cwd":CWD,"title":"Cursor session"}),
                    false,
                )],
            ),
            with(
                fixture(
                    "antigravity",
                    ".gemini/antigravity-cli/brain/ag/.system_generated/logs/transcript.jsonl",
                    json!([{"source":"USER_EXPLICIT","type":"USER_INPUT","content":format!("<USER_REQUEST>{USER}</USER_REQUEST>"),"created_at":AT},{"source":"MODEL","type":"PLANNER_RESPONSE","content":ANSWER,"created_at":LATER}]),
                    true,
                ),
                vec![(
                    ".gemini/antigravity-cli/history.jsonl",
                    json!([{"conversationId":"ag","workspace":CWD,"timestamp":AT}]),
                    true,
                )],
            ),
            with(
                fixture(
                    "rovo",
                    ".rovodev/sessions/r/metadata.json",
                    json!({"workspace_path":CWD,"created_at":AT}),
                    false,
                ),
                vec![(
                    ".rovodev/sessions/r/session_context.json",
                    json!({"messages":[{"role":"user","content":USER,"timestamp":AT},{"role":"assistant","content":ANSWER,"timestamp":LATER}]}),
                    false,
                )],
            ),
            fixture(
                "copilot",
                ".copilot/session-state/co/events.jsonl",
                json!([{"type":"session.start","data":{"sessionId":"co","context":{"cwd":CWD},"startTime":AT}},{"type":"user.message","data":{"content":USER},"timestamp":AT},{"type":"assistant.message","data":{"content":ANSWER},"timestamp":LATER}]),
                true,
            ),
            with(
                fixture(
                    "grok",
                    ".grok/sessions/gr/summary.json",
                    json!({"info":{"id":"gr","cwd":CWD},"created_at":AT}),
                    false,
                ),
                vec![(
                    ".grok/sessions/gr/chat_history.jsonl",
                    json!([{"type":"user","content":USER,"timestamp":AT},{"type":"assistant","content":ANSWER,"timestamp":LATER}]),
                    true,
                )],
            ),
            fixture(
                "devin",
                ".local/share/devin/cli/transcripts/de.json",
                json!({"session_id":"de","working_directory":CWD,"steps":[{"source":"user","message":USER,"timestamp":AT},{"source":"agent","message":{"content":ANSWER},"timestamp":LATER}]}),
                false,
            ),
            fixture(
                "droid",
                ".factory/sessions/dr.jsonl",
                json!([{"type":"session_start","id":"dr","cwd":CWD,"timestamp":AT},{"type":"message","role":"user","content":USER,"timestamp":AT},{"type":"message","role":"assistant","message":{"content":ANSWER},"timestamp":LATER}]),
                true,
            ),
            with(
                fixture(
                    "cline",
                    ".cline/data/sessions/cl/cl.json",
                    json!({"session_id":"cl","cwd":CWD,"started_at":AT}),
                    false,
                ),
                vec![(
                    ".cline/data/sessions/cl/cl.messages.json",
                    json!({"messages":[{"role":"user","content":USER,"ts":AT},{"role":"assistant","content":ANSWER,"ts":LATER}]}),
                    false,
                )],
            ),
            with(
                fixture(
                    "kimi",
                    ".kimi-code/sessions/work/session_ki/state.json",
                    json!({"title":"Kimi session","createdAt":AT}),
                    false,
                ),
                vec![
                    (
                        ".kimi-code/session_index.jsonl",
                        json!([{"sessionId":"session_ki","workDir":CWD}]),
                        true,
                    ),
                    (
                        ".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl",
                        json!([{"type":"context.append_message","message":{"role":"user","origin":{"kind":"user"},"content":USER}},{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":ANSWER}}},{"type":"context.append_loop_event","event":{"type":"step.end"}}]),
                        true,
                    ),
                ],
            ),
            fixture(
                "muse",
                ".local/share/muse/sessions/mu/session.jsonl",
                json!([{"payload_type":"runtime.session.metadata","payload":{"record":{"workspace_root":CWD}},"recorded_at":1793527200000000_i64},{"payload_type":"runtime.user_intent.accepted","payload":{"refill_blocks":[{"text":USER}]},"recorded_at":1793527200000000_i64},{"payload_type":"runtime.session","payload":{"event":{"kind":"assistant_message_committed","text":ANSWER}},"recorded_at":1793527260000000_i64}]),
                true,
            ),
            fixture(
                "jcode",
                ".jcode/sessions/session_j.json",
                json!({"id":"j","working_dir":CWD,"messages":[{"role":"user","content":USER,"timestamp":AT},{"role":"assistant","content":ANSWER,"timestamp":LATER}]}),
                false,
            ),
            with(
                fixture(
                    "kiro",
                    ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.json",
                    json!({"cwd":CWD,"created_at":AT}),
                    false,
                ),
                vec![(
                    ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.jsonl",
                    json!([{"kind":"Prompt","data":{"meta":{"timestamp":AT},"content":[{"kind":"text","data":USER}]}},{"kind":"AssistantMessage","data":{"content":[{"kind":"text","data":ANSWER}]}}]),
                    true,
                )],
            ),
        ]
    }

    #[test]
    fn every_file_provider_reads_its_fixture_and_isolates_other_worktrees() {
        let all = fixtures();
        assert_eq!(all.len(), 19);
        assert_eq!(file_sources("/home", false).len(), 19);
        for f in all {
            let dir = tempfile::tempdir().unwrap();
            put(dir.path(), f.file, &f.value, f.jsonl);
            for (name, value, jsonl) in &f.sidecars {
                put(dir.path(), name, value, *jsonl);
            }
            let (sessions, status) = read(dir.path(), f.provider);
            assert_eq!(sessions.len(), 1, "{}: {status}", f.provider);
            let t = &sessions[0];
            assert_eq!(t.summary["matchedPath"], CWD, "{}", f.provider);
            assert_eq!(t.summary["firstUserPrompt"], USER, "{}", f.provider);
            let mut bodies = texts(t);
            bodies.sort();
            assert_eq!(
                bodies,
                vec![USER.to_string(), ANSWER.to_string()],
                "{}",
                f.provider
            );
            let foreign = read_file_provider(&context(dir.path(), "/other"), f.provider).unwrap();
            assert!(foreign.0.is_empty(), "{}", f.provider);
        }
    }

    #[test]
    fn broken_and_symlinked_files_stay_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let hermes = fixtures()
            .into_iter()
            .find(|f| f.provider == "hermes")
            .unwrap();
        put(home, hermes.file, &hermes.value, false);
        fs::write(home.join(".hermes/sessions/session_bad.json"), "{broken").unwrap();
        let outside = put(
            home,
            "outside.json",
            &json!({"cwd":CWD,"session_id":"secret","messages":[{"role":"user","content":"private"}]}),
            false,
        );
        std::os::unix::fs::symlink(&outside, home.join(".hermes/sessions/session_link.json"))
            .unwrap();
        let (sessions, status) = read(home, "hermes");
        assert_eq!(
            sessions
                .iter()
                .map(|t| text(&t.summary, "id"))
                .collect::<Vec<_>>(),
            vec!["h"]
        );
        assert!(text(&status, "reason").contains("could not be read"));
    }

    #[test]
    fn missing_directory_is_reported_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let (sessions, status) = read(dir.path(), "kiro");
        assert!(sessions.is_empty());
        assert_eq!(status["available"], false);
    }

    #[test]
    fn omp_written_transcripts_in_pi_directory_go_only_to_omp() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            ".pi/agent/sessions/omp.jsonl",
            &json!([{"type":"session","id":"omp-id","cwd":CWD},{"type":"title","v":1,"title":"OMP"},{"type":"message","message":{"role":"user","content":USER}}]),
            true,
        );
        assert!(read(dir.path(), "pi").0.is_empty());
        let omp: Vec<_> = read(dir.path(), "omp")
            .0
            .iter()
            .map(|t| text(&t.summary, "id").to_owned())
            .collect();
        assert_eq!(omp, vec!["omp-id"]);
    }

    #[test]
    fn devin_reads_cwd_and_visibility_from_sibling_database() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            ".local/share/devin/cli/transcripts/de.json",
            &json!({"session_id":"de","steps":[{"source":"user","message":USER,"timestamp":AT}]}),
            false,
        );
        let db = dir.path().join(".local/share/devin/cli/sessions.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(&format!("CREATE TABLE sessions (id TEXT, working_directory TEXT, title TEXT, hidden INTEGER); INSERT INTO sessions VALUES ('de','{CWD}','DB title',0);")).unwrap();
        let first = read(dir.path(), "devin").0;
        assert_eq!(first[0].summary["title"], "DB title");
        assert_eq!(first[0].summary["matchedPath"], CWD);
        conn.execute("UPDATE sessions SET hidden=1", []).unwrap();
        assert!(read(dir.path(), "devin").0.is_empty());
    }

    #[test]
    fn antigravity_does_not_guess_conflicting_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixtures()
            .into_iter()
            .find(|f| f.provider == "antigravity")
            .unwrap();
        put(dir.path(), f.file, &f.value, true);
        put(
            dir.path(),
            ".gemini/antigravity-cli/history.jsonl",
            &json!([{"conversationId":"ag","workspace":CWD,"timestamp":AT},{"conversationId":"ag","workspace":"/other","timestamp":AT}]),
            true,
        );
        assert!(read(dir.path(), "antigravity").0.is_empty());
    }

    #[test]
    fn foreign_graph_transcript_is_skipped_by_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let file = put(
            dir.path(),
            ".pi/agent/sessions/other.jsonl",
            &json!([{"type":"session","id":"other","cwd":"/other"},{"type":"message","message":{"role":"user","content":"large body"}}]),
            true,
        );
        let ctx = context(dir.path(), CWD);
        let mut scope = Scope::new(&ctx);
        let source = file_sources(&ctx.home, false)
            .into_iter()
            .find(|s| s.provider == "pi")
            .unwrap();
        assert!(read_session(&mut scope, &source, &file).unwrap().is_none());
    }

    #[test]
    fn kimi_and_kiro_keep_log_order_without_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            ".kimi-code/sessions/work/session_ki/state.json",
            &json!({"title":"Kimi"}),
            false,
        );
        put(
            dir.path(),
            ".kimi-code/session_index.jsonl",
            &json!([{"sessionId":"session_ki","workDir":CWD}]),
            true,
        );
        let records: Vec<Value> = ["first", "second"].iter().flat_map(|turn| vec![
            json!({"type":"context.append_message","message":{"role":"user","origin":{"kind":"user"},"content":format!("{turn} user")}}),
            json!({"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":format!("{turn} assistant")}}}),
            json!({"type":"context.append_loop_event","event":{"type":"step.end"}}),
        ]).collect();
        put(
            dir.path(),
            ".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl",
            &Value::Array(records),
            true,
        );
        let kimi = &read(dir.path(), "kimi").0[0];
        assert_eq!(
            texts(kimi),
            vec![
                "first user",
                "first assistant",
                "second user",
                "second assistant"
            ]
        );

        let kiro = ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.json";
        put(dir.path(), kiro, &json!({"cwd":CWD,"created_at":AT}), false);
        put(
            dir.path(),
            &format!("{kiro}l"),
            &json!([
                {"kind":"Prompt","data":{"meta":{"timestamp":AT},"content":[{"kind":"text","data":"first user"}]}},
                {"kind":"AssistantMessage","data":{"content":[{"kind":"text","data":"first assistant"}]}},
                {"kind":"ToolResults","data":{"content":[{"kind":"text","data":"tool result"}]}},
            ]),
            true,
        );
        let kiro = &read(dir.path(), "kiro").0[0];
        assert_eq!(
            texts(kiro),
            vec!["first user", "first assistant", "tool result"]
        );
        assert_eq!(kiro.messages[2]["role"], "tool");
    }

    const OPENCODE2_SCHEMA: &str = "CREATE TABLE session_v2 (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE session_message (session_id TEXT,type TEXT,data TEXT,time_created INTEGER,seq INTEGER);";
    const ZCODE_SCHEMA: &str = "CREATE TABLE session (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message (id TEXT PRIMARY KEY,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER); CREATE TABLE part (id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER);";

    fn database(home: &Path, relative: &str, sql: &str) -> String {
        let file = home.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        rusqlite::Connection::open(&file)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        file.to_string_lossy().into()
    }

    #[test]
    fn opencode2_reads_every_matching_database_and_keeps_task_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path(), ".local/share/opencode/opencode-beta.db", &format!("{OPENCODE2_SCHEMA} INSERT INTO session_v2 VALUES ('s','/workspace/task','Beta',1760000000000,1760000002000),('foreign','/workspace/task-other','Other',1760000000000,1760000003000); INSERT INTO session_message VALUES ('s','user','{{\"text\":[\"question\",\"second line\"]}}',1760000000000,1),('s','assistant','{{\"content\":[{{\"type\":\"text\",\"text\":\"answer\"}}]}}',1760000002000,2);"));
        let (sessions, status) =
            read_sqlite_provider(&context(dir.path(), "/workspace/task"), "opencode2");
        assert_eq!(status["available"], true);
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.summary["title"], "Beta");
        assert_eq!(s.summary["messageCount"], 2);
        assert_eq!(s.summary["sourceRef"], db);
        assert_eq!(texts(s), vec!["question\nsecond line", "answer"]);
    }

    #[test]
    fn zcode_respects_visibility_sequence_and_tool_roles() {
        let dir = tempfile::tempdir().unwrap();
        database(dir.path(), ".zcode/cli/db/db.sqlite", &format!("{ZCODE_SCHEMA} INSERT INTO session VALUES ('s','/workspace/task',NULL,1760000000000,1760000003000); INSERT INTO message VALUES ('m1','s','{{\"role\":\"user\"}}',1760000003000,1),('m2','s','{{\"role\":\"assistant\"}}',1760000001000,2),('hidden','s','{{\"role\":\"assistant\",\"semantics\":{{\"transcriptVisibility\":\"hidden\"}}}}',1760000002000,3); INSERT INTO part VALUES ('p1','m1','s','{{\"type\":\"text\",\"text\":\"first prompt\"}}',1760000003000,1),('p2','m2','s','{{\"type\":\"text\",\"text\":\"second reply\"}}',1760000001000,2),('p3','hidden','s','{{\"type\":\"text\",\"text\":\"secret needle\"}}',1760000002000,3);"));
        let (sessions, _) = read_sqlite_provider(&context(dir.path(), "/workspace/task"), "zcode");
        assert_eq!(sessions[0].summary["messageCount"], 2);
        assert_eq!(sessions[0].summary["firstUserPrompt"], "first prompt");
        assert_eq!(texts(&sessions[0]), vec!["first prompt", "second reply"]);

        let legacy = tempfile::tempdir().unwrap();
        database(legacy.path(), ".zcode/cli/db/db.sqlite", &format!("{} INSERT INTO session VALUES ('s','/workspace/task','Tools',1760000000000,1760000003000); INSERT INTO message VALUES ('m','s','{{\"role\":\"assistant\"}}',1760000001000); INSERT INTO part VALUES ('p','m','s','{{\"type\":\"reasoning\",\"text\":\"thinking\"}}',1760000001000),('t','m','s','{{\"type\":\"tool\",\"tool\":\"read\",\"state\":{{\"output\":\"file body\"}}}}',1760000002000);", ZCODE_SCHEMA.replace(",sequence INTEGER", "")));
        let (sessions, _) =
            read_sqlite_provider(&context(legacy.path(), "/workspace/task"), "zcode");
        let roles: Vec<_> = sessions[0]
            .messages
            .iter()
            .map(|m| text(m, "role"))
            .collect();
        assert_eq!(roles, vec!["reasoning", "tool"]);
        assert_eq!(sessions[0].messages[1]["fullText"], "read: file body");
    }

    #[test]
    fn sqlite_keeps_readable_database_beside_broken_schema_and_reports_absence() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = context(dir.path(), "/workspace/task");
        let (_, absent) = read_sqlite_provider(&ctx, "opencode2");
        assert_eq!(absent["available"], false);
        assert!(!dir.path().join(".local/share/opencode").exists());
        database(dir.path(), ".local/share/opencode/opencode.db", "CREATE TABLE session_v2 (id TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);");
        database(dir.path(), ".local/share/opencode/opencode-beta.db", &format!("{OPENCODE2_SCHEMA} INSERT INTO session_v2 VALUES ('healthy','/workspace/task','Healthy',1760000000000,1760000002000); INSERT INTO session_message VALUES ('healthy','user','{{\"text\":\"hello\"}}',1760000000000,1);"));
        let (sessions, status) = read_sqlite_provider(&ctx, "opencode2");
        assert_eq!(status["available"], true);
        assert_eq!(sessions[0].summary["id"], "healthy");
        assert!(text(&status, "reason").contains("schema is unreadable"));
    }

    fn legacy_put(home: &Path, relative: &str, value: Value) -> String {
        put(
            home,
            &format!(".local/share/opencode/storage/{relative}"),
            &value,
            false,
        )
    }

    #[test]
    fn legacy_opencode_orders_messages_and_reads_part_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cwd = "/projects/legacy-task";
        let session = |id: &str, directory: &str| {
            legacy_put(
                home,
                &format!("session/project/{id}.json"),
                json!({"id":id,"directory":directory,"title":"Legacy session","time":{"created":1793527200000_i64,"updated":1793527260000_i64}}),
            )
        };
        let file = session("ses_legacy", cwd);
        legacy_put(
            home,
            "message/ses_legacy/a.json",
            json!({"id":"a","sessionID":"ses_legacy","role":"assistant","content":[{"type":"text","text":"finished change"}],"time":{"created":1793527260000_i64}}),
        );
        legacy_put(
            home,
            "message/ses_legacy/z.json",
            json!({"id":"z","sessionID":"ses_legacy","role":"user","content":[{"type":"text","text":"please change this"}],"time":{"created":1793527200000_i64}}),
        );
        session("ses_other", "/other");
        session("ses_parts", cwd);
        legacy_put(
            home,
            "message/ses_parts/msg_user.json",
            json!({"id":"msg_user","sessionID":"ses_parts","role":"user","time":{"created":1793527200000_i64}}),
        );
        legacy_put(
            home,
            "part/msg_user/part_a.json",
            json!({"id":"part_a","messageID":"msg_user","type":"text","text":"real prompt"}),
        );
        legacy_put(
            home,
            "message/ses_parts/msg_summary.json",
            json!({"id":"msg_summary","role":"assistant","summary":{"body":"generated summary"},"time":{"created":1793527260000_i64}}),
        );
        legacy_put(home, "session/project/broken.json", Value::Null);
        let remote = Context {
            host: Some("remote".into()),
            ..context(home, cwd)
        };
        assert_eq!(
            legacy_storage(&remote),
            home.join(".local/share/opencode/storage").to_string_lossy()
        );
        let ctx = context(home, cwd);
        let root = home
            .join(".local/share/opencode/storage")
            .to_string_lossy()
            .to_string();
        let mut sessions: Vec<_> = files(&ctx, &join(&root, &["session"]), ".json")
            .iter()
            .filter_map(|f| read_legacy_session(&ctx, f, &root))
            .collect();
        sessions.sort_by(|a, b| text(&a.summary, "id").cmp(text(&b.summary, "id")));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].summary["sourceRef"], file);
        assert_eq!(sessions[0].summary["firstUserPrompt"], "please change this");
        assert_eq!(
            texts(&sessions[0]),
            vec!["please change this", "finished change"]
        );
        assert_eq!(texts(&sessions[1]), vec!["real prompt"]);
    }
}
