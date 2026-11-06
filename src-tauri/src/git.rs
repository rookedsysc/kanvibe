use crate::database::{self, text, Database, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

pub fn quote(s: &str) -> String {
    crate::runtime::quote(s)
}
pub fn run(command: &str, host: Option<&str>) -> Result<String> {
    crate::runtime::run_shell(command, host)
}
pub fn run_input(command: &str, host: Option<&str>, input: Option<&[u8]>) -> Result<String> {
    crate::runtime::run_shell_input(command, host, input, Duration::from_secs(30))
}

pub fn git(path: &str, args: &str, host: Option<&str>) -> Result<String> {
    run(&format!("git -C {} {args}", quote(path)), host)
}
pub fn read(path: &str, host: Option<&str>) -> Result<String> {
    if host.is_some() {
        run(&format!("cat -- {}", quote(path)), host)
    } else {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }
}
fn read_optional(path: &str, host: Option<&str>) -> Result<Option<String>> {
    if host.is_none() {
        return match std::fs::read_to_string(path) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        };
    }
    match read(path, host) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            if run(&format!("test ! -e {}", quote(path)), host).is_ok() {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

fn remote_write_command(path: &str, executable: bool) -> Result<String> {
    let parent = Path::new(path)
        .parent()
        .ok_or("부모 경로 없음")?
        .to_string_lossy();
    let mode = if executable {
        "chmod 755 -- \"$temporary\"; "
    } else {
        ""
    };
    Ok(format!("set -e; mkdir -p -- {}; destination={}; if [ -L \"$destination\" ]; then destination=$(realpath \"$destination\"); fi; temporary=$(mktemp \"$destination.kanvibe.XXXXXX\"); trap 'rm -f -- \"$temporary\"' EXIT; if [ -f \"$destination\" ]; then cp -p -- \"$destination\" \"$temporary\"; fi; cat > \"$temporary\"; {mode}mv -f -- \"$temporary\" \"$destination\"",quote(&parent),quote(path)))
}

pub fn write(path: &str, content: &str, host: Option<&str>, executable: bool) -> Result<()> {
    if host.is_some() {
        let command = remote_write_command(path, executable)?;
        run_input(&command, host, Some(content.as_bytes()))?;
    } else {
        let original_path = Path::new(path);
        let resolved = if std::fs::symlink_metadata(original_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            std::fs::canonicalize(original_path).map_err(|e| e.to_string())?
        } else {
            original_path.to_path_buf()
        };
        let p = resolved.as_path();
        let permissions = std::fs::metadata(p).ok().map(|m| m.permissions());
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = p.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, content).map_err(|e| e.to_string())?;
        if let Some(permissions) = permissions {
            std::fs::set_permissions(&temporary, permissions).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&temporary, p).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn join(root: &str, relative: &str) -> String {
    Path::new(root)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}
pub fn safe_path(root: &str, relative: &str, host: Option<&str>) -> Result<String> {
    if Path::new(relative).is_absolute() {
        return Err("잘못된 파일 경로: 절대 경로는 허용되지 않습니다".into());
    }
    let mut clean = PathBuf::new();
    for part in Path::new(relative).components() {
        match part {
            Component::ParentDir => {
                if !clean.pop() {
                    return Err("잘못된 파일 경로: worktree 외부 접근이 감지되었습니다".into());
                }
            }
            Component::Normal(p) => clean.push(p),
            Component::CurDir => {}
            _ => return Err("잘못된 파일 경로".into()),
        }
    }
    let p = Path::new(root).join(clean);
    if host.is_none() {
        let base = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        let mut existing = p.as_path();
        while !existing.exists() {
            existing = existing.parent().ok_or("유효하지 않은 경로")?;
        }
        let resolved = std::fs::canonicalize(existing).map_err(|e| e.to_string())?;
        if !resolved.starts_with(base) {
            return Err("worktree 외부 심볼릭 링크는 허용되지 않습니다".into());
        }
    }
    Ok(p.to_string_lossy().into_owned())
}
pub fn session_name(repo: &str, branch: &str) -> String {
    format!(
        "{}-{branch}",
        Path::new(repo)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    )
    .replace('/', "-")
}
/// Herdr는 세션 이름 규칙이 좁아서 그 세션 타입일 때만 결정적 이름으로 바꾼다.
pub fn session_name_for(session_type: &str, repo: &str, branch: &str) -> String {
    let name = session_name(repo, branch);
    if session_type == "herdr" {
        crate::herdr::sanitize_session_name(&name)
    } else {
        name
    }
}
pub fn worktree_path(repo: &str, branch: &str) -> String {
    format!("{repo}__worktrees/{}", branch.replace('/', "-"))
}
pub fn create_worktree(
    project: &Value,
    branch: &str,
    base: &str,
    session_type: &str,
) -> Result<(String, String)> {
    let path = text(project, "repoPath");
    let host = project["sshHost"].as_str();
    git(
        path,
        &format!("check-ref-format --branch {}", quote(branch)),
        host,
    )?;
    let worktree = worktree_path(path, branch);
    git(
        path,
        &format!(
            "worktree add {} -b {} {}",
            quote(&worktree),
            quote(branch),
            quote(base)
        ),
        host,
    )?;
    Ok((worktree, session_name_for(session_type, path, branch)))
}
pub fn worktrees(project: &Value) -> Result<Vec<Value>> {
    let output = git(
        text(project, "repoPath"),
        "worktree list --porcelain",
        project["sshHost"].as_str(),
    )?;
    let mut entries = Vec::new();
    let mut current = json!({});
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if current.get("path").is_some() {
                entries.push(current.clone());
            }
            current = json!({});
        } else if let Some(p) = line.strip_prefix("worktree ") {
            current["path"] = json!(p);
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            current["branch"] = json!(b);
        } else if line == "bare" {
            current["isBare"] = json!(true);
        }
    }
    Ok(entries)
}
pub fn persist_task(task: &Value, project: Option<&Value>, metadata: bool) -> Result<()> {
    let is_root = project.is_some_and(|p| task["branchName"] == p["defaultBranch"]);
    let path = task["worktreePath"].as_str().or_else(|| {
        if is_root {
            project.and_then(|p| p["repoPath"].as_str())
        } else {
            None
        }
    });
    let Some(path) = path else { return Ok(()) };
    let host = task["sshHost"]
        .as_str()
        .or_else(|| project.and_then(|p| p["sshHost"].as_str()));
    let mut v = if metadata {
        json!({"schemaVersion":1,"description":task["description"],"updatedAt":database::now()})
    } else {
        json!({"schemaVersion":1,"status":task["status"],"updatedAt":database::now()})
    };
    if metadata {
        if is_root {
            if let Some(project) = project {
                let state_path = join(text(project, "repoPath"), ".kanvibe/project.json");
                let mut state: Value =
                    serde_json::from_str(&read(&state_path, host).unwrap_or_default())
                        .unwrap_or(json!({}));
                state["schemaVersion"] = json!(1);
                state["priority"] = task["priority"].clone();
                state["updatedAt"] = json!(database::now());
                write(&state_path, &state.to_string(), host, false)?;
            }
        } else {
            v["priority"] = task["priority"].clone();
        }
    }
    write(
        &join(
            path,
            if metadata {
                ".kanvibe/task.json"
            } else {
                ".kanvibe/status.json"
            },
        ),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        ),
        host,
        false,
    )?;
    ensure_exclude(path, host)
}

