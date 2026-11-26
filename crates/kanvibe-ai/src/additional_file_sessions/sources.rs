//! provider별 기록 위치와 받아들이는 파일 모양. 경로 재정의 환경 변수는 로컬에서만 적용한다.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::ReadScope;
use crate::additional_file_session_formats::session_record;
use crate::host::SessionHost;
use crate::javascript::js_trim;
use crate::node_path;
use crate::providers::AiSessionProvider;

pub(crate) use crate::legacy_opencode_sessions::is_within;

pub(crate) struct FileSource {
    pub provider: AiSessionProvider,
    pub roots: Vec<String>,
    pub suffix: &'static str,
    pub accepts: Option<fn(&str) -> bool>,
}

fn path_parts(file: &str) -> std::str::Split<'_, char> {
    file.split('/')
}

fn has_no_subagents(file: &str) -> bool {
    !path_parts(file).any(|part| part == "subagents")
}

fn is_graph_file(file: &str) -> bool {
    !path_parts(file).any(|part| ["tasks", "artifacts", "subagents"].contains(&part))
}

fn is_session_prefixed(file: &str) -> bool {
    node_path::basename(file).starts_with("session_")
}

fn is_cursor_transcript(file: &str) -> bool {
    path_parts(file).any(|part| part == "agent-transcripts")
}

static ANTIGRAVITY_TRANSCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[/\\]\.system_generated[/\\]logs[/\\]transcript(?:_full)?\.jsonl$")
        .expect("Antigravity 기록 패턴")
});

fn is_antigravity_transcript(file: &str) -> bool {
    ANTIGRAVITY_TRANSCRIPT.is_match(file)
}

fn is_rovo_metadata(file: &str) -> bool {
    node_path::basename(file) == "metadata.json"
}

fn is_grok_summary(file: &str) -> bool {
    node_path::basename(file) == "summary.json"
}

fn is_openclaw_session(file: &str) -> bool {
    path_parts(file).any(|part| part == "sessions")
}

fn is_cline_session(file: &str) -> bool {
    node_path::basename(file) == format!("{}.json", node_path::basename(&node_path::dirname(file)))
}

fn is_kimi_state(file: &str) -> bool {
    node_path::basename(file) == "state.json"
        && node_path::basename(&node_path::dirname(file)).starts_with("session_")
}

fn is_muse_session(file: &str) -> bool {
    node_path::basename(file) == "session.jsonl"
}

static UUID_JSON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.json$")
        .expect("UUID 파일 패턴")
});

fn is_kiro_session(file: &str) -> bool {
    UUID_JSON.is_match(&node_path::basename(file))
}

/// `.pi`·`.omp`를 가리키면 그 아래 `agent/sessions`, `agent`를 가리키면 `sessions`
fn normalize_agent_sessions(directory: &str) -> String {
    let leaf = node_path::basename(directory);
    match leaf.as_str() {
        ".pi" | ".omp" => node_path::join(&[directory, "agent", "sessions"]),
        "agent" => node_path::join(&[directory, "sessions"]),
        _ => directory.to_string(),
    }
}

struct SourcePaths<'a> {
    home: &'a str,
    host: &'a SessionHost,
}

impl SourcePaths<'_> {
    fn root(&self, segments: &[&str]) -> String {
        let mut parts = vec![self.home];
        parts.extend_from_slice(segments);
        node_path::join(&parts)
    }

    /// 로컬에서 절대 경로로 준 재정의만 받는다
    fn overridden(&self, name: &str, fallback: String) -> String {
        self.host
            .variable(name)
            .map(js_trim)
            .filter(|value| !value.is_empty() && node_path::is_absolute(value))
            .map_or(fallback, str::to_string)
    }

    fn under(&self, name: &str, fallback: String, leaf: &[&str]) -> String {
        let base = self.overridden(name, fallback);
        let mut parts = vec![base.as_str()];
        parts.extend_from_slice(leaf);
        node_path::join(&parts)
    }
}

