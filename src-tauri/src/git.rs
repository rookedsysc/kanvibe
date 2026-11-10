use crate::database::{self, text, Database, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Condvar, Mutex, OnceLock, PoisonError},
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
    run(
        &format!("git -C {} {args}", resolve_path_for_shell(path, host)),
        host,
    )
}
/// 경로를 셸 인자로 바꾼다. 앞의 `~`만 홈으로 확장하고 나머지 문자는 어떤 것이든 리터럴로 둔다.
pub fn resolve_path_for_shell(path: &str, host: Option<&str>) -> String {
    let Some(suffix) = path.strip_prefix('~') else {
        return quote(path);
    };
    if host.is_some() {
        format!("\"$HOME\"{}", quote(suffix))
    } else {
        quote(&format!(
            "{}{suffix}",
            dirs::home_dir().unwrap_or_default().to_string_lossy()
        ))
    }
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

fn remote_write_command(
    path: &str,
    executable: bool,
    expected_size: Option<usize>,
) -> Result<String> {
    let parent = Path::new(path)
        .parent()
        .ok_or("부모 경로 없음")?
        .to_string_lossy();
    let mode = if executable {
        "chmod 755 \"$temporary\"; "
    } else if private_state_file(path) {
        "chmod 600 \"$temporary\"; "
    } else {
        ""
    };
    let preserve_mode = if private_state_file(path) {
        ""
    } else {
        "if [ -f \"$destination\" ]; then cp -p -- \"$destination\" \"$temporary\"; fi; "
    };
    let size_check = expected_size
        .map(|size| format!("test \"$(wc -c < \"$temporary\" | tr -d ' ')\" = {size}; "))
        .unwrap_or_default();
    Ok(format!("set -e; umask 077; mkdir -p -- {}; destination={}; if [ -L \"$destination\" ]; then destination=$(realpath \"$destination\"); fi; temporary=$(mktemp \"$destination.kanvibe.XXXXXX\"); trap 'rm -f -- \"$temporary\"' EXIT; {preserve_mode}{mode}cat > \"$temporary\"; {size_check}mv -f -- \"$temporary\" \"$destination\"",quote(&parent),quote(path)))
}

fn private_state_file(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .is_some_and(|name| name == "targets.json")
        && Path::new(path)
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".kanvibe")
}
fn state_path_components(path: &str) -> Vec<PathBuf> {
    let mut current = PathBuf::new();
    let mut in_state = false;
    let mut guarded = Vec::new();
    for part in Path::new(path).components() {
        current.push(part);
        if part.as_os_str() == ".kanvibe" {
            in_state = true;
        }
        if in_state {
            guarded.push(current.clone());
        }
    }
    guarded
}
fn state_guard_command(path: &str) -> Option<String> {
    let paths = state_path_components(path);
    if paths.is_empty() {
        return None;
    }
    Some(format!("for kanvibe_state_path in {}; do if test -L \"$kanvibe_state_path\"; then printf '%s\\n' '심볼릭 링크인 KanVibe 상태 경로에는 기록할 수 없습니다' >&2; exit 1; fi; done",paths.iter().map(|path|quote(&path.to_string_lossy())).collect::<Vec<_>>().join(" ")))
}
fn assert_state_path(path: &str, host: Option<&str>) -> Result<()> {
    if host.is_some() {
        if let Some(command) = state_guard_command(path) {
            run(&command, host)?;
        }
    } else {
        for path in state_path_components(path) {
            if std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(format!(
                    "심볼릭 링크인 KanVibe 상태 경로에는 기록할 수 없습니다: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}
pub fn write(path: &str, content: &str, host: Option<&str>, executable: bool) -> Result<()> {
    assert_state_path(path, host)?;
    if host.is_some() {
        let command = remote_write_command(path, executable, Some(content.len()))?;
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
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mode = if executable {
                0o755
            } else if private_state_file(path) {
                0o600
            } else {
                permissions
                    .as_ref()
                    .map(PermissionsExt::mode)
                    .unwrap_or(0o666)
            };
            options.mode(mode);
        }
        let updated = (|| {
            use std::io::Write;
            let mut file = options
                .open(&temporary)
                .map_err(|error| error.to_string())?;
            if !executable && !private_state_file(path) {
                if let Some(permissions) = permissions {
                    file.set_permissions(permissions)
                        .map_err(|error| error.to_string())?;
                }
            }
            file.write_all(content.as_bytes())
                .map_err(|error| error.to_string())?;
            drop(file);
            std::fs::rename(&temporary, p).map_err(|error| error.to_string())
        })();
        if updated.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        updated?;
    }
    Ok(())
}

/// 저장소 쪽 잠금 디렉터리로 여러 프로세스와 SSH client의 읽기·수정·교체를 직렬화한다.
/// 파일이 없을 때만 빈 내용으로 넘기고, 읽기 실패는 다른 client의 기록을 덮지 않도록 그대로 실패시킨다.
pub fn update_text(
    path: &str,
    host: Option<&str>,
    update: impl FnOnce(&str) -> Result<String>,
) -> Result<()> {
    match host {
        Some(host) => update_remote_text(path, update, &|command, input| {
            run_input(command, Some(host), input)
        }),
        None => update_local_text(path, update),
    }
}
fn update_local_text(path: &str, update: impl FnOnce(&str) -> Result<String>) -> Result<()> {
    assert_state_path(path,None)?;
    let lock = format!("{path}.lock");
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut attempt = 0;
    loop {
        match std::fs::create_dir(&lock) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 199 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let updated = read_optional(path, None)
        .and_then(|content| update(&content.unwrap_or_default()))
        .and_then(|next| write(path, &next, None, false));
    let released = std::fs::remove_dir(&lock).map_err(|e| e.to_string());
    updated.and(released)
}
type RemoteCommandRunner<'a> = dyn Fn(&str, Option<&[u8]>) -> Result<String> + 'a;
/// 큰 내용은 명령 인자가 아니라 표준 입력으로 보내므로 Linux의 인자 길이 제한(E2BIG)에 걸리지 않는다.
fn update_remote_text(
    path: &str,
    update: impl FnOnce(&str) -> Result<String>,
    run_command: &RemoteCommandRunner<'_>,
) -> Result<()> {
    if let Some(command)=state_guard_command(path){run_command(&command,None)?;}
    let parent = Path::new(path)
        .parent()
        .ok_or("부모 경로 없음")?
        .to_string_lossy();
    let lock = quote(&format!("{path}.lock"));
    let quoted_path = quote(path);
    run_command(
        &format!(
            "mkdir -p -- {} || exit $?; kanvibe_attempt=0; while ! mkdir -- {lock} 2>/dev/null; do kanvibe_attempt=$((kanvibe_attempt + 1)); test \"$kanvibe_attempt\" -lt 200 || exit 1; sleep 0.05; done",
            quote(&parent)
        ),
        None,
    )?;
    let updated = run_command(
        &format!("if test -e {quoted_path}; then cat -- {quoted_path}; fi"),
        None,
    )
    .and_then(|content| update(&content))
    .and_then(|next| {
        run_command(&remote_write_command(path, false,Some(next.len()))?, Some(next.as_bytes())).map(|_| ())
    });
    let released = run_command(&format!("rmdir -- {lock}"), None).map(|_| ());
    updated.and(released)
}
/// 공유 상태 파일을 읽는다. 깨졌거나 객체가 아닌 내용은 없는 상태와 같게 다룬다.
fn parse_json_object(content: &str) -> Value {
    serde_json::from_str::<Value>(content)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
fn merge_project_state(content: &str, key: &str, value: Value) -> String {
    let mut state = parse_json_object(content);
    state["schemaVersion"] = json!(1);
    state[key] = value;
    state["updatedAt"] = json!(database::now());
    state.to_string()
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
    } else if session_type=="zellij" {
        String::from_utf16_lossy(&name.encode_utf16().take(60).collect::<Vec<_>>())
    } else {
        name
    }
}
pub fn worktree_path(repo: &str, branch: &str) -> String {
    format!("{repo}__worktrees/{}", branch.replace('/', "-"))
}
pub fn is_safe_git_ref(value:&str)->bool {
    !value.is_empty()&&value.as_bytes()[0].is_ascii_alphanumeric()&&!value.contains("..")&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||b"._/-".contains(&byte))
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
            resolve_path_for_shell(&worktree, host),
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
                update_text(&state_path, host, |content| {
                    Ok(merge_project_state(
                        content,
                        "priority",
                        task["priority"].clone(),
                    ))
                })?;
            }
        } else {
            v["priority"] = task["priority"].clone();
        }
    }
    let state_path=join(path,if metadata{".kanvibe/task.json"}else{".kanvibe/status.json"});
    if metadata{
        update_text(&state_path,host,|content|{
            let current=parse_json_object(content);
            if let Some(session)=current.get("session"){v["session"]=session.clone();}
            serde_json::to_string_pretty(&v).map(|content|format!("{content}\n")).map_err(|error|error.to_string())
        })?;
    }else{
        write(&state_path,&format!("{}\n",serde_json::to_string_pretty(&v).map_err(|error|error.to_string())?),host,false)?;
    }
    ensure_exclude(path, host)
}

pub fn persist_session(task: &Value, project: Option<&Value>) -> Result<()> {
    let kind = text(task, "sessionType");
    let name = text(task, "sessionName");
    if kind.is_empty() || name.is_empty() {
        return Ok(());
    }
    let root = project.is_some_and(|project| task["branchName"] == project["defaultBranch"]);
    let path = task["worktreePath"]
        .as_str()
        .filter(|path| !path.is_empty())
        .or_else(|| {
            if root {
                project.and_then(|project| project["repoPath"].as_str())
            } else {
                None
            }
        });
    let Some(path) = path else { return Ok(()) };
    let host = task["sshHost"]
        .as_str()
        .or_else(|| project.and_then(|project| project["sshHost"].as_str()));
    ensure_exclude(path, host)?;
    update_text(&join(path, ".kanvibe/task.json"), host, |content| {
        let mut metadata = parse_json_object(content);
        metadata["schemaVersion"] = json!(1);
        metadata["session"] = json!({"sessionType":kind,"sessionName":name});
        metadata["updatedAt"] = json!(database::now());
        Ok(metadata.to_string())
    })
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
    update_text(&path, host, |content| {
        Ok(merge_project_state(
            content,
            "projectColor",
            project["color"].clone(),
        ))
    })?;
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
        let state = parse_json_object(&read_optional(&path, host)?.unwrap_or_default());
        let is_propagated = ["projectColor", "priority"].iter().all(|key| {
            shared
                .get(*key)
                .is_none_or(|value| state.get(*key) == Some(value))
        });
        if !is_propagated {
            update_text(&path, host, |content| {
                let mut state = parse_json_object(content);
                for key in ["projectColor", "priority"] {
                    if let Some(value) = shared.get(key) {
                        state[key] = value.clone();
                    }
                }
                state["schemaVersion"] = json!(1);
                state["updatedAt"] = json!(database::now());
                Ok(state.to_string())
            })?;
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
    // 잘못된 revision을 빈 diff로 넘기면 저장된 집계가 0으로 덮이므로 조회 실패를 그대로 돌려준다.
    let output = git(root, &format!("diff {} --numstat -z", quote(&range)), host)?;
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
    let output = git(
        root,
        &format!("diff {} --name-status -z", quote(&range)),
        host,
    )?;
    let mut fields = output.split('\0');
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let first = fields.next().unwrap_or("");
        let is_moved = status.starts_with('R') || status.starts_with('C');
        let p = if is_moved {
            fields.next().unwrap_or(first)
        } else {
            first
        };
        let counts = stats.get(p).copied().unwrap_or_default();
        let mut file = json!({"path":p,"status":status_name(status),"additions":counts.0,"deletions":counts.1});
        if is_moved {
            file["originalPath"] = json!(first);
        }
        files.insert(p.into(), file);
    }
    // -z 상태 출력은 rename 목적지를 먼저 주고 이전 경로를 별도 필드로 준다.
    let output = git(root, "status --porcelain -z --untracked-files=all", host)?;
    let mut fields = output.split('\0');
    while let Some(line) = fields.next() {
        if line.len() < 3 {
            continue;
        }
        let xy = &line[..2];
        let p = &line[3..];
        let original = if xy.contains('R') || xy.contains('C') {
            fields.next()
        } else {
            None
        };
        if files.contains_key(p) {
            continue;
        }
        let Some(status) = working_tree_status(xy) else {
            continue;
        };
        // 커밋된 rename을 다시 옮긴 경우에도 base 브랜치의 원본 경로와 집계를 잃지 않는다.
        let previous = original.and_then(|path| files.remove(path));
        let counted = |key: &str| previous.as_ref().map_or(json!(0), |file| file[key].clone());
        let mut file = json!({"path":p,"status":status,"additions":counted("additions"),"deletions":counted("deletions")});
        if let Some(original) = original {
            file["originalPath"] = previous
                .as_ref()
                .and_then(|file| file.get("originalPath").cloned())
                .unwrap_or_else(|| json!(original));
        }
        files.insert(p.into(), file);
    }
    Ok(files.into_values().collect())
}
fn working_tree_status(xy: &str) -> Option<&'static str> {
    if xy == "??" || xy.contains('A') {
        Some("added")
    } else if xy.contains('D') {
        Some("deleted")
    } else if xy.contains('M') {
        Some("modified")
    } else if xy.contains('R') {
        Some("renamed")
    } else {
        None
    }
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
    regex::Regex::new(r"(?i)/kanvibe-plugin\.(?:[cm]?js|ts)$")
        .is_ok_and(|pattern| pattern.is_match(reference))
}
/// `opencode debug config`의 뒤쪽 구간에는 깨진 JSON이 섞일 수 있어, 그때는 맨 위 plugin 블록만 다시 읽는다.
fn extract_registered_plugin_urls(output: &str) -> Result<Vec<String>> {
    let entries = serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|config| config["plugin"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| entry.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    if !entries.is_empty() {
        return Ok(entries);
    }
    let block = regex::Regex::new(r#""plugin"\s*:\s*\[([\s\S]*?)\]\s*,\s*"[^"]+"\s*:"#)
        .map_err(|e| e.to_string())?;
    let Some(captures) = block.captures(output) else {
        return Ok(vec![]);
    };
    regex::Regex::new(r#""((?:\\.|[^"\\])*)""#)
        .map_err(|e| e.to_string())?
        .captures_iter(&captures[1])
        .map(|entry| serde_json::from_str(&format!("\"{}\"", &entry[1])).map_err(|e| e.to_string()))
        .collect()
}
fn read_opencode_debug_config(program: &str, repo: &str, timeout: Duration) -> PluginRegistration {
    let mut command = std::process::Command::new(program);
    command
        .args(["debug", "config"])
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .env_clear()
        .envs(crate::runtime::shell_environment());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let output = crate::ai::bounded_output(&mut command, timeout)?;
    if !output.status.success() {
        return Err(format!(
            "{} (exit {})",
            String::from_utf8_lossy(&output.stderr).trim(),
            output.status.code().unwrap_or(-1)
        ));
    }
    let urls = extract_registered_plugin_urls(&String::from_utf8_lossy(&output.stdout))?;
    Ok(urls
        .into_iter()
        .filter(|url| is_opencode_plugin_reference(url))
        .collect())
}
type PluginRegistration = Result<Vec<String>>;
const PLUGIN_REGISTRATION_TTL: Duration = Duration::from_secs(5);
const OPENCODE_DEBUG_CONFIG_TIMEOUT: Duration = Duration::from_secs(3);
struct PluginRegistrationQuery {
    outcome: Option<PluginRegistration>,
    expires_at: Option<Instant>,
    is_invalidated: bool,
}
/// 보드가 여러 task의 상태를 한꺼번에 물어도 OpenCode CLI는 경로마다 한 번만 돌도록 결과를 공유한다.
struct PluginRegistrationQueries {
    queries: Mutex<HashMap<String, PluginRegistrationQuery>>,
    completed: Condvar,
    ttl: Duration,
}
impl PluginRegistrationQueries {
    fn new(ttl: Duration) -> Self {
        Self {
            queries: Mutex::new(HashMap::new()),
            completed: Condvar::new(),
            ttl,
        }
    }
    fn get(&self, key: &str, query: impl FnOnce() -> PluginRegistration) -> PluginRegistration {
        let mut queries = self.queries.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match queries.get(key) {
                // 진행 중인 조회는 끝날 때까지 기다린다. 무효화된 조회도 동시 CLI 실행을 막기 위해 끝난 뒤 새로 시작한다.
                Some(existing) if existing.outcome.is_none() => {
                    queries = self
                        .completed
                        .wait(queries)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                Some(existing)
                    if !existing.is_invalidated
                        && existing.expires_at.is_some_and(|at| at > Instant::now()) =>
                {
                    return existing.outcome.clone().unwrap_or_else(|| Ok(vec![]));
                }
                _ => break,
            }
        }
        let now = Instant::now();
        queries.retain(|_, existing| existing.expires_at.is_none_or(|at| at > now));
        queries.insert(
            key.to_string(),
            PluginRegistrationQuery {
                outcome: None,
                expires_at: None,
                is_invalidated: false,
            },
        );
        drop(queries);
        let outcome = query();
        let mut queries = self.queries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = queries.get_mut(key) {
            existing.outcome = Some(outcome.clone());
            existing.expires_at = Some(Instant::now() + self.ttl);
        }
        self.completed.notify_all();
        outcome
    }
    fn invalidate(&self, key: &str) {
        let mut queries = self.queries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = queries.get_mut(key) {
            existing.is_invalidated = true;
        }
    }
}
fn plugin_registration_queries() -> &'static PluginRegistrationQueries {
    static QUERIES: OnceLock<PluginRegistrationQueries> = OnceLock::new();
    QUERIES.get_or_init(|| PluginRegistrationQueries::new(PLUGIN_REGISTRATION_TTL))
}
fn plugin_registration_key(repo: &str) -> String {
    std::path::absolute(repo)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| repo.to_string())
}
fn opencode_registered_plugin_urls(repo: &str) -> PluginRegistration {
    let key = plugin_registration_key(repo);
    plugin_registration_queries().get(&key, || {
        read_opencode_debug_config("opencode", &key, OPENCODE_DEBUG_CONFIG_TIMEOUT)
    })
}
fn invalidate_opencode_plugin_registration(repo: &str) {
    plugin_registration_queries().invalidate(&plugin_registration_key(repo));
}
/// 검증 실패를 미등록과 구분해 알리고, 설치 여부에 쓸 등록 판정만 돌려준다.
fn apply_opencode_plugin_registration(
    state: &mut Value,
    expected_plugin: &str,
    registration: PluginRegistration,
) -> bool {
    match registration {
        Ok(urls) => {
            let is_registered = urls.iter().any(|url| url == expected_plugin);
            state["hasRegisteredPlugin"] = json!(is_registered);
            state["hasDuplicateKanvibePlugins"] = json!(urls.len() > 1);
            state["registeredPluginUrls"] = json!(urls);
            is_registered
        }
        Err(error) => {
            state["pluginRegistrationError"] = json!(error);
            false
        }
    }
}
fn merge_hook_registration(
    content: &str,
    url: &str,
    task_id: &str,
    token: &str,
    client: &str,
) -> String {
    merge_hook_target(
        content,
        &json!({"url":url,"taskId":task_id,"token":token,"clientId":client}),
    )
}
fn normalize_hook_target(entry: &Value) -> Option<Value> {
    let url = text(entry, "url").trim().trim_end_matches('/');
    let task = text(entry, "taskId").trim();
    if !(url.starts_with("http://") || url.starts_with("https://"))
        || url.chars().any(char::is_whitespace)
        || task.is_empty()
    {
        return None;
    }
    let mut target = json!({"url":url,"taskId":task});
    let token = text(entry, "token").trim();
    if !token.is_empty() {
        target["token"] = json!(token);
    }
    let client = text(entry, "clientId");
    if crate::hook_transport::valid_client_id(client) {
        target["clientId"] = json!(client);
    }
    Some(target)
}
fn merge_hook_target(content: &str, target: &Value) -> String {
    let mut state = parse_json_object(content);
    let Some(target) = normalize_hook_target(target) else {
        return state.to_string();
    };
    let mut seen = std::collections::HashSet::new();
    let entries = state["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(normalize_hook_target)
        .filter(|entry| seen.insert(text(entry, "url").to_owned()));
    let same_client = |entry: &Value| {
        entry["url"] == target["url"]
            || (!text(&target, "clientId").is_empty()
                && (entry["clientId"] == target["clientId"]
                    || (text(entry, "clientId").is_empty() && entry["taskId"] == target["taskId"])))
    };
    let mut inserted = false;
    let mut next = Vec::new();
    for entry in entries {
        if same_client(&entry) {
            if !inserted {
                next.push(target.clone());
                inserted = true;
            }
        } else {
            next.push(entry);
        }
    }
    if !inserted {
        next.push(target);
    }
    state["schemaVersion"] = json!(1);
    state["targets"] = json!(next);
    state["updatedAt"] = json!(database::now());
    state.to_string()
}

pub fn install_hooks(
    db: &Database,
    path: &str,
    task_id: &str,
    host: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let url = hook_url(host)?;
    ensure_exclude(path, host)?;
    let token=crate::task_targets::build_token(db,task_id)?;
    let client=crate::core::settings(db,"getOrCreateHookClientId",&[])?.as_str().ok_or("Hook client ID가 없습니다")?.to_owned();
    update_text(&join(path,".kanvibe/targets.json"),host,|content|Ok(merge_hook_registration(content,&url,task_id,&token,&client)))?;
    write(&join(path,".kanvibe/task.sh"),crate::task_targets::TASK_CLI_SCRIPT,host,true)?;
    if let Some(host)=host{crate::hook_transport::ensure(host);}
    for provider in provider
        .map(|s| vec![s])
        .unwrap_or_else(|| vec!["claude", "gemini", "codex", "opencode"])
    {
        if provider == "opencode" {
            let installed = install_opencode_plugin(path, task_id, host, &url);
            // 실패해도 일부 파일이 바뀌었을 수 있으므로 이전 등록 판정을 재사용하지 않는다.
            if host.is_none() {
                invalidate_opencode_plugin_registration(path);
            }
            installed?;
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
                .replace("KANVIBE_TASK_ID", task_id)
                .replace("KANVIBE_HOOK_SOCKET_RESOLVER",crate::hook_transport::shell_socket_resolver());
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
            .any(|t| text(t, "url") == url && text(t, "taskId") == task_id && !text(t,"token").is_empty() && (host.is_none()||crate::hook_transport::valid_client_id(text(t,"clientId"))))
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
        // 원격 저장소는 OpenCode 설정 레지스트리를 조회할 수 없으므로 등록 여부를 검증하지 않는다.
        if host.is_some() {
            state["hasRegisteredPlugin"] = json!(true);
            state["hasDuplicateKanvibePlugins"] = json!(false);
            state["registeredPluginUrls"] = json!([]);
        } else {
            apply_opencode_plugin_registration(
                &mut state,
                &expected_plugin,
                opencode_registered_plugin_urls(path),
            );
        };

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
        let command = remote_write_command(path.to_str().unwrap(), false,None).unwrap();
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

#[cfg(test)]
mod shared_state_concurrency_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Copy, Debug)]
    enum Access {
        Local,
        Remote,
    }
    /// SSH 대신 같은 명령을 로컬 셸에서 돌린다. Linux에서 관측한 명령 인자 제한도 함께 재현한다.
    fn run_like_ssh(command: &str, input: Option<&[u8]>) -> Result<String> {
        if command.len() >= 128 * 1024 {
            return Err("spawn E2BIG".into());
        }
        run_input(command, None, input)
    }
    fn update_with(
        access: Access,
        path: &str,
        update: impl FnOnce(&str) -> Result<String>,
    ) -> Result<()> {
        match access {
            Access::Local => update_local_text(path, update),
            Access::Remote => update_remote_text(path, update, &run_like_ssh),
        }
    }
    fn read_state(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
    fn directory_entries(path: &Path) -> Vec<String> {
        std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
    fn concurrent_targets_survive(accesses: [Access; 8]) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("kanvibe-targets-'");
        let path = repo.join(".kanvibe/targets.json");
        let target = path.to_str().unwrap();
        std::thread::scope(|scope| {
            for (index, access) in accesses.into_iter().enumerate() {
                scope.spawn(move || {
                    update_with(access, target, |content| {
                        Ok(merge_hook_target(content, &json!({"url":&format!("http://client-{index}:19736"),"taskId":&format!("task-{index}")})))
                    })
                    .unwrap()
                });
            }
        });
        let state = read_state(&path);
        assert_eq!(state["schemaVersion"], 1);
        let targets = state["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 8);
        for index in 0..8 {
            assert!(targets
                .iter()
                .any(|t| t["url"] == format!("http://client-{index}:19736")
                    && t["taskId"] == format!("task-{index}")));
        }
        assert_eq!(directory_entries(path.parent().unwrap()), ["targets.json"]);
    }
    #[test]
    fn concurrent_distinct_clients_survive_with_local_repository_access() {
        concurrent_targets_survive([Access::Local; 8]);
    }
    #[test]
    fn concurrent_distinct_clients_survive_with_remote_repository_access() {
        concurrent_targets_survive([Access::Remote; 8]);
    }
    #[test]
    fn concurrent_distinct_clients_survive_with_mixed_repository_access() {
        use Access::{Local, Remote};
        concurrent_targets_survive([Remote, Local, Remote, Local, Remote, Local, Remote, Local]);
    }
    #[test]
    fn concurrent_color_and_priority_are_both_preserved() {
        use Access::{Local, Remote};
        for (color_access, priority_access) in [(Local, Local), (Remote, Remote), (Remote, Local)] {
            for priority in [json!("high"), Value::Null] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("kanvibe-project-'/.kanvibe/project.json");
                let state_path = path.to_str().unwrap();
                update_with(color_access, state_path, |content| {
                    Ok(merge_project_state(
                        content,
                        "projectColor",
                        json!("#111111"),
                    ))
                })
                .unwrap();
                update_with(priority_access, state_path, |content| {
                    Ok(merge_project_state(content, "priority", json!("low")))
                })
                .unwrap();
                std::thread::scope(|scope| {
                    scope.spawn(|| {
                        update_with(color_access, state_path, |content| {
                            Ok(merge_project_state(
                                content,
                                "projectColor",
                                json!("#222222"),
                            ))
                        })
                        .unwrap()
                    });
                    scope.spawn(|| {
                        update_with(priority_access, state_path, |content| {
                            Ok(merge_project_state(content, "priority", priority.clone()))
                        })
                        .unwrap()
                    });
                });
                let state = read_state(&path);
                assert_eq!(state["projectColor"], "#222222");
                assert_eq!(state["priority"], priority);
                assert_eq!(directory_entries(path.parent().unwrap()), ["project.json"]);
            }
        }
    }
    #[test]
    fn independent_processes_keep_each_others_registrations() {
        // 같은 테스트 바이너리를 작업 프로세스로 다시 띄운다. 값은 자식 프로세스 환경으로만 넘긴다.
        if let Ok(target) = std::env::var("KANVIBE_TEST_TARGETS_FILE") {
            let client = std::env::var("KANVIBE_TEST_TARGETS_CLIENT").unwrap();
            std::thread::scope(|scope| {
                for index in 0..4 {
                    let (target, client) = (&target, &client);
                    scope.spawn(move || {
                        update_local_text(target, |content| {
                            Ok(merge_hook_target(content, &json!({"url":&format!("http://client-{client}-{index}"),"taskId":&format!("task-{index}")})))
                        })
                        .unwrap()
                    });
                }
            });
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo/.kanvibe/targets.json");
        let workers = [0, 1].map(|client| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::shared_state_concurrency_tests::independent_processes_keep_each_others_registrations",
                ])
                .env("KANVIBE_TEST_TARGETS_FILE", &path)
                .env("KANVIBE_TEST_TARGETS_CLIENT", client.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        });
        for mut worker in workers {
            assert!(worker.wait().unwrap().success());
        }
        assert_eq!(read_state(&path)["targets"].as_array().unwrap().len(), 8);
        assert_eq!(directory_entries(path.parent().unwrap()), ["targets.json"]);
    }
    #[test]
    fn failed_update_releases_lock_without_replacing_saved_contents() {
        for access in [Access::Local, Access::Remote] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("targets.json");
            let target = path.to_str().unwrap();
            std::fs::write(&path, "saved").unwrap();
            let failed = update_with(access, target, |_| Err("update failed".into()));
            assert_eq!(failed.unwrap_err(), "update failed");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved");
            update_with(access, target, |content| Ok(format!("{content} next"))).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved next");
            assert_eq!(directory_entries(dir.path()), ["targets.json"]);
        }
    }
    #[test]
    fn read_failure_is_not_interpreted_as_an_absent_file() {
        for access in [Access::Local, Access::Remote] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("targets.json");
            std::fs::create_dir(&path).unwrap();
            let is_called = AtomicUsize::new(0);
            let result = update_with(access, path.to_str().unwrap(), |_| {
                is_called.fetch_add(1, Ordering::Relaxed);
                Ok("replacement".into())
            });
            assert!(result.is_err(), "{access:?}");
            assert_eq!(is_called.load(Ordering::Relaxed), 0);
            assert_eq!(directory_entries(dir.path()), ["targets.json"]);
        }
    }
    #[test]
    fn readers_only_observe_complete_json_while_large_writes_run() {
        for access in [Access::Local, Access::Remote] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(".kanvibe/targets.json");
            let target = path.to_str().unwrap();
            update_with(access, target, |content| {
                Ok(merge_hook_target(content, &json!({"url":"http://initial","taskId":"initial"})))
            })
            .unwrap();
            let is_completed = std::sync::atomic::AtomicBool::new(false);
            let reads = std::thread::scope(|scope| {
                let observer = scope.spawn(|| {
                    let mut reads = 0;
                    while !is_completed.load(Ordering::Relaxed) {
                        assert_eq!(read_state(&path)["schemaVersion"], 1);
                        reads += 1;
                    }
                    reads
                });
                let writers = (0..4)
                    .map(|index| {
                        scope.spawn(move || {
                            update_with(access, target, |content| {
                                Ok(merge_hook_target(content, &json!({"url":&format!("http://client-{index}"),"taskId":&"x".repeat(100_000)})))
                            })
                            .unwrap()
                        })
                    })
                    .collect::<Vec<_>>();
                // 작성자가 실패해도 관찰 스레드가 끝나도록 결과 확인을 뒤로 미룬다.
                let written = writers
                    .into_iter()
                    .map(|writer| writer.join())
                    .collect::<Vec<_>>();
                is_completed.store(true, Ordering::Relaxed);
                assert!(written.iter().all(|result| result.is_ok()));
                observer.join().unwrap()
            });
            assert!(reads > 0);
            assert_eq!(read_state(&path)["targets"].as_array().unwrap().len(), 5);
        }
    }
    #[test]
    fn non_object_shared_state_is_replaced_without_panicking() {
        assert_eq!(
            serde_json::from_str::<Value>(&merge_hook_target("[1]", &json!({"url":"http://a","taskId":"t"}))).unwrap()
                ["targets"][0]["url"],
            "http://a"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&merge_project_state("null", "priority", Value::Null))
                .unwrap()["schemaVersion"],
            1
        );
    }
}