pub fn ensure_exclude(path: &str, host: Option<&str>) -> Result<()> {
    const PATTERNS: [&str; 10] = [
        ".claude/hooks/",
        ".claude/settings.json",
        ".gemini/hooks/",
        ".gemini/settings.json",
        ".codex/hooks/",
        ".codex/hooks.json",
        ".codex/config.toml",
        ".opencode/plugins/",
        ".zellij-layout.kdl",
        ".kanvibe/",
    ];
    let common = git(
        path,
        "rev-parse --path-format=absolute --git-common-dir",
        host,
    )?;
    let common = common.trim();
    if common.is_empty() {
        return Err("git common directory가 비어 있습니다".into());
    }
    let common = if Path::new(common).is_absolute() {
        common.to_string()
    } else {
        join(path, common)
    };
    let exclude = join(&common, "info/exclude");
    if host.is_some() {
        let parent = Path::new(&exclude)
            .parent()
            .ok_or("git exclude 부모 경로 없음")?
            .to_string_lossy();
        let mut command = format!(
            "set -e; mkdir -p -- {}; touch -- {};",
            quote(&parent),
            quote(&exclude)
        );
        for pattern in PATTERNS {
            command.push_str(&format!(
                " if ! grep -qxF -- {} {}; then printf '\\n%s\\n' {} >> {}; fi;",
                quote(pattern),
                quote(&exclude),
                quote(pattern),
                quote(&exclude)
            ));
        }
        run(&command, host)?;
    } else {
        use std::io::Write;
        static EXCLUDE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = EXCLUDE_LOCK.lock().map_err(|e| e.to_string())?;
        let content = read_optional(&exclude, None)?.unwrap_or_default();
        let missing = PATTERNS
            .iter()
            .filter(|pattern| !content.lines().any(|line| line == **pattern))
            .copied()
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        if let Some(parent) = Path::new(&exclude).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&exclude)
            .map_err(|e| e.to_string())?;
        let addition = format!("\n# KanVibe shared state\n{}\n", missing.join("\n"));
        file.write_all(addition.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn persist_color(project: &Value) -> Result<()> {
    let path = join(text(project, "repoPath"), ".kanvibe/project.json");
    let host = project["sshHost"].as_str();
    let mut state: Value =
        serde_json::from_str(&read_optional(&path, host)?.unwrap_or_default()).unwrap_or(json!({}));
    state["schemaVersion"] = json!(1);
    state["projectColor"] = project["color"].clone();
    state["updatedAt"] = json!(database::now());
    write(&path, &state.to_string(), host, false)?;
    ensure_exclude(text(project, "repoPath"), host)
}

pub fn sync_project_state(db: &Database, project: &Value) -> Result<Value> {
    let root = text(project, "repoPath");
    let host = project["sshHost"].as_str();
    let path = join(root, ".kanvibe/project.json");
    if read_optional(&path, host)?.is_none() {
        let current = db
            .project(text(project, "id"))?
            .unwrap_or_else(|| project.clone());
        let seed =
            json!({"schemaVersion":1,"projectColor":current["color"],"updatedAt":database::now()});
        if host.is_some() {
            let parent = join(root, ".kanvibe");
            let command = format!("set -e; mkdir -p -- {}; temporary=$(mktemp {}); trap 'rm -f -- \"$temporary\"' EXIT; cat > \"$temporary\"; ln \"$temporary\" {} 2>/dev/null || test -f {}", quote(&parent), quote(&format!("{path}.XXXXXX")), quote(&path), quote(&path));
            run_input(&command, host, Some(seed.to_string().as_bytes()))?;
        } else {
            use std::io::Write;
            std::fs::create_dir_all(join(root, ".kanvibe")).map_err(|e| e.to_string())?;
            let temporary = format!("{path}.{}.tmp", database::id());
            std::fs::File::create(&temporary)
                .and_then(|mut file| file.write_all(seed.to_string().as_bytes()))
                .map_err(|e| e.to_string())?;
            let linked = std::fs::hard_link(&temporary, &path);
            let _ = std::fs::remove_file(&temporary);
            if let Err(error) = linked {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error.to_string());
                }
            }
        }
        ensure_exclude(root, host)?;
    }
    let shared: Value = serde_json::from_str(&read(&path, host)?).unwrap_or(json!({}));
    let paths = db
        .tasks()?
        .into_iter()
        .filter(|task| task["projectId"] == project["id"])
        .filter_map(|task| task["worktreePath"].as_str().map(str::to_string))
        .filter(|path| !path.is_empty() && path != root)
        .collect::<std::collections::HashSet<_>>();
    for root in paths {
        let path = join(&root, ".kanvibe/project.json");
        let mut state: Value =
            serde_json::from_str(&read_optional(&path, host)?.unwrap_or_default())
                .unwrap_or(json!({}));
        let original = state.clone();
        for key in ["projectColor", "priority"] {
            if let Some(value) = shared.get(key) {
                state[key] = value.clone();
            }
        }
        if state != original {
            state["schemaVersion"] = json!(1);
            state["updatedAt"] = json!(database::now());
            write(&path, &state.to_string(), host, false)?;
        }
    }
    Ok(shared)
}