fn source(
    provider: AiSessionProvider,
    roots: Vec<String>,
    suffix: &'static str,
    accepts: Option<fn(&str) -> bool>,
) -> FileSource {
    FileSource {
        provider,
        roots,
        suffix,
        accepts,
    }
}

/// TS `fileSources`. 순서가 목록 결과 순서다
pub(crate) fn file_sources(home: &str, host: &SessionHost) -> Vec<FileSource> {
    let paths = SourcePaths { home, host };
    let mut sources = project_sources(&paths);
    sources.extend(agent_directory_sources(&paths));
    sources.extend(state_directory_sources(&paths));
    sources.extend(data_directory_sources(&paths));
    sources
}

/// 프로젝트별 디렉터리와 pi 계열 그래프 기록
fn project_sources(paths: &SourcePaths<'_>) -> Vec<FileSource> {
    use AiSessionProvider as Provider;
    let pi_root = normalize_agent_sessions(&paths.overridden(
        "PI_CODING_AGENT_DIR",
        paths.root(&[".pi", "agent", "sessions"]),
    ));
    let omp_root = normalize_agent_sessions(&paths.overridden(
        "OMP_CODING_AGENT_DIR",
        paths.root(&[".omp", "agent", "sessions"]),
    ));
    let hermes_root = paths.under("HERMES_HOME", paths.root(&[".hermes"]), &["sessions"]);
    vec![
        source(
            Provider::Qoder,
            vec![paths.root(&[".qoder", "projects"])],
            ".jsonl",
            Some(has_no_subagents),
        ),
        source(
            Provider::Codebuddy,
            vec![paths.root(&[".codebuddy", "projects"])],
            ".jsonl",
            Some(has_no_subagents),
        ),
        source(
            Provider::Hermes,
            vec![hermes_root],
            ".json",
            Some(is_session_prefixed),
        ),
        source(
            Provider::Pi,
            vec![pi_root.clone()],
            ".jsonl",
            Some(is_graph_file),
        ),
        source(
            Provider::Omp,
            vec![omp_root, pi_root],
            ".jsonl",
            Some(is_graph_file),
        ),
    ]
}

/// 에이전트별 세션 디렉터리
fn agent_directory_sources(paths: &SourcePaths<'_>) -> Vec<FileSource> {
    use AiSessionProvider as Provider;
    let coding_agent_sessions = paths.under(
        "PRIME_AGENT_CODING_AGENT_DIR",
        paths.root(&[".prime", "agent"]),
        &["sessions"],
    );
    let prime_agent_root = paths.overridden(
        "PRIME_AGENT_SESSION_DIR",
        paths.overridden(
            "PRIME_AGENT_CODING_AGENT_SESSION_DIR",
            coding_agent_sessions,
        ),
    );
    let antigravity_root = paths.root(&[".gemini", "antigravity-cli", "brain"]);
    let copilot_root = paths.under(
        "COPILOT_HOME",
        paths.root(&[".copilot"]),
        &["session-state"],
    );
    let grok_root = paths.under("GROK_HOME", paths.root(&[".grok"]), &["sessions"]);
    vec![
        source(Provider::PrimeAgent, vec![prime_agent_root], ".jsonl", None),
        source(
            Provider::Cursor,
            vec![paths.root(&[".cursor", "projects"])],
            ".jsonl",
            Some(is_cursor_transcript),
        ),
        source(
            Provider::Antigravity,
            vec![antigravity_root],
            ".jsonl",
            Some(is_antigravity_transcript),
        ),
        source(
            Provider::Rovo,
            vec![paths.root(&[".rovodev", "sessions"])],
            ".json",
            Some(is_rovo_metadata),
        ),
        source(Provider::Copilot, vec![copilot_root], ".jsonl", None),
        source(
            Provider::Grok,
            vec![grok_root],
            ".json",
            Some(is_grok_summary),
        ),
    ]
}