#[cfg(test)]
mod opencode_registration_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting_query(count: &AtomicUsize, urls: &[&str]) -> PluginRegistration {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(urls.iter().map(|url| url.to_string()).collect())
    }
    #[test]
    fn extracts_plugin_urls_from_json_and_the_plugin_block_of_broken_output() {
        let valid =
            json!({"plugin":["file:///tmp/alpha.ts","file:///tmp/kanvibe-plugin.ts"],"agent":{}});
        assert_eq!(
            extract_registered_plugin_urls(&valid.to_string()).unwrap(),
            ["file:///tmp/alpha.ts", "file:///tmp/kanvibe-plugin.ts"]
        );
        let broken = "{\n  \"plugin\": [\n    \"file:///tmp/alpha.ts\",\n    \"file:///tmp/kanvibe-plugin.ts\"\n  ],\n  \"agent\": {\n    \"custom\": {\n      \"template\": \"line 1\nline 2\"\n    }\n  }\n}";
        assert_eq!(
            extract_registered_plugin_urls(broken).unwrap(),
            ["file:///tmp/alpha.ts", "file:///tmp/kanvibe-plugin.ts"]
        );
        assert!(extract_registered_plugin_urls("not config")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn recognizes_every_kanvibe_plugin_extension_case_insensitively() {
        for url in [
            "file:///tmp/kanvibe-plugin.ts",
            "file:///tmp/kanvibe-plugin.js",
            "file:///tmp/kanvibe-plugin.mjs",
            "file:///tmp/kanvibe-plugin.cjs",
            "file:///tmp/KANVIBE-PLUGIN.JS",
        ] {
            assert!(is_opencode_plugin_reference(url), "{url}");
        }
        assert!(!is_opencode_plugin_reference("file:///tmp/not-kanvibe.ts"));
    }
    #[test]
    fn shares_concurrent_queries_and_caches_until_the_time_limit() {
        let queries = PluginRegistrationQueries::new(Duration::from_millis(300));
        let count = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..10 {
                scope.spawn(|| {
                    queries
                        .get("/same/repo", || {
                            std::thread::sleep(Duration::from_millis(50));
                            counting_query(&count, &[])
                        })
                        .unwrap()
                });
            }
        });
        queries
            .get("/same/repo", || counting_query(&count, &[]))
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(350));
        queries
            .get("/same/repo", || counting_query(&count, &[]))
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        queries
            .get("/other/repo", || counting_query(&count, &[]))
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn normalizes_equivalent_repository_paths_to_one_key() {
        assert_eq!(
            plugin_registration_key("/same/repo/."),
            plugin_registration_key("/same/repo")
        );
    }
    #[test]
    fn retains_failure_for_the_cache_window_instead_of_reporting_empty_registration() {
        let queries = PluginRegistrationQueries::new(Duration::from_millis(200));
        let count = AtomicUsize::new(0);
        let failing = || {
            count.fetch_add(1, Ordering::SeqCst);
            Err("OpenCode timed out".to_string())
        };
        assert_eq!(
            queries.get("/slow", failing).unwrap_err(),
            "OpenCode timed out"
        );
        assert_eq!(
            queries.get("/slow", failing).unwrap_err(),
            "OpenCode timed out"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(250));
        assert!(queries.get("/slow", || Ok(vec![])).unwrap().is_empty());
    }
    #[test]
    fn invalidation_discards_a_completed_query() {
        let queries = PluginRegistrationQueries::new(Duration::from_secs(5));
        let count = AtomicUsize::new(0);
        queries
            .get("/installed", || counting_query(&count, &[]))
            .unwrap();
        queries.invalidate("/installed");
        let fresh = queries
            .get("/installed", || counting_query(&count, &["fresh"]))
            .unwrap();
        assert_eq!(fresh, ["fresh"]);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn invalidated_query_in_flight_finishes_before_one_fresh_query_is_shared() {
        let queries = PluginRegistrationQueries::new(Duration::from_secs(5));
        let count = AtomicUsize::new(0);
        let (started, wait_started) = std::sync::mpsc::channel();
        let (finish, wait_finish) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            let (shared_queries, shared_count) = (&queries, &count);
            let old = scope.spawn(move || {
                shared_queries.get("/installing", || {
                    started.send(()).unwrap();
                    wait_finish.recv().unwrap();
                    counting_query(shared_count, &[])
                })
            });
            wait_started.recv().unwrap();
            queries.invalidate("/installing");
            let fresh = [(); 2].map(|_| {
                scope.spawn(|| {
                    queries.get("/installing", || {
                        std::thread::sleep(Duration::from_millis(30));
                        counting_query(&count, &["fresh"])
                    })
                })
            });
            // 무효화된 조회가 끝나기 전에는 새 CLI를 띄우지 않는다.
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(count.load(Ordering::SeqCst), 0);
            finish.send(()).unwrap();
            assert!(old.join().unwrap().unwrap().is_empty());
            for query in fresh {
                assert_eq!(query.join().unwrap().unwrap(), ["fresh"]);
            }
        });
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
    fn fake_opencode(dir: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("opencode");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }
    #[test]
    fn debug_config_runs_in_the_repository_and_keeps_only_kanvibe_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(dir.path()).unwrap();
        let program = fake_opencode(
            &repo,
            r#"printf '{"plugin":["file://%s/kanvibe-plugin.js","file:///tmp/alpha.ts"],"argv":"%s"}' "$PWD" "$*""#,
        );
        let urls =
            read_opencode_debug_config(&program, repo.to_str().unwrap(), Duration::from_secs(3))
                .unwrap();
        assert_eq!(
            urls,
            [format!("file://{}/kanvibe-plugin.js", repo.display())]
        );
    }
    #[test]
    fn slow_debug_config_is_killed_with_its_children_and_reported_as_failure() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("child.pid");
        let program = fake_opencode(
            dir.path(),
            &format!(
                "sleep 30 &\nprintf '%s' $! > {}\nwait",
                quote(marker.to_str().unwrap())
            ),
        );
        let started = Instant::now();
        let error = read_opencode_debug_config(
            &program,
            dir.path().to_str().unwrap(),
            Duration::from_millis(1500),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let child = std::fs::read_to_string(&marker).unwrap();
        let mut is_child_alive = true;
        for _ in 0..50 {
            is_child_alive = std::process::Command::new("kill")
                .args(["-0", child.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            if !is_child_alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!is_child_alive, "자식 프로세스가 남아 있으면 안 된다");
    }
    #[test]
    fn exited_debug_config_with_lingering_pipe_does_not_outlast_the_time_limit() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_opencode(dir.path(), "sleep 30 &\nexit 0");
        let started = Instant::now();
        let error = read_opencode_debug_config(
            &program,
            dir.path().to_str().unwrap(),
            Duration::from_millis(1500),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
    #[test]
    fn missing_cli_and_nonzero_exit_are_failures_not_empty_registrations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        assert!(read_opencode_debug_config(
            &dir.path().join("absent").to_string_lossy(),
            root,
            Duration::from_secs(1)
        )
        .is_err());
        let program = fake_opencode(dir.path(), "echo boom >&2\nexit 3");
        let error = read_opencode_debug_config(&program, root, Duration::from_secs(3)).unwrap_err();
        assert!(
            error.contains("boom") && error.contains("exit 3"),
            "{error}"
        );
    }
    #[test]
    fn registration_failure_is_distinct_from_an_unregistered_plugin() {
        let mut state = json!({});
        let is_registered = apply_opencode_plugin_registration(
            &mut state,
            "file:///repo/kanvibe-plugin.js",
            Err("OpenCode timeout".into()),
        );
        assert!(!is_registered);
        assert_eq!(state, json!({"pluginRegistrationError":"OpenCode timeout"}));
    }
    #[test]
    fn global_plugin_diagnostics_survive_when_the_local_plugin_is_not_registered() {
        let mut state = json!({});
        let is_registered = apply_opencode_plugin_registration(
            &mut state,
            "file:///repo/kanvibe-plugin.js",
            Ok(vec![
                "file:///global/kanvibe-plugin.ts".into(),
                "file:///other/kanvibe-plugin.ts".into(),
            ]),
        );
        assert!(!is_registered);
        assert_eq!(state["hasRegisteredPlugin"], false);
        assert_eq!(state["hasDuplicateKanvibePlugins"], true);
        assert!(state.get("pluginRegistrationError").is_none());
    }
}

#[cfg(test)]
mod dev_behaviour_tests {
    use super::*;

    fn init_repo(root: &Path) -> &str {
        let path = root.to_str().unwrap();
        for args in [
            vec!["init", "-b", "main", path],
            vec!["-C", path, "config", "user.name", "QA"],
            vec!["-C", path, "config", "user.email", "qa@example.test"],
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        }
        path
    }
    fn commit_all(path: &str, message: &str) {
        git(path, "add .", None).unwrap();
        git(path, &format!("commit -m {}", quote(message)), None).unwrap();
    }
    fn task_on(path: &str, base: &str, branch: &str) -> Value {
        json!({"worktreePath":path,"baseBranch":base,"branchName":branch})
    }
    #[test]
    fn shell_paths_expand_only_the_leading_home_and_keep_the_rest_literal() {
        assert_eq!(
            resolve_path_for_shell("/workspace", Some("host")),
            "'/workspace'"
        );
        assert_eq!(
            resolve_path_for_shell("~/work", Some("host")),
            "\"$HOME\"'/work'"
        );
        let suffix = "/$ABSENT$(printf nope)`printf nope`\"'";
        let output = std::process::Command::new("sh")
            .args([
                "-c",
                &format!(
                    "printf '%s' {}",
                    resolve_path_for_shell(&format!("~{suffix}"), Some("fixture"))
                ),
            ])
            .env("HOME", "/fixture/home")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("/fixture/home{suffix}")
        );
    }
    #[test]
    fn git_and_worktree_operations_use_hostile_paths_and_branches_literally() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let repo = base.join("repo$ABSENT$(printf substituted)`printf tick`\"quote'");
        std::fs::create_dir(&repo).unwrap();
        let path = init_repo(&repo);
        git(path, "commit --allow-empty -m fixture", None).unwrap();
        assert_eq!(
            git(path, "rev-parse --is-inside-work-tree", None)
                .unwrap()
                .trim(),
            "true"
        );
        assert_eq!(
            git(path, "symbolic-ref --short HEAD", None).unwrap().trim(),
            "main"
        );
        let project = json!({"repoPath":path,"sshHost":null});
        assert_eq!(worktrees(&project).unwrap()[0]["path"], path);

        // 명령 치환이 실행됐다면 브랜치 이름에서 $(...) 부분이 사라진다.
        let hostile = "feature/literal$(printf${IFS}expanded)";
        let (worktree, _) = create_worktree(&project, hostile, "main", "tmux").unwrap();
        assert!(Path::new(&worktree).is_dir());
        assert!(worktrees(&project)
            .unwrap()
            .iter()
            .any(|tree| tree["path"] == json!(worktree) && tree["branch"] == hostile));
        git(
            path,
            &format!(
                "worktree remove --force {}",
                resolve_path_for_shell(&worktree, None)
            ),
            None,
        )
        .unwrap();
        git(path, &format!("branch -D -- {}", quote(hostile)), None).unwrap();
        assert_eq!(worktrees(&project).unwrap().len(), 1);
    }
    #[test]
    fn hook_status_follows_the_common_task_state_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = init_repo(dir.path());
        let project = json!({"repoPath":path,"defaultBranch":"main","sshHost":null});
        let status = dir.path().join(".kanvibe/status.json");
        let feature = json!({"branchName":"feature","worktreePath":null,"status":"done"});
        persist_task(&feature, Some(&project), false).unwrap();
        assert!(!status.exists(), "worktree 없는 브랜치는 저장 위치가 없다");
        let root = json!({"branchName":"main","worktreePath":null,"status":"done"});
        persist_task(&root, Some(&project), false).unwrap();
        assert!(std::fs::read_to_string(status)
            .unwrap()
            .contains("\"status\": \"done\""));
    }
    #[test]
    fn commented_codex_sections_keep_their_boundaries() {
        let content =
            "[features] # user settings\nfast_mode = true\n[other] # next table\nhooks = false\n";
        let updated = upsert_codex_config(content);
        assert_eq!(
            updated,
            "[features] # user settings\nfast_mode = true\nhooks = true\n[other] # next table\nhooks = false\n"
        );
        assert_eq!(upsert_codex_config(&updated), updated);
        assert!(codex_hooks_enabled(&updated));
    }
    #[test]
    fn diff_reads_unusual_file_names_and_rename_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let path = init_repo(dir.path());
        std::fs::write(dir.path().join("committed old.txt"), "committed rename\n").unwrap();
        std::fs::write(dir.path().join("working old.txt"), "working rename\n").unwrap();
        commit_all(path, "initial");
        git(path, "checkout -b feature", None).unwrap();
        git(
            path,
            &format!(
                "mv {} {}",
                quote("committed old.txt"),
                quote("committed 한글\tnew.txt")
            ),
            None,
        )
        .unwrap();
        std::fs::write(dir.path().join("stats\n\t한글.txt"), "first\nsecond\n").unwrap();
        commit_all(path, "rename");
        git(
            path,
            &format!(
                "mv {} {}",
                quote("working old.txt"),
                quote("working 새 파일.txt")
            ),
            None,
        )
        .unwrap();
        let untracked = [
            "hello world.txt",
            "한글.txt",
            "tab\tfile.txt",
            "line\nfile.txt",
        ];
        for name in untracked {
            std::fs::write(dir.path().join(name), "original\n").unwrap();
        }
        let files = diff_files(&task_on(path, "main", "feature")).unwrap();
        let mut names = files
            .iter()
            .map(|f| text(f, "path").to_string())
            .collect::<Vec<_>>();
        names.sort();
        let mut expected = untracked.map(str::to_string).to_vec();
        expected.extend([
            "committed 한글\tnew.txt".into(),
            "working 새 파일.txt".into(),
            "stats\n\t한글.txt".into(),
        ]);
        expected.sort();
        assert_eq!(names, expected);
        let stats = files
            .iter()
            .find(|f| f["path"] == "stats\n\t한글.txt")
            .unwrap();
        assert_eq!(
            stats,
            &json!({"path":"stats\n\t한글.txt","status":"added","additions":2,"deletions":0})
        );
        let mut renamed = files
            .iter()
            .filter(|f| f["status"] == "renamed")
            .map(|f| (text(f, "path"), text(f, "originalPath")))
            .collect::<Vec<_>>();
        renamed.sort();
        assert_eq!(
            renamed,
            [
                ("committed 한글\tnew.txt", "committed old.txt"),
                ("working 새 파일.txt", "working old.txt")
            ]
        );
        for name in &expected {
            let target = safe_path(path, name, None).unwrap();
            assert!(!read(&target, None).unwrap().is_empty());
            write(&target, "updated\n", None, false).unwrap();
            assert_eq!(
                std::fs::read_to_string(dir.path().join(name)).unwrap(),
                "updated\n"
            );
        }
    }
    #[test]
    fn rename_keeps_the_base_path_for_committed_staged_and_chained_moves() {
        let old_path = "old 한글\tfile.txt";
        let new_path = "new 한글\nfile.txt";
        let original = (0..12)
            .map(|i| format!("original line {i}\n"))
            .collect::<String>();
        for kind in ["committed", "staged", "chained"] {
            for is_edited in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = init_repo(dir.path());
                std::fs::write(dir.path().join(old_path), &original).unwrap();
                commit_all(path, "initial");
                git(path, "checkout -b feature", None).unwrap();
                git(
                    path,
                    &format!("mv {} {}", quote(old_path), quote(new_path)),
                    None,
                )
                .unwrap();
                let current = if is_edited {
                    original.replace("original line 5", "changed line 5")
                } else {
                    original.clone()
                };
                std::fs::write(dir.path().join(new_path), &current).unwrap();
                git(path, "add .", None).unwrap();
                if kind != "staged" {
                    git(path, "commit -m rename", None).unwrap();
                }
                let final_path = if kind == "chained" {
                    git(path, &format!("mv {} final.txt", quote(new_path)), None).unwrap();
                    "final.txt"
                } else {
                    new_path
                };
                let files = diff_files(&task_on(path, "main", "feature")).unwrap();
                assert_eq!(files.len(), 1, "{kind} {is_edited}: {files:?}");
                assert_eq!(files[0]["path"], final_path);
                assert_eq!(files[0]["originalPath"], old_path);
                assert_eq!(files[0]["status"], "renamed");
                let shown = git(
                    path,
                    &format!("show {}", quote(&format!("main:{old_path}"))),
                    None,
                )
                .unwrap();
                assert_eq!(shown, original);
                assert_eq!(
                    read(&safe_path(path, final_path, None).unwrap(), None).unwrap(),
                    current
                );
            }
        }
    }
    #[test]
    fn modified_staged_rename_reports_modified_like_the_working_tree_mapping() {
        assert_eq!(working_tree_status("RM"), Some("modified"));
        assert_eq!(working_tree_status("R "), Some("renamed"));
        assert_eq!(working_tree_status("??"), Some("added"));
        assert_eq!(working_tree_status("UU"), None);
    }
    #[test]
    fn invalid_revision_fails_but_an_empty_diff_succeeds_with_zero_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = init_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "initial\n").unwrap();
        commit_all(path, "initial");
        assert!(diff_files(&task_on(path, "missing-base", "main")).is_err());
        assert!(diff_files(&task_on(path, "main", "main"))
            .unwrap()
            .is_empty());
    }
    #[test]
    fn dot_dot_prefixed_files_are_inside_the_worktree_but_parents_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        std::fs::create_dir(dir.path().join("..directory")).unwrap();
        std::fs::write(dir.path().join("..notes"), "notes\n").unwrap();
        std::fs::write(dir.path().join("..directory/file"), "nested\n").unwrap();
        for host in [None, Some("remote-host")] {
            assert!(safe_path(root, "..notes", host).is_ok());
            assert!(safe_path(root, "..directory/file", host).is_ok());
            assert!(safe_path(root, "../outside", host).is_err());
            assert!(safe_path(root, "/outside", host).is_err());
        }
    }
}