pub fn dispatch(app: &AppHandle, namespace: &str, method: &str, args: Vec<Value>) -> Result<Value> {
    let db = app.state::<Database>();
    let task_id = args.first().and_then(Value::as_str).unwrap_or("");
    let task = db.task(task_id)?.ok_or("태스크를 찾을 수 없습니다")?;
    let project = db.project(text(&task, "projectId"))?;
    let root = task["worktreePath"]
        .as_str()
        .or_else(|| project.as_ref().and_then(|p| p["repoPath"].as_str()))
        .ok_or("no-target-path")?;
    let host = task["sshHost"]
        .as_str()
        .or_else(|| project.as_ref().and_then(|p| p["sshHost"].as_str()));
    if namespace == "editor" {
        let mut command = format!("code {}", quote(root));
        if let Some(host) = host {
            command = format!(
                "code --remote {} {}",
                quote(&format!("ssh-remote+{host}")),
                quote(root)
            );
        }
        return Ok(match run(&command, None) {
            Ok(_) => json!({"ok":true}),
            Err(e) => json!({"ok":false,"error":e}),
        });
    }
    match method {
        "getGitDiffFiles" => {
            let files = match diff_files(&task) {
                Ok(files) => files,
                Err(error) => {
                    eprintln!("git diff 파일 조회 실패: {error}");
                    return Ok(json!([]));
                }
            };
            cache_stats(&db, task_id, &files)?;
            Ok(json!(files))
        }
        "getOriginalFileContent" => {
            let path = args.get(1).and_then(Value::as_str).unwrap_or("");
            safe_path(root, path, host)?;
            Ok(json!(git(
                root,
                &format!(
                    "show {}",
                    quote(&format!(
                        "{}:{path}",
                        task["baseBranch"].as_str().unwrap_or("main")
                    ))
                ),
                host
            )
            .unwrap_or_default()))
        }
        "getFileContent" => {
            let path = safe_path(
                root,
                args.get(1).and_then(Value::as_str).unwrap_or(""),
                host,
            )?;
            Ok(json!(read(&path, host).unwrap_or_default()))
        }
        "saveFileContent" => {
            let result = (|| {
                let path = safe_path(
                    root,
                    args.get(1).and_then(Value::as_str).unwrap_or(""),
                    host,
                )?;
                write(
                    &path,
                    args.get(2).and_then(Value::as_str).unwrap_or(""),
                    host,
                    false,
                )
            })();
            Ok(match result {
                Ok(_) => json!({"success":true}),
                Err(e) => json!({"success":false,"error":e}),
            })
        }
        _ => Err(format!("알 수 없는 서비스 메서드: {namespace}.{method}")),
    }
}
pub fn diff_files(task: &Value) -> Result<Vec<Value>> {
    let root = task["worktreePath"]
        .as_str()
        .ok_or("worktree 경로가 설정되지 않은 태스크입니다")?;
    let host = task["sshHost"].as_str();
    let range = format!(
        "{}...{}",
        task["baseBranch"].as_str().unwrap_or("main"),
        text(task, "branchName")
    );
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    let mut stats = BTreeMap::new();
    if let Ok(output) = git(root, &format!("diff {} --numstat -z", quote(&range)), host) {
        let mut fields = output.split('\0');
        while let Some(field) = fields.next() {
            if field.is_empty() {
                continue;
            }
            let parts = field.splitn(3, '\t').collect::<Vec<_>>();
            if parts.len() < 3 {
                continue;
            }
            let mut path = parts[2].to_string();
            if path.is_empty() {
                let _ = fields.next();
                path = fields.next().unwrap_or("").to_string();
            }
            stats.insert(
                path,
                (
                    parts[0].parse::<u64>().unwrap_or(0),
                    parts[1].parse::<u64>().unwrap_or(0),
                ),
            );
        }
    }
    if let Ok(output) = git(
        root,
        &format!("diff {} --name-status -z", quote(&range)),
        host,
    ) {
        let mut fields = output.split('\0');
        while let Some(status) = fields.next() {
            if status.is_empty() {
                continue;
            }
            let first = fields.next().unwrap_or("");
            let p = if status.starts_with('R') || status.starts_with('C') {
                fields.next().unwrap_or(first)
            } else {
                first
            };
            let counts = stats.get(p).copied().unwrap_or_default();
            files.insert(p.into(),json!({"path":p,"status":status_name(status),"additions":counts.0,"deletions":counts.1}));
        }
    }
    let output = git(root, "status --porcelain -z --untracked-files=all", host)?;
    let mut fields = output.split('\0');
    while let Some(line) = fields.next() {
        if line.len() < 3 {
            continue;
        }
        let xy = &line[..2];
        let p = &line[3..];
        if xy.contains('R') || xy.contains('C') {
            let _ = fields.next();
        }
        if xy.chars().any(|c| "?ADMR".contains(c)) {
            files.entry(p.into()).or_insert(json!({"path":p,"status":if xy=="??"{"added"}else{status_name(xy)},"additions":0,"deletions":0}));
        }
    }
    Ok(files.into_values().collect())
}
fn status_name(s: &str) -> &str {
    if s.contains('A') {
        "added"
    } else if s.contains('D') {
        "deleted"
    } else if s.contains('R') {
        "renamed"
    } else {
        "modified"
    }
}
pub fn cache_stats(db: &Database, task_id: &str, files: &[Value]) -> Result<()> {
    let additions: u64 = files.iter().filter_map(|f| f["additions"].as_u64()).sum();
    let deletions: u64 = files.iter().filter_map(|f| f["deletions"].as_u64()).sum();
    db.lock()?.execute("INSERT INTO task_diff_stats(task_id,file_count,additions,deletions,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(task_id) DO UPDATE SET file_count=excluded.file_count,additions=excluded.additions,deletions=excluded.deletions,updated_at=excluded.updated_at",rusqlite::params![task_id,files.len() as i64,additions as i64,deletions as i64,database::now()]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn diff_stats(app: &AppHandle, args: Vec<Value>) -> Result<Value> {
    let db = app.state::<Database>();
    let ids = args
        .first()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    std::thread::scope(|scope| {
        for id in &ids {
            let db = &*db;
            scope.spawn(move || {
                if let Some(task) = id.as_str().and_then(|id| db.task(id).ok().flatten()) {
                    if let Ok(files) = diff_files(&task) {
                        let _ = cache_stats(db, text(&task, "id"), &files);
                    }
                }
            });
        }
    });
    let c = db.lock()?;
    let mut s = c
        .prepare("SELECT task_id,file_count,additions,deletions FROM task_diff_stats")
        .map_err(|e| e.to_string())?;
    let rows=s.query_map([],|r|Ok((r.get::<_,String>(0)?,json!({"fileCount":r.get::<_,i64>(1)?,"additions":r.get::<_,i64>(2)?,"deletions":r.get::<_,i64>(3)?})))).map_err(|e|e.to_string())?;
    let mut map = serde_json::Map::new();
    for row in rows {
        let (k, v) = row.map_err(|e| e.to_string())?;
        map.insert(k, v);
    }
    Ok(Value::Object(map))
}

fn toml_section(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}
fn upsert_codex_config(original: &str) -> String {
    let normalized = original.replace("\r\n", "\n");
    let mut lines = normalized
        .lines()
        .filter(|l| {
            !l.trim().starts_with("notify") || !l.contains(".codex/hooks/kanvibe-notify-hook.sh")
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(start) = lines.iter().position(|l| toml_section(l) == "[features]") {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, l)| toml_section(l).starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(lines.len());
        let mut body = lines[start + 1..end]
            .iter()
            .filter(|line| {
                !line
                    .split('=')
                    .next()
                    .is_some_and(|key| ["hooks", "codex_hook", "codex_hooks"].contains(&key.trim()))
            })
            .cloned()
            .collect::<Vec<_>>();
        body.push("hooks = true".into());
        lines.splice(start + 1..end, body);
    } else {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("[features]".into());
        lines.push("hooks = true".into());
    }
    format!("{}\n", lines.join("\n").trim_end())
}
fn codex_hooks_enabled(config: &str) -> bool {
    let mut features = false;
    for line in config.lines() {
        let line = toml_section(line);
        if line.starts_with('[') {
            features = line == "[features]";
        } else if features {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "hooks" && value.trim() == "true" {
                    return true;
                }
            }
        }
    }
    false
}

fn hook_template(provider: &str, kind: &str) -> &'static str {
    match (provider, kind) {
        ("claude", "prompt") => {
            include_str!("../resources/hooks/claude-generatePromptHookScript.sh")
        }
        ("claude", "stop") => include_str!("../resources/hooks/claude-generateStopHookScript.sh"),
        ("claude", "question") => {
            include_str!("../resources/hooks/claude-generateQuestionHookScript.sh")
        }
        ("gemini", "prompt") => {
            include_str!("../resources/hooks/gemini-generatePromptHookScript.sh")
        }
        ("gemini", "stop") => include_str!("../resources/hooks/gemini-generateStopHookScript.sh"),
        ("codex", "prompt") => include_str!("../resources/hooks/codex-generatePromptHookScript.sh"),
        ("codex", "stop") => include_str!("../resources/hooks/codex-generateStopHookScript.sh"),
        ("codex", "permission") => {
            include_str!("../resources/hooks/codex-generatePermissionHookScript.sh")
        }
        ("codex", "pre-tool") => {
            include_str!("../resources/hooks/codex-generatePreToolHookScript.sh")
        }
        _ => "",
    }
}
fn provider_events(provider: &str) -> Vec<(&'static str, &'static str, Option<&'static str>)> {
    match provider {
        "gemini" => vec![
            ("BeforeAgent", "prompt", Some("*")),
            ("AfterAgent", "stop", Some("*")),
        ],
        "codex" => vec![
            ("UserPromptSubmit", "prompt", None),
            ("PermissionRequest", "permission", Some("Bash")),
            ("PreToolUse", "pre-tool", Some("Bash")),
            ("Stop", "stop", None),
        ],
        _ => vec![
            ("UserPromptSubmit", "prompt", None),
            ("PreToolUse", "question", Some("AskUserQuestion")),
            ("PostToolUse", "prompt", Some("AskUserQuestion")),
            ("Stop", "stop", None),
        ],
    }
}
pub fn hook_url(host: Option<&str>) -> Result<String> {
    crate::server::hook_url(host)
}
fn opencode_plugin_path(path: &str, extension: &str) -> String {
    join(
        path,
        &format!(".opencode/plugins/kanvibe-plugin.{extension}"),
    )
}
fn is_managed_opencode_plugin(source: &str) -> bool {
    source.contains("KanVibe OpenCode Plugin")
        && source.contains("export const KanvibePlugin")
        && source.contains("KANVIBE_TARGETS_FILE")
}
fn install_opencode_plugin(path: &str, task_id: &str, host: Option<&str>, url: &str) -> Result<()> {
    let source = include_str!("../resources/hooks/opencode-plugin.txt")
        .replace("http://KANVIBE_HOOK_HOST:19736", url)
        .replace("KANVIBE_TASK_ID", task_id);
    write(&opencode_plugin_path(path, "js"), &source, host, false)?;
    let legacy = opencode_plugin_path(path, "ts");
    if read_optional(&legacy, host)?
        .as_deref()
        .is_some_and(is_managed_opencode_plugin)
    {
        if host.is_some() {
            run(&format!("rm -f -- {}", quote(&legacy)), host)?;
        } else {
            std::fs::remove_file(&legacy).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn read_opencode_plugin(path: &str, host: Option<&str>) -> Result<(String, String)> {
    let preferred = opencode_plugin_path(path, "js");
    if let Some(source) = read_optional(&preferred, host)? {
        return Ok((preferred, source));
    }
    let legacy = opencode_plugin_path(path, "ts");
    Ok((
        legacy.clone(),
        read_optional(&legacy, host)?.unwrap_or_default(),
    ))
}
fn is_opencode_plugin_reference(reference: &str) -> bool {
    reference.ends_with("/kanvibe-plugin.js") || reference.ends_with("/kanvibe-plugin.ts")
}
pub fn install_hooks(
    path: &str,
    task_id: &str,
    host: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let url = hook_url(host)?;
    ensure_exclude(path, host)?;
    let targets_path = join(path, ".kanvibe/targets.json");
    let mut state: Value =
        serde_json::from_str(&read_optional(&targets_path, host)?.unwrap_or_default())
            .unwrap_or(json!({"schemaVersion":1,"targets":[]}));
    let entries = state["targets"].as_array().cloned().unwrap_or_default();
    let mut entries = entries
        .into_iter()
        .filter(|v| text(v, "url") != url)
        .collect::<Vec<_>>();
    entries.push(json!({"url":url,"taskId":task_id}));
    state["targets"] = json!(entries);
    state["updatedAt"] = json!(database::now());
    write(&targets_path, &state.to_string(), host, false)?;
    for provider in provider
        .map(|s| vec![s])
        .unwrap_or_else(|| vec!["claude", "gemini", "codex", "opencode"])
    {
        if provider == "opencode" {
            install_opencode_plugin(path, task_id, host, &url)?;
            continue;
        }
        let settings_path = join(
            path,
            &format!(
                ".{provider}/{}",
                if provider == "codex" {
                    "hooks.json"
                } else {
                    "settings.json"
                }
            ),
        );
        let original = read_optional(&settings_path, host)?.unwrap_or_default();
        let mut settings: Value = if original.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&original)
                .map_err(|e| format!("기존 {settings_path} JSON을 보존할 수 없습니다: {e}"))?
        };
        if !settings.is_object() {
            return Err(format!("{settings_path}: JSON 객체가 필요합니다"));
        }
        if !settings["hooks"].is_object() {
            settings["hooks"] = json!({});
        }
        for (event, kind, matcher) in provider_events(provider) {
            let filename = format!("kanvibe-{kind}-hook.sh");
            let source = hook_template(provider, kind)
                .replace("http://KANVIBE_HOOK_HOST:19736", &url)
                .replace("KANVIBE_TASK_ID", task_id);
            write(
                &join(path, &format!(".{provider}/hooks/{filename}")),
                &source,
                host,
                true,
            )?;
            let command = if provider == "codex" {
                format!("bash \"$(git rev-parse --show-toplevel)/.codex/hooks/{filename}\"")
            } else {
                format!(
                    "\"${}_PROJECT_DIR\"/.{provider}/hooks/{filename}",
                    provider.to_uppercase()
                )
            };
            let mut entry = json!({"hooks":[{"type":"command","command":command,"timeout":if provider=="gemini"{10000}else{10}}]});
            if let Some(matcher) = matcher {
                entry["matcher"] = json!(matcher);
            }
            let mut entries = settings["hooks"][event]
                .as_array()
                .cloned()
                .unwrap_or_default();
            entries.retain(|v| !v.to_string().contains(&filename));
            entries.push(entry);
            settings["hooks"][event] = json!(entries);
        }
        write(
            &settings_path,
            &format!(
                "{}\n",
                serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?
            ),
            host,
            false,
        )?;
        if provider == "codex" {
            let config_path = join(path, ".codex/config.toml");
            let original = read_optional(&config_path, host)?.unwrap_or_default();
            write(&config_path, &upsert_codex_config(&original), host, false)?;
        }
    }
    Ok(())
}
pub fn hooks_status(
    path: &str,
    task_id: &str,
    host: Option<&str>,
    provider: &str,
) -> Result<Value> {
    let url = hook_url(host)?;
    let targets: Value =
        serde_json::from_str(&read(&join(path, ".kanvibe/targets.json"), host).unwrap_or_default())
            .unwrap_or(Value::Null);
    let registered = targets["targets"].as_array().is_some_and(|a| {
        a.iter()
            .any(|t| text(t, "url") == url && text(t, "taskId") == task_id)
    });
    let mut state = json!({"installed":false,"hasTaskIdBinding":false,"hasStatusMappings":false,"hasStatusJsonPersistence":false,"hasTargetFanout":false,"hasBoundedNotifyTimeout":false,"hasParallelTargetFanout":false,"hasExpectedHookServerUrl":false,"hasReachableHookServer":false,"boundTaskId":null,"configuredHookServerUrl":null,"expectedHookServerUrl":url,"hasTargetsFile":!targets.is_null(),"hasCurrentTarget":registered,"hasTargetRegistration":registered,"isCurrentTargetRegistered":registered});
    state["hasRegisteredHookTarget"] = json!(registered);
    state["registeredHookTargetUrl"] = if registered { json!(url) } else { Value::Null };

    let (plugin_path, source) = if provider == "opencode" {
        read_opencode_plugin(path, host)
            .unwrap_or_else(|_| (opencode_plugin_path(path, "js"), String::new()))
    } else {
        (
            String::new(),
            read(
                &join(path, &format!(".{provider}/hooks/kanvibe-prompt-hook.sh")),
                host,
            )
            .unwrap_or_default(),
        )
    };
    let bound_id = regex::Regex::new(r#"(?:const )?TASK_ID\s*=\s*"([^"\n]+)""#)
        .ok()
        .and_then(|pattern| {
            pattern
                .captures(&source)
                .and_then(|c| c.get(1).map(|s| s.as_str().to_string()))
        });
    let bound = bound_id.is_some() && source.contains("taskId") && source.contains("TASK_ID");
    let expected = source.contains(&url);
    let persist = source.contains("status.json");
    let fanout = source.contains("targets.json");
    let bounded = source.contains("--max-time") || source.contains("KANVIBE_NOTIFY_TIMEOUT_MS");
    let parallel = source.contains("wait") || source.contains("Promise.all");
    for (k, v) in [
        ("hasTaskIdBinding", bound),
        ("hasStatusMappings", source.contains("progress")),
        ("hasStatusJsonPersistence", persist),
        ("hasTargetFanout", fanout),
        ("hasBoundedNotifyTimeout", bounded),
        ("hasParallelTargetFanout", parallel),
        ("hasExpectedHookServerUrl", expected),
    ] {
        state[k] = json!(v);
    }
    state["boundTaskId"] = json!(bound_id);
    state["configuredHookServerUrl"] = if expected { json!(url) } else { Value::Null };
    let reachable = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("{url}/api/hooks/health"))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    state["hasReachableHookServer"] = json!(reachable);
    let mut all = !source.is_empty();
    if provider == "opencode" {
        state["hasPluginFile"] = json!(all);
        state["hasPlugin"] = json!(all);
        state["hasStatusEndpoint"] = json!(source.contains("/api/hooks/status"));
        state["hasEventMappings"] = json!([
            "message.updated",
            "question.asked",
            "question.replied",
            "session.idle",
            "session.deleted"
        ]
        .iter()
        .all(|s| source.contains(s)));
        state["hasMainSessionGuard"] = json!(source.contains("isMainSession(message)"));
        state["hasDuplicateProgressGuard"] = json!(source.contains("lastUserMessageBySession"));
        state["targetPath"] = json!(path);
        state["pluginPath"] = json!(plugin_path);
        let expected_plugin = url::Url::from_file_path(&plugin_path)
            .ok()
            .map(|u| u.to_string())
            .unwrap_or_default();
        let plugin_config = if host.is_some() {
            None
        } else {
            run(
                &format!("cd {} && opencode debug config", quote(path)),
                None,
            )
            .ok()
        };
        let registered_plugins = plugin_config
            .as_deref()
            .and_then(|s| {
                serde_json::from_str::<Value>(s)
                    .ok()
                    .and_then(|v| v["plugin"].as_array().cloned())
                    .or_else(|| {
                        regex::Regex::new(r#""plugin"\s*:\s*(\[[\s\S]*?\])"#)
                            .ok()?
                            .captures(s)
                            .and_then(|c| {
                                serde_json::from_str::<Vec<Value>>(c.get(1)?.as_str()).ok()
                            })
                    })
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .filter(|s| is_opencode_plugin_reference(s))
            .collect::<Vec<_>>();
        let plugin_registered = host.is_some() || registered_plugins.contains(&expected_plugin);
        state["hasRegisteredPlugin"] = json!(plugin_registered);
        state["hasDuplicateKanvibePlugins"] = json!(registered_plugins.len() > 1);
        state["registeredPluginUrls"] = json!(registered_plugins);
        all &= plugin_registered;

        state["hasPluginEntry"] = json!(source.contains("KanvibePlugin"));
    } else {
        let settings_path = join(
            path,
            &format!(
                ".{provider}/{}",
                if provider == "codex" {
                    "hooks.json"
                } else {
                    "settings.json"
                }
            ),
        );
        let settings = read(&settings_path, host).unwrap_or_default();
        let mut entries = true;
        for (_, kind, _) in provider_events(provider) {
            let filename = format!("kanvibe-{kind}-hook.sh");
            let exists = !read(&join(path, &format!(".{provider}/hooks/{filename}")), host)
                .unwrap_or_default()
                .is_empty();
            all &= exists;
            entries &= settings.contains(&filename);
            state[match kind {
                "prompt" => "hasPromptHook",
                "stop" => "hasStopHook",
                "question" => "hasQuestionHook",
                "permission" => "hasPermissionHook",
                _ => "hasPreToolHook",
            }] = json!(exists);
        }
        state["hasSettingsEntry"] = json!(entries);
        state["hasHooksFile"] = json!(!settings.is_empty());
        state["hasHookEntries"] = json!(entries);
        if provider == "codex" {
            let config = read(&join(path, ".codex/config.toml"), host).unwrap_or_default();
            let enabled = codex_hooks_enabled(&config);
            state["hasConfigEntry"] = json!(enabled);
            all &= enabled;
        }
        all &= entries;
    }
    state["installed"] =
        json!(all && bound && persist && fanout && bounded && parallel && registered);
    Ok(state)
}

pub fn project_icon(project: &Value) -> Option<String> {
    use base64::Engine;
    use std::sync::{Mutex, OnceLock};
    type CachedOwnerIcons = BTreeMap<String, (Instant, Option<String>)>;
    static CACHE: OnceLock<Mutex<CachedOwnerIcons>> = OnceLock::new();
    let remote = git(
        text(project, "repoPath"),
        "config --get remote.origin.url",
        project["sshHost"].as_str(),
    )
    .ok()?;
    let regex=regex::Regex::new(r"(?i)^(?:https?://|ssh://)?(?:[^@/]+@)?github\.com[:/]+([A-Za-z0-9][A-Za-z0-9-]{0,38})/[^/]+(?:/)?$").ok()?;
    let capture = regex.captures(remote.trim())?;
    let owner = capture.get(1)?.as_str();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((timestamp, value)) = cache.lock().ok()?.get(owner) {
        if timestamp.elapsed() < Duration::from_secs(600) {
            return value.clone();
        }
    }
    let download = (|| {
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok()?
            .get(format!("https://github.com/{owner}.png?size=64"))
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let content_type = response
            .headers()
            .get("content-type")?
            .to_str()
            .ok()?
            .split(';')
            .next()?
            .trim()
            .to_string();
        if !["image/png", "image/jpeg", "image/gif", "image/webp"].contains(&content_type.as_str())
        {
            return None;
        }
        if response.content_length().is_some_and(|n| n > 512 * 1024) {
            return None;
        }
        let mut bytes = Vec::new();
        response.take(512 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.is_empty() || bytes.len() > 512 * 1024 {
            return None;
        }
        Some(format!(
            "data:{content_type};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    })();
    cache
        .lock()
        .ok()?
        .insert(owner.into(), (Instant::now(), download.clone()));
    download
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_paths_reject_parent_and_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        assert!(safe_path(root.to_str().unwrap(), "../secret", None).is_err());
        assert!(safe_path(root.to_str().unwrap(), "/tmp/secret", None).is_err());
        assert!(safe_path(root.to_str().unwrap(), "nested/file.txt", None).is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), root.join("outside")).unwrap();
            assert!(safe_path(root.to_str().unwrap(), "outside/file.txt", None).is_err());
        }
    }
    #[test]
    fn shell_arguments_preserve_quotes_and_substitutions() {
        let value = "a' $(printf dangerous) `echo danger`\n";
        let output = run(&format!("printf %s {}", quote(value)), None).unwrap();
        assert_eq!(output, value);
    }
    #[test]
    fn local_file_writes_are_atomic_and_preserve_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b");
        write(path.to_str().unwrap(), "한글\n'\"$", None, false).unwrap();
        assert_eq!(read(path.to_str().unwrap(), None).unwrap(), "한글\n'\"$");
    }
    #[test]
    fn git_diff_includes_committed_and_untracked_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        git(root, "init -b main", None).unwrap();
        git(root, "config user.name QA", None).unwrap();
        git(root, "config user.email qa@example.test", None).unwrap();
        std::fs::write(dir.path().join("file.txt"), "one\n").unwrap();
        git(root, "add .", None).unwrap();
        git(root, "commit -m initial", None).unwrap();
        git(root, "checkout -b feature", None).unwrap();
        std::fs::write(dir.path().join("file.txt"), "one\ntwo\n").unwrap();
        git(root, "add .", None).unwrap();
        git(root, "commit -m change", None).unwrap();
        std::fs::write(dir.path().join("한글.txt"), "new\n").unwrap();
        let files =
            diff_files(&json!({"worktreePath":root,"branchName":"feature","baseBranch":"main"}))
                .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files
                .iter()
                .find(|v| text(v, "path") == "file.txt")
                .unwrap()["additions"],
            1
        );
        assert_eq!(
            files
                .iter()
                .find(|v| text(v, "path") == "한글.txt")
                .unwrap()["status"],
            "added"
        );
    }
}

#[cfg(test)]
mod persistence_regression_tests {
    use super::*;
    #[test]
    fn shared_project_state_seeds_current_color_and_propagates_root_authority() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let root = root.to_str().unwrap();
        git(root, "init -b main", None).unwrap();
        let db = Database::open(&dir.path().join("app.db")).unwrap();
        let project = db
            .save_project(
                &json!({"repoPath":root,"name":"repo","color":"#0064FF","defaultBranch":"main"}),
            )
            .unwrap();
        let mut current = project.clone();
        current["color"] = json!("#112233");
        db.save_project(&current).unwrap();
        let worktree = dir.path().join("feature").to_string_lossy().to_string();
        let task = db.save_task(&json!({"projectId":project["id"],"branchName":"feature","worktreePath":worktree,"priority":"low"})).unwrap();
        write(
            &join(&worktree, ".kanvibe/task.json"),
            r#"{"priority":"low","description":"task-specific"}"#,
            None,
            false,
        )
        .unwrap();
        let shared = sync_project_state(&db, &project).unwrap();
        assert_eq!(shared["projectColor"], "#112233");
        let feature_state = || {
            serde_json::from_str::<Value>(
                &read(&join(&worktree, ".kanvibe/project.json"), None).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(feature_state()["projectColor"], "#112233");
        persist_color(&project).unwrap();
        let root_task = json!({"branchName":"main","worktreePath":root,"priority":"high"});
        persist_task(&root_task, Some(&project), true).unwrap();
        let root_before = read(&join(root, ".kanvibe/project.json"), None).unwrap();
        sync_project_state(&db, &current).unwrap();
        assert_eq!(feature_state()["projectColor"], "#0064FF");
        assert_eq!(feature_state()["priority"], "high");
        assert_eq!(
            db.task(text(&task, "id")).unwrap().unwrap()["priority"],
            "low"
        );
        let task_metadata: Value =
            serde_json::from_str(&read(&join(&worktree, ".kanvibe/task.json"), None).unwrap())
                .unwrap();
        assert_eq!(task_metadata["priority"], "low");
        assert_eq!(task_metadata["description"], "task-specific");
        assert_eq!(
            read(&join(root, ".kanvibe/project.json"), None).unwrap(),
            root_before
        );
        let before = read(&join(&worktree, ".kanvibe/project.json"), None).unwrap();
        sync_project_state(&db, &project).unwrap();
        assert_eq!(
            read(&join(&worktree, ".kanvibe/project.json"), None).unwrap(),
            before
        );
        let new_path = dir.path().join("later").to_string_lossy().to_string();
        db.save_task(
            &json!({"projectId":project["id"],"branchName":"later","worktreePath":new_path}),
        )
        .unwrap();
        sync_project_state(&db, &project).unwrap();
        let later: Value =
            serde_json::from_str(&read(&join(&new_path, ".kanvibe/project.json"), None).unwrap())
                .unwrap();
        assert_eq!(later["projectColor"], "#0064FF");
        assert_eq!(later["priority"], "high");
    }
    #[test]
    fn metadata_root_priority_is_separate_from_description_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        git(root, "init -b main", None).unwrap();
        let p = json!({"repoPath":root,"defaultBranch":"main","color":"#0064FF"});
        persist_color(&p).unwrap();
        let task = json!({"id":"t","branchName":"main","status":"progress","description":"설명","priority":"high","worktreePath":root});
        persist_task(&task, Some(&p), true).unwrap();
        let metadata: Value =
            serde_json::from_str(&read(&join(root, ".kanvibe/task.json"), None).unwrap()).unwrap();
        let project: Value =
            serde_json::from_str(&read(&join(root, ".kanvibe/project.json"), None).unwrap())
                .unwrap();
        assert_eq!(metadata["description"], "설명");
        assert!(metadata.get("priority").is_none());
        assert_eq!(project["priority"], "high");
        assert_eq!(project["projectColor"], "#0064FF");
    }
    #[test]
    fn unrelated_task_without_worktree_does_not_overwrite_root_state() {
        let dir = tempfile::tempdir().unwrap();
        let task = json!({"id":"t","title":"note","status":"done","projectId":"p"});
        let project =
            json!({"id":"p","repoPath":dir.path().to_str().unwrap(),"defaultBranch":"main"});
        persist_task(&task, Some(&project), false).unwrap();
        assert!(!dir.path().join(".kanvibe/status.json").exists());
    }
    #[cfg(unix)]
    #[test]
    fn replacing_file_preserves_owner_only_permissions_and_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret");
        std::fs::write(&p, "old").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link");
        symlink(&p, &link).unwrap();
        write(link.to_str().unwrap(), "new", None, false).unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new");
        assert_eq!(
            std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(test)]
mod codex_config_regression_tests {
    use super::*;
    #[test]
    fn migrates_obsolete_hook_flags_without_losing_other_settings() {
        let input="model = \"codex\"\nnotify = [\".codex/hooks/kanvibe-notify-hook.sh\"]\n[features] # personal settings\ncodex_hook = false\ncodex_hooks = true\nhooks = false\nweb_search = true\n[mcp_servers.example]\ncommand = \"tool\"\n";
        let output = upsert_codex_config(input);
        assert!(output.contains("model = \"codex\""));
        assert!(output.contains("web_search = true"));
        assert!(output.contains("[mcp_servers.example]\ncommand = \"tool\""));
        assert!(!output.contains("codex_hook"));
        assert!(!output.contains("notify ="));
        assert_eq!(output.matches("[features]").count(), 1);
        assert!(codex_hooks_enabled(&output));
        assert_eq!(upsert_codex_config(&output), output);
    }
    #[test]
    fn feature_flag_is_scoped_to_features_section() {
        assert!(!codex_hooks_enabled("[other]\nhooks = true\n"));
        assert!(codex_hooks_enabled(
            "[features]\nhooks=true # enabled\n[other]\nhooks=false\n"
        ));
    }
}

#[cfg(test)]
mod remote_write_contract_tests {
    use super::*;
    #[test]
    fn ssh_write_command_handles_quotes_spaces_and_stdin_without_expansion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder ' name/file $ literal.txt");
        let command = remote_write_command(path.to_str().unwrap(), false).unwrap();
        run_input(&command, None, Some("한글 $(echo unsafe)\n'\"".as_bytes())).unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "한글 $(echo unsafe)\n'\""
        );
    }
}

#[cfg(test)]
mod exclude_regression_tests {
    use super::*;
    #[test]
    fn exclude_is_append_only_idempotent_and_uses_target_repo_outside_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("target repo");
        std::fs::create_dir(&root).unwrap();
        let root = root.to_str().unwrap();
        git(root, "init -b main", None).unwrap();
        let target_exclude = join(root, ".git/info/exclude");
        let original = "# personal exclusions\n/local-only/\n/.qa-loop/\n";
        std::fs::write(&target_exclude, original).unwrap();
        let current = std::env::current_dir().unwrap();
        let caller_common = git(
            current.to_str().unwrap(),
            "rev-parse --path-format=absolute --git-common-dir",
            None,
        )
        .unwrap();
        let caller_exclude = join(caller_common.trim(), "info/exclude");
        let caller_before = std::fs::read(&caller_exclude).unwrap_or_default();
        ensure_exclude(root, None).unwrap();
        let first = std::fs::read_to_string(&target_exclude).unwrap();
        assert!(first.starts_with(original));
        assert_eq!(first.lines().filter(|l| *l == ".kanvibe/").count(), 1);
        assert!(first.lines().any(|l| l == ".codex/hooks.json"));
        ensure_exclude(root, None).unwrap();
        assert_eq!(std::fs::read_to_string(&target_exclude).unwrap(), first);
        assert_eq!(
            std::fs::read(&caller_exclude).unwrap_or_default(),
            caller_before
        );
    }
}

#[cfg(test)]
mod opencode_plugin_migration_tests {
    use super::*;
    #[test]
    fn installs_javascript_plugin_with_unchanged_hooks_and_target_fanout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        install_opencode_plugin(root, "task-1", None, "http://127.0.0.1:12345").unwrap();
        let (path, source) = read_opencode_plugin(root, None).unwrap();
        assert!(path.ends_with("/kanvibe-plugin.js"));
        assert!(!Path::new(&opencode_plugin_path(root, "ts")).exists());
        assert!(source.contains("http://127.0.0.1:12345"));
        assert!(source.contains("const TASK_ID = \"task-1\""));
        assert!(is_managed_opencode_plugin(&source));
        for expected in [
            "message.updated",
            "question.asked",
            "question.replied",
            "session.idle",
            "session.deleted",
            "targets.json",
            "Promise.all",
            "KANVIBE_NOTIFY_TIMEOUT_MS",
            "isMainSession(message)",
            "lastUserMessageBySession",
        ] {
            assert!(
                source.contains(expected),
                "missing OpenCode behavior: {expected}"
            );
        }
        assert!(!source.contains("import type"));
        assert!(!source.contains("new Map<string"));
    }
    #[test]
    fn recognizes_legacy_plugin_and_migrates_without_duplicate_managed_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let legacy = opencode_plugin_path(root, "ts");
        let old="/* KanVibe OpenCode Plugin */\nexport const KanvibePlugin: Plugin = async () => { const KANVIBE_TARGETS_FILE = 'targets.json'; };";
        write(&legacy, old, None, false).unwrap();
        assert_eq!(
            read_opencode_plugin(root, None).unwrap(),
            (legacy.clone(), old.to_owned())
        );
        install_opencode_plugin(root, "task-2", None, "http://127.0.0.1:12345").unwrap();
        assert!(!Path::new(&legacy).exists());
        install_opencode_plugin(root, "task-2", None, "http://127.0.0.1:12345").unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join(".opencode/plugins"))
                .unwrap()
                .count(),
            1
        );
        assert!(read_opencode_plugin(root, None).unwrap().0.ends_with(".js"));
    }
    #[test]
    fn javascript_is_preferred_and_unrelated_legacy_typescript_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let legacy = opencode_plugin_path(root, "ts");
        let custom =
            "// My personal OpenCode plugin\nexport const CustomPlugin = async () => ({});";
        write(&legacy, custom, None, false).unwrap();
        install_opencode_plugin(root, "task-3", None, "http://127.0.0.1:12345").unwrap();
        assert_eq!(read(&legacy, None).unwrap(), custom);
        assert!(read_opencode_plugin(root, None)
            .unwrap()
            .0
            .ends_with("/kanvibe-plugin.js"));
        assert!(is_opencode_plugin_reference(
            "file:///tmp/a/kanvibe-plugin.js"
        ));
        assert!(is_opencode_plugin_reference(
            "file:///tmp/a/kanvibe-plugin.ts"
        ));
        assert!(!is_opencode_plugin_reference(
            "file:///tmp/a/personal-plugin.js"
        ));
    }
}