/// 상태 디렉터리 아래 기록
fn state_directory_sources(paths: &SourcePaths<'_>) -> Vec<FileSource> {
    use AiSessionProvider as Provider;
    let openclaw_roots = vec![
        paths.under(
            "OPENCLAW_STATE_DIR",
            paths.root(&[".openclaw"]),
            &["agents"],
        ),
        paths.root(&[".clawdbot", "agents"]),
    ];
    let devin_home = paths.overridden(
        "DEVIN_HOME",
        paths.root(&[".local", "share", "devin", "cli"]),
    );
    let devin_roots = vec![
        node_path::join(&[&devin_home, "transcripts"]),
        node_path::join(&[&devin_home, "agent_logs"]),
    ];
    let droid_roots = vec![
        paths.root(&[".factory", "sessions"]),
        paths.root(&[".factory", "projects"]),
    ];
    vec![
        source(
            Provider::Openclaw,
            openclaw_roots,
            ".jsonl",
            Some(is_openclaw_session),
        ),
        source(Provider::Devin, devin_roots, ".json", None),
        source(Provider::Droid, droid_roots, ".jsonl", None),
    ]
}

/// 데이터 디렉터리와 홈 아래 숨김 디렉터리의 기록
fn data_directory_sources(paths: &SourcePaths<'_>) -> Vec<FileSource> {
    use AiSessionProvider as Provider;
    let cline_root = paths.overridden(
        "CLINE_SESSION_DATA_DIR",
        paths.root(&[".cline", "data", "sessions"]),
    );
    let kimi_root = paths.under("KIMI_CODE_HOME", paths.root(&[".kimi-code"]), &["sessions"]);
    let muse_root = paths.under(
        "XDG_DATA_HOME",
        paths.root(&[".local", "share"]),
        &["muse", "sessions"],
    );
    let jcode_root = paths.under("JCODE_HOME", paths.root(&[".jcode"]), &["sessions"]);
    let kiro_root = paths.under("KIRO_HOME", paths.root(&[".kiro"]), &["sessions", "cli"]);
    vec![
        source(
            Provider::Cline,
            vec![cline_root],
            ".json",
            Some(is_cline_session),
        ),
        source(
            Provider::Kimi,
            vec![kimi_root],
            ".json",
            Some(is_kimi_state),
        ),
        source(
            Provider::Muse,
            vec![muse_root],
            ".jsonl",
            Some(is_muse_session),
        ),
        source(
            Provider::Jcode,
            vec![jcode_root],
            ".json",
            Some(is_session_prefixed),
        ),
        source(
            Provider::Kiro,
            vec![kiro_root],
            ".json",
            Some(is_kiro_session),
        ),
    ]
}

fn is_omp_record(record: &serde_json::Map<String, Value>) -> bool {
    let has_runtime = record.get("runtime").is_some_and(Value::is_object);
    let record_type = record.get("type").and_then(Value::as_str);
    let is_titled =
        record_type == Some("title") && record.get("v").and_then(Value::as_f64) == Some(1.0);
    let is_model_change = record_type == Some("model_change")
        && !record.contains_key("modelId")
        && record.get("model").is_some_and(Value::is_string);
    has_runtime || is_titled || is_model_change
}

/// pi 디렉터리를 OMP와 나눠 쓰므로, 기록을 남긴 쪽이 OMP인지 가려 한쪽에만 올린다
pub(crate) fn matches_graph_writer(
    provider: AiSessionProvider,
    file: &str,
    raw: &Value,
    scope: &ReadScope<'_>,
) -> bool {
    let sources = scope.sources();
    let root_of = |target: AiSessionProvider| {
        sources
            .iter()
            .find(|source| source.provider == target)
            .and_then(|source| source.roots.first())
            .cloned()
            .unwrap_or_default()
    };
    let own_omp_root = root_of(AiSessionProvider::Omp);
    let pi_root = root_of(AiSessionProvider::Pi);
    let is_omp_root = is_within(file, &own_omp_root) && own_omp_root != pi_root;
    let is_omp_writer = is_omp_root
        || raw.as_array().is_some_and(|records| {
            records
                .iter()
                .any(|record| is_omp_record(session_record(record)))
        });
    if provider == AiSessionProvider::Omp {
        is_omp_writer
    } else {
        !is_omp_writer
    }
}