#[cfg(all(test,unix))]
mod state_security_regressions {
    use super::*;
    #[test]
    fn targets_keep_primary_client_order_and_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let state = r#"{"targets":[{"clientId":"client-a","taskId":"old-a","token":"old","url":"http://a/"},{"clientId":"client-b","taskId":"b","token":"b-secret","url":"http://b"}]}"#;
        let updated = merge_hook_registration(state, "http://new-a/", "a", "a-secret", "client-a");
        let parsed: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(
            parsed["targets"][0],
            json!({"url":"http://new-a","taskId":"a","token":"a-secret","clientId":"client-a"})
        );
        assert_eq!(parsed["targets"][1]["url"], "http://b");
        let serialized = parsed["targets"][0].to_string();
        assert!(serialized.find("\"url\"").unwrap() < serialized.find("\"taskId\"").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".kanvibe/targets.json");
        write(path.to_str().unwrap(), &updated, None, false).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        update_remote_text(
            path.to_str().unwrap(),
            |_| Ok(updated.clone()),
            &|command, input| {
                assert!(!command.contains("a-secret"));
                run_input(command, None, input)
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn remote_state_guard_refuses_symlinks_and_truncated_stdin_keeps_original() {
        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("repo");
        let outside = parent.path().join("outside");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join(".kanvibe")).unwrap();
        let target = repo.join(".kanvibe/targets.json");
        assert!(update_remote_text(
            target.to_str().unwrap(),
            |_| Ok("secret".into()),
            &|command, input| run_input(command, None, input)
        )
        .is_err());
        assert!(!outside.join("targets.json").exists());
        let file = outside.join("keep");
        std::fs::write(&file, "original").unwrap();
        let command = remote_write_command(file.to_str().unwrap(), false, Some(100)).unwrap();
        assert!(run_input(&command, None, Some(b"short")).is_err());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    }
    #[test]
    fn zellij_names_limit_native_socket_length_and_network_refs_exclude_shell_syntax() {
        assert_eq!(
            session_name_for("zellij", "/work/repo", &"x".repeat(100))
                .encode_utf16()
                .count(),
            60
        );
        for valid in ["main", "feature/task-1", "release/1.5.3"] {
            assert!(is_safe_git_ref(valid));
        }
        for invalid in ["", "-flag", "../outside", "$(id)", "a`id`", "a b", "a\n"] {
            assert!(!is_safe_git_ref(invalid));
        }
    }
    #[test]
    fn state_symlink_directory_cannot_receive_hook_credentials() {
        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("repo");
        let outside = parent.path().join("outside");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join(".kanvibe")).unwrap();
        assert!(write(
            repo.join(".kanvibe/targets.json").to_str().unwrap(),
            "secret",
            None,
            false
        )
        .is_err());
        assert!(!outside.join("targets.json").exists());
    }
    #[test]
    fn state_symlink_file_is_not_overwritten_by_installer() {
        let parent = tempfile::tempdir().unwrap();
        let state = parent.path().join(".kanvibe");
        std::fs::create_dir(&state).unwrap();
        let victim = parent.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, state.join("task.sh")).unwrap();
        assert!(write(
            state.join("task.sh").to_str().unwrap(),
            "replace",
            None,
            true
        )
        .is_err());
        assert_eq!(std::fs::read_to_string(victim).unwrap(), "keep");
    }
}
