use crate::{
    database::{self, nullable, text, Database, Result},
    git,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};
use tauri::{AppHandle, Emitter, Manager};

const STATUSES: [&str; 5] = ["todo", "progress", "pending", "review", "done"];
fn arg(args: &[Value], index: usize) -> &str {
    args.get(index).and_then(Value::as_str).unwrap_or("")
}
pub fn board_update(app: &AppHandle) {
    let _ = app.emit("kanvibe:board-event", json!({"type":"board-updated"}));
}
pub fn dispatch(app: &AppHandle, namespace: &str, method: &str, args: Vec<Value>) -> Result<Value> {
    match namespace {
        "appSettings" => settings(&app.state::<Database>(), method, &args),
        "paneLayout" => pane_layout(&app.state::<Database>(), method, &args),
        "kanban" => kanban(app, method, &args),
        "project" => project(app, method, &args),
        "diff" if method == "getTaskDiffStats" => git::diff_stats(app, args),
        "diff" | "editor" => git::dispatch(app, namespace, method, args),
        "hooks" => hooks(app, method, &args),
        _ => Err(format!("알 수 없는 서비스: {namespace}.{method}")),
    }
}
pub fn settings(db: &Database, method: &str, args: &[Value]) -> Result<Value> {
    let mapping = [
        (
            "SidebarDefaultCollapsed",
            "sidebar_default_collapsed",
            false,
        ),
        ("SidebarHintDismissed", "sidebar_hint_dismissed", false),
        ("DoneAlertDismissed", "done_alert_dismissed", false),
        (
            "NotificationUnreadOnlyEnabled",
            "notification_unread_only",
            false,
        ),
        ("BackgroundSyncEnabled", "background_sync_enabled", true),
        ("VimModeEnabled", "vim_mode_enabled", true),
    ];
    for (name, key, default) in mapping {
        if method == format!("get{name}") {
            return Ok(json!(db
                .setting(key)?
                .map(|s| s == "true")
                .unwrap_or(default)));
        }
        if method == format!("set{name}") {
            db.set_setting(
                key,
                if args.first().and_then(Value::as_bool).unwrap_or(false) {
                    "true"
                } else {
                    "false"
                },
            )?;
            if key == "background_sync_enabled" {
                crate::integrations::wake_background();
            }
            return Ok(Value::Null);
        }
    }
    match method {
        "getAppSetting" => Ok(json!(db.setting(arg(args, 0))?)),
        "setAppSetting" => {
            db.set_setting(arg(args, 0), arg(args, 1))?;
            Ok(Value::Null)
        }
        "dismissSidebarHint" => {
            db.set_setting("sidebar_hint_dismissed", "true")?;
            Ok(Value::Null)
        }
        "dismissDoneAlert" => {
            db.set_setting("done_alert_dismissed", "true")?;
            Ok(Value::Null)
        }
        "getDefaultSessionType" => {
            let value = db.setting("default_session_type")?.unwrap_or_default();
            Ok(json!(
                if ["tmux", "zellij", "terminal"].contains(&value.as_str()) {
                    value
                } else {
                    "tmux".into()
                }
            ))
        }
        "setDefaultSessionType" => {
            if !["tmux", "zellij", "terminal"].contains(&arg(args, 0)) {
                return Err("유효하지 않은 세션 타입".into());
            }
            db.set_setting("default_session_type", arg(args, 0))?;
            Ok(Value::Null)
        }
        "getNotificationSettings" => Ok(
            json!({"isEnabled":db.setting("notification_enabled")?.as_deref()!=Some("false"),"enabledStatuses":db.setting("notification_statuses")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).filter(Value::is_array).unwrap_or(json!(["progress","pending","review"]))}),
        ),
        "setNotificationEnabled" => {
            db.set_setting(
                "notification_enabled",
                if args.first().and_then(Value::as_bool).unwrap_or(false) {
                    "true"
                } else {
                    "false"
                },
            )?;
            Ok(Value::Null)
        }
        "setNotificationStatuses" => {
            let value = args.first().cloned().unwrap_or(json!([]));
            if !value.is_array() {
                return Err("상태 배열 필요".into());
            }
            db.set_setting("notification_statuses", &value.to_string())?;
            Ok(Value::Null)
        }
        "getBackgroundSyncIntervalMs" => Ok(json!(db
            .setting("background_sync_interval_ms")?
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(600_000))),
        "setBackgroundSyncIntervalMs" => {
            let n = args
                .first()
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .ok_or("양수 sync 주기 필요")?;
            db.set_setting("background_sync_interval_ms", &n.to_string())?;
            crate::integrations::wake_background();
            Ok(Value::Null)
        }
        "getReleaseUpdateDismissedVersions" => {
            let values = db
                .setting("release_update_dismissed_versions")?
                .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                .unwrap_or_default();
            let mut seen = HashSet::new();
            Ok(json!(values
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && seen.insert(s.clone()))
                .collect::<Vec<_>>()))
        }
        "dismissReleaseUpdateVersion" => {
            let version = arg(args, 0).trim();
            if !version.is_empty() {
                let mut versions = settings(db, "getReleaseUpdateDismissedVersions", &[])?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if !versions.contains(&json!(version)) {
                    versions.push(json!(version));
                    db.set_setting(
                        "release_update_dismissed_versions",
                        &json!(versions).to_string(),
                    )?;
                }
            }
            Ok(Value::Null)
        }
        "getShortcutBindings" | "getTaskSearchShortcut" => {
            let mut bindings: Value =
                serde_json::from_str(include_str!("../resources/default-shortcuts.json"))
                    .map_err(|e| e.to_string())?;
            if let Some(overrides) = db
                .setting("shortcut_bindings")?
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            {
                if let Some(map) = overrides.as_object() {
                    for (k, v) in map {
                        if bindings.get(k).is_some() && v.is_string() {
                            bindings[k] = v.clone();
                        }
                    }
                }
            } else if let Some(legacy) = db.setting("task_search_shortcut")? {
                if !legacy.trim().is_empty() {
                    bindings["taskSearch"] =
                        json!(legacy.replace("Meta+", "Mod+").replace("Ctrl+", "Mod+"));
                    db.set_setting(
                        "shortcut_bindings",
                        &json!({"taskSearch":bindings["taskSearch"]}).to_string(),
                    )?;
                }
            }
            if method == "getTaskSearchShortcut" {
                Ok(bindings["taskSearch"].clone())
            } else {
                Ok(bindings)
            }
        }
        "setShortcutBindings" => {
            let bindings = args.first().ok_or("단축키 설정 필요")?;
            if !bindings.is_object() {
                return Err("단축키 설정 객체 필요".into());
            }
            db.set_setting("shortcut_bindings", &bindings.to_string())?;
            Ok(Value::Null)
        }
        "getBoardSortPreference" => {
            let value = db
                .setting("board_sort_preference")?
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or(Value::Null);
            let mut seen = HashSet::new();
            let keys=value["keys"].as_array().cloned().unwrap_or_default().into_iter().filter(|v|["priority","createdAt","updatedAt","title","project"].contains(&text(v,"field"))&&seen.insert(text(v,"field").to_string())).map(|v|json!({"field":v["field"],"direction":if text(&v,"direction")=="desc"{"desc"}else{"asc"}})).collect::<Vec<_>>();
            Ok(json!({"keys":keys}))
        }
        "setBoardSortPreference" => {
            db.set_setting(
                "board_sort_preference",
                &args
                    .first()
                    .cloned()
                    .unwrap_or(json!({"keys":[]}))
                    .to_string(),
            )?;
            Ok(Value::Null)
        }
        _ => Err(format!("알 수 없는 설정 메서드: {method}")),
    }
}
fn layouts(db: &Database) -> Result<Vec<Value>> {
    let c = db.lock()?;
    let mut s=c.prepare("SELECT id,layout_type,panes,project_id,is_global,created_at,updated_at FROM pane_layout_configs ORDER BY is_global DESC,created_at ASC").map_err(|e|e.to_string())?;
    let rows=s.query_map([],|r|{let panes:String=r.get(2)?;Ok(json!({"id":r.get::<_,String>(0)?,"layoutType":r.get::<_,String>(1)?,"panes":serde_json::from_str::<Value>(&panes).unwrap_or(json!([])),"projectId":r.get::<_,Option<String>>(3)?,"isGlobal":r.get::<_,bool>(4)?,"createdAt":r.get::<_,String>(5)?,"updatedAt":r.get::<_,String>(6)?}))}).map_err(|e|e.to_string())?.collect::<std::result::Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(rows)
}
pub fn pane_layout(db: &Database, method: &str, args: &[Value]) -> Result<Value> {
    let all = layouts(db)?;
    let global = all.iter().find(|v| v["isGlobal"].as_bool() == Some(true));
    let project = all
        .iter()
        .find(|v| text(v, "projectId") == arg(args, 0) && !arg(args, 0).is_empty());
    match method {
        "getAllPaneLayouts" => Ok(json!(all)),
        "getGlobalPaneLayout" => Ok(global.cloned().unwrap_or(Value::Null)),
        "getProjectPaneLayout" => Ok(project.cloned().unwrap_or(Value::Null)),
        "getEffectivePaneLayout" => Ok(project.or(global).cloned().unwrap_or(Value::Null)),
        "deletePaneLayout" => Ok(json!(
            db.lock()?
                .execute(
                    "DELETE FROM pane_layout_configs WHERE id=?1",
                    [arg(args, 0)]
                )
                .map_err(|e| e.to_string())?
                > 0
        )),
        "savePaneLayout" => {
            let v = args.first().ok_or("레이아웃 입력 필요")?;
            if ![
                "single",
                "horizontal_2",
                "vertical_2",
                "left_right_tb",
                "left_tb_right",
                "quad",
            ]
            .contains(&text(v, "layoutType"))
            {
                return Err("유효하지 않은 레이아웃".into());
            }
            let existing = if v["isGlobal"].as_bool() == Some(true) {
                global
            } else {
                all.iter()
                    .find(|l| !text(v, "projectId").is_empty() && l["projectId"] == v["projectId"])
            };
            let id = existing
                .map(|l| text(l, "id").to_string())
                .unwrap_or_else(database::id);
            db.lock()?.execute("INSERT INTO pane_layout_configs(id,layout_type,panes,project_id,is_global,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(id) DO UPDATE SET layout_type=excluded.layout_type,panes=excluded.panes,updated_at=excluded.updated_at",params![id,text(v,"layoutType"),v["panes"].to_string(),nullable(v,"projectId"),v["isGlobal"].as_bool().unwrap_or(false),database::now()]).map_err(|e|e.to_string())?;
            Ok(layouts(db)?
                .into_iter()
                .find(|v| text(v, "id") == id)
                .unwrap_or(Value::Null))
        }
        _ => Err(format!("알 수 없는 레이아웃 메서드: {method}")),
    }
}

fn project_for_task(db: &Database, task: &Value) -> Result<Option<Value>> {
    db.project(text(task, "projectId"))
}
fn save_task(app: &AppHandle, task: &Value, metadata: bool) -> Result<Value> {
    let db = app.state::<Database>();
    let propagate_shared = metadata
        || db
            .task(text(task, "id"))?
            .is_none_or(|previous| previous["worktreePath"] != task["worktreePath"]);
    let saved = db.save_task(task)?;
    let project = project_for_task(&db, &saved)?;
    if let Err(error) = git::persist_task(&saved, project.as_ref(), false) {
        eprintln!("작업 상태 파일 저장 실패: {error}");
    }
    if metadata {
        if let Err(error) = git::persist_task(&saved, project.as_ref(), true) {
            eprintln!("작업 메타데이터 저장 실패: {error}");
        }
    }
    if let Some(project) = project.as_ref().filter(|_| propagate_shared) {
        if let Err(error) = git::sync_project_state(&db, project) {
            eprintln!("프로젝트 공유 상태 전파 실패: {error}");
        }
    }
    board_update(app);
    Ok(saved)
}
fn task_with_project(db: &Database, id: &str) -> Result<Value> {
    let Some(mut task) = db.task(id)? else {
        return Ok(Value::Null);
    };
    task["project"] = project_for_task(db, &task)?.unwrap_or(Value::Null);
    Ok(task)
}
fn validate_status(status: &str) -> Result<()> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!("유효하지 않은 상태입니다: {status}"))
    }
}
fn create_task(app: &AppHandle, input: &Value) -> Result<Value> {
    let db = app.state::<Database>();
    if let Some(session) = input["sessionType"].as_str() {
        if !["tmux", "zellij", "terminal"].contains(&session) {
            return Err("유효하지 않은 세션 타입".into());
        }
    }
    let mut task = input.clone();
    if !task.is_object() {
        return Err("태스크 입력 객체 필요".into());
    }
    task["id"] = json!(database::id());
    task["status"] = json!("todo");
    if text(&task, "title").is_empty() {
        task["title"] = json!(if text(input, "branchName").is_empty() {
            "Untitled"
        } else {
            text(input, "branchName")
        });
    }
    if let Some(priority) = input["priority"].as_str() {
        if !["low", "medium", "high"].contains(&priority) {
            return Err("유효하지 않은 우선순위".into());
        }
    }
    if !text(input, "branchName").is_empty()
        && !text(input, "sessionType").is_empty()
        && !text(input, "projectId").is_empty()
    {
        if let Some(project) = db.project(text(input, "projectId"))? {
            let base = if text(input, "baseBranch").is_empty() {
                text(&project, "defaultBranch")
            } else {
                text(input, "baseBranch")
            };
            let (path, session) = git::create_worktree(&project, text(input, "branchName"), base)?;
            task["baseBranch"] = json!(base);
            task["worktreePath"] = json!(path);
            task["sessionName"] = json!(session);
            task["sshHost"] = project["sshHost"].clone();
        }
    }
    let saved = save_task(
        app,
        &task,
        !input["description"].is_null() || !input["priority"].is_null(),
    )?;
    install_task_hooks(app, &saved, None);
    Ok(saved)
}
fn install_task_hooks(app: &AppHandle, task: &Value, provider: Option<&str>) {
    if let Some(path) = task["worktreePath"].as_str() {
        if let Err(error) =
            git::install_hooks(path, text(task, "id"), task["sshHost"].as_str(), provider)
        {
            let _=app.emit("kanvibe:board-event",json!({"type":"task-hook-install-failed","taskId":task["id"],"taskTitle":task["title"],"error":error}));
        }
    }
}
fn cleanup_task(app: &AppHandle, task: &Value) -> Result<()> {
    let db = app.state::<Database>();
    let project = project_for_task(&db, task)?;
    let host = task["sshHost"]
        .as_str()
        .or_else(|| project.as_ref().and_then(|p| p["sshHost"].as_str()));
    crate::terminal::kill_task(app, text(task, "id"))?;
    if let Some(session) = task["sessionName"].as_str() {
        match text(task, "sessionType") {
            "tmux" => {
                for prefix in ["tmux", "tmux -L kanvibe"] {
                    let _ = git::run(
                        &format!("{prefix} kill-session -t {}", git::quote(session)),
                        host,
                    );
                }
            }
            "zellij" => {
                let _ = git::run(
                    &format!("zellij kill-sessions {}", git::quote(session)),
                    host,
                );
                let _ = git::run(
                    &format!("zellij delete-session {}", git::quote(session)),
                    host,
                );
            }
            _ => {}
        }
    }
    if let Some(project) = project.as_ref() {
        let repo = text(project, "repoPath");
        let branch = text(task, "branchName");
        let path = text(task, "worktreePath");
        if !branch.is_empty() && branch != text(project, "defaultBranch") && path != repo {
            let worktrees = git::worktrees(project)?;
            if let Some(found) = worktrees.iter().find(|w| {
                text(w, "branch") == branch && (!path.is_empty() && text(w, "path") == path)
            }) {
                git::git(
                    repo,
                    &format!(
                        "worktree remove --force {}",
                        git::quote(text(found, "path"))
                    ),
                    host,
                )?;
            }
            if git::git(
                repo,
                &format!(
                    "show-ref --verify --quiet {}",
                    git::quote(&format!("refs/heads/{branch}"))
                ),
                host,
            )
            .is_ok()
            {
                git::git(repo, &format!("branch -D -- {}", git::quote(branch)), host)?;
            }
        }
    } else if !text(task, "branchName").is_empty() && !text(task, "worktreePath").is_empty() {
        return Err(
            "연결된 프로젝트를 찾을 수 없어 worktree/브랜치 정리를 완료할 수 없습니다.".into(),
        );
    }
    Ok(())
}
fn delete_task(app: &AppHandle, id: &str) -> Result<bool> {
    let db = app.state::<Database>();
    let Some(task) = db.task(id)? else {
        return Ok(false);
    };
    cleanup_task(app, &task)?;
    db.lock()?
        .execute("DELETE FROM kanban_tasks WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    board_update(app);
    Ok(true)
}
fn kanban(app: &AppHandle, method: &str, args: &[Value]) -> Result<Value> {
    let db = app.state::<Database>();
    match method {
        "getTasksByStatus" => {
            let mut grouped = json!({"todo":[],"progress":[],"pending":[],"review":[],"done":[]});
            let (tasks, done) = db.board_tasks()?;
            for task in tasks {
                let status = text(&task, "status").to_string();
                if let Some(a) = grouped.get_mut(&status).and_then(Value::as_array_mut) {
                    a.push(task);
                }
            }
            Ok(json!({"tasks":grouped,"doneTotal":done,"doneLimit":20}))
        }
        "getMoreDoneTasks" => {
            let (tasks, count) = db.done_tasks(
                args.first().and_then(Value::as_i64).unwrap_or(0).max(0),
                args.get(1)
                    .and_then(Value::as_i64)
                    .unwrap_or(20)
                    .clamp(0, 1000),
            )?;
            Ok(json!({"tasks":tasks,"doneTotal":count}))
        }
        "getTaskById" => task_with_project(&db, arg(args, 0)),
        "getSearchableTasks" => {
            let projects = db.projects()?;
            Ok(json!(db.tasks()?.into_iter().map(|t|{let p=projects.iter().find(|p|p["id"]==t["projectId"]);json!({"id":t["id"],"title":t["title"],"branchName":t["branchName"],"projectId":t["projectId"],"projectName":p.map(|p|p["name"].clone()),"sshHost":t["sshHost"].as_str().map(str::to_string).or_else(||p.and_then(|p|p["sshHost"].as_str().map(str::to_string))),"status":t["status"],"updatedAt":t["updatedAt"]})}).collect::<Vec<_>>()))
        }
        "getTaskIdByProjectAndBranch" => Ok(db
            .tasks()?
            .iter()
            .find(|t| text(t, "projectId") == arg(args, 0) && text(t, "branchName") == arg(args, 1))
            .map(|t| t["id"].clone())
            .unwrap_or(Value::Null)),
        "createTask" => create_task(app, args.first().ok_or("태스크 입력 필요")?),
        "updateTask" => {
            let Some(mut task) = db.task(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            let updates = args.get(1).ok_or("수정 값 필요")?;
            for key in ["title", "description", "priority"] {
                if let Some(v) = updates.get(key) {
                    task[key] = v.clone();
                }
            }
            if let Some(p) = task["priority"].as_str() {
                if !["low", "medium", "high"].contains(&p) {
                    return Err("유효하지 않은 우선순위".into());
                }
            }
            save_task(
                app,
                &task,
                updates.get("description").is_some() || updates.get("priority").is_some(),
            )
        }
        "updateTaskStatus" | "moveTaskToColumn" => {
            validate_status(arg(args, 1))?;
            let Some(mut task) = db.task(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            task["status"] = json!(arg(args, 1));
            let saved = save_task(app, &task, false)?;
            if method == "moveTaskToColumn" {
                Ok(Value::Null)
            } else {
                Ok(saved)
            }
        }
        "deleteTask" => Ok(json!(delete_task(app, arg(args, 0))?)),
        "deleteTasks" => {
            let mut deleted = Vec::new();
            let mut seen = HashSet::new();
            for id in args
                .first()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if seen.insert(id) && delete_task(app, id)? {
                    deleted.push(id);
                }
            }
            Ok(json!(deleted))
        }
        "branchFromTask" => {
            let Some(mut task) = db.task(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            let Some(project) = db.project(arg(args, 1))? else {
                return Ok(Value::Null);
            };
            let (path, session) = git::create_worktree(&project, arg(args, 3), arg(args, 2))?;
            task["projectId"] = project["id"].clone();
            task["branchName"] = json!(arg(args, 3));
            task["baseBranch"] = json!(arg(args, 2));
            task["sessionType"] = json!(arg(args, 4));
            task["sessionName"] = json!(session);
            task["worktreePath"] = json!(path);
            task["sshHost"] = project["sshHost"].clone();
            task["status"] = json!("progress");
            let saved = save_task(app, &task, false)?;
            install_task_hooks(app, &saved, None);
            Ok(saved)
        }
        "connectTerminalSession" => {
            let Some(mut task) = db.task(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            let Some(project) = project_for_task(&db, &task)? else {
                return Ok(Value::Null);
            };
            let session_type = arg(args, 1);
            if !["tmux", "zellij", "terminal"].contains(&session_type) {
                return Err("유효하지 않은 세션 타입".into());
            }
            let branch = task["branchName"]
                .as_str()
                .or_else(|| task["baseBranch"].as_str())
                .ok_or("브랜치가 없습니다")?;
            task["sessionName"] = json!(git::session_name(text(&project, "repoPath"), branch));
            task["sessionType"] = json!(session_type);
            if text(&task, "worktreePath").is_empty() {
                task["worktreePath"] = project["repoPath"].clone();
            }
            task["sshHost"] = project["sshHost"].clone();
            let saved = save_task(app, &task, false)?;
            install_task_hooks(app, &saved, None);
            Ok(saved)
        }
        "updateProjectColor" => {
            let Some(project) = db.project(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            let color = arg(args, 1);
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err("유효한 #RRGGBB 색상이 필요합니다".into());
            }
            let base = text(&project, "repoPath")
                .split("__worktrees")
                .next()
                .unwrap_or("");
            let mut related = db
                .projects()?
                .into_iter()
                .filter(|p| {
                    text(p, "repoPath").starts_with(base) && p["sshHost"] == project["sshHost"]
                })
                .collect::<Vec<_>>();
            related.sort_by_key(|p| text(p, "repoPath") != base);
            for mut p in related {
                p["color"] = json!(color);
                git::persist_color(&p)?;
                git::sync_project_state(&db, &p)?;
                db.save_project(&p)?;
            }
            board_update(app);
            Ok(Value::Null)
        }
        "fetchAndSavePrUrl" => {
            let Some(mut task) = db.task(arg(args, 0))? else {
                return Ok(Value::Null);
            };
            let project = project_for_task(&db, &task)?;
            let root = project
                .as_ref()
                .map(|p| text(p, "repoPath"))
                .unwrap_or_else(|| text(&task, "worktreePath"));
            let host = task["sshHost"]
                .as_str()
                .or_else(|| project.as_ref().and_then(|p| p["sshHost"].as_str()));
            if text(&task, "branchName").is_empty() {
                return Ok(Value::Null);
            }
            let output = git::run(
                &format!(
                    "cd {} && gh pr list --head {} --json url -q '.[0].url'",
                    git::quote(root),
                    git::quote(text(&task, "branchName"))
                ),
                host,
            );
            if let Ok(url) = output {
                if !url.trim().is_empty() {
                    task["prUrl"] = json!(url.trim());
                    db.save_task(&task)?;
                    board_update(app);
                    return Ok(json!(url.trim()));
                }
            }
            Ok(Value::Null)
        }
        _ => Err(format!("알 수 없는 태스크 메서드: {method}")),
    }
}

fn project(app: &AppHandle, method: &str, args: &[Value]) -> Result<Value> {
    let db = app.state::<Database>();
    match method {
        "getAllProjects" => {
            let projects = db.projects()?;
            for project in &projects {
                schedule_project_icon(app, project);
            }
            Ok(json!(projects))
        }
        "getProjectById" => Ok(db.project(arg(args, 0))?.unwrap_or(Value::Null)),
        "getAvailableHosts" => {
            let mut hosts = Vec::new();
            let root = dirs::home_dir().unwrap_or_default().join(".ssh/config");
            read_ssh_hosts(&root, &mut HashSet::new(), &mut hosts);
            Ok(json!(hosts))
        }
        "registerProject" => register_project(
            app,
            arg(args, 0),
            arg(args, 1),
            args.get(2).and_then(Value::as_str),
        ),
        "deleteProject" => {
            let id = arg(args, 0);
            if db.project(id)?.is_none() {
                return Ok(json!(false));
            }
            for task in db
                .tasks()?
                .into_iter()
                .filter(|t| text(t, "projectId") == id)
            {
                crate::terminal::kill_task(app, text(&task, "id"))?;
            }
            let mut c = db.lock()?;
            let tx = c.transaction().map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM kanban_tasks WHERE project_id=?1", [id])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM projects WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            board_update(app);
            Ok(json!(true))
        }
        "listSubdirectories" => {
            let host = args.get(1).and_then(Value::as_str);
            let root = expand_path(arg(args, 0), host);
            let output = git::run(
                &format!("find {} -mindepth 1 -maxdepth 1 -type d", root),
                host,
            )
            .unwrap_or_default();
            let mut dirs = output
                .lines()
                .filter_map(|s| {
                    Path::new(s)
                        .file_name()
                        .map(|p| p.to_string_lossy().to_string())
                })
                .filter(|s| !s.starts_with('.'))
                .collect::<Vec<_>>();
            dirs.sort();
            Ok(json!(dirs))
        }
        "getProjectBranches" => {
            let Some(p) = db.project(arg(args, 0))? else {
                return Ok(json!([]));
            };
            let output = git::git(
                text(&p, "repoPath"),
                "for-each-ref --format='%(refname:short)' refs/heads refs/remotes",
                p["sshHost"].as_str(),
            )?;
            let mut branches = Vec::new();
            for line in output.lines() {
                let branch = line.strip_prefix("origin/").unwrap_or(line);
                if branch != "HEAD" && !branches.iter().any(|s| s == branch) {
                    branches.push(branch.to_string());
                }
            }
            Ok(json!(branches))
        }
        "scanAndRegisterProjects" => {
            scan_projects(app, arg(args, 0), args.get(1).and_then(Value::as_str))
        }
        "syncRegisteredProjectWorktrees" => sync_worktrees(app),
        "getTaskAiSessions"
        | "getTaskAiSessionDetail"
        | "getTaskLiveAiSessions"
        | "getRunningAgentPanes"
        | "getTaskAgentCallGraph" => crate::ai::project_dispatch(app, method, args.to_vec()),
        _ if method.contains("Hooks") => project_hooks(app, method, args),
        _ => Err(format!("알 수 없는 프로젝트 메서드: {method}")),
    }
}
fn expand_path(path: &str, host: Option<&str>) -> String {
    if let Some(suffix) = path.strip_prefix('~') {
        if host.is_some() {
            format!("\"$HOME\"{}", git::quote(suffix))
        } else {
            git::quote(&format!(
                "{}{suffix}",
                dirs::home_dir().unwrap_or_default().to_string_lossy()
            ))
        }
    } else {
        git::quote(path)
    }
}
fn read_ssh_hosts(path: &Path, seen: &mut HashSet<std::path::PathBuf>, hosts: &mut Vec<String>) {
    if !seen.insert(path.to_path_buf()) {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else {
            continue;
        };
        if key.eq_ignore_ascii_case("host") {
            for host in words {
                if !host.contains(['*', '?', '!']) && !hosts.iter().any(|h| h == host) {
                    hosts.push(host.into());
                }
            }
        } else if key.eq_ignore_ascii_case("include") {
            for include in words {
                let path = if include.starts_with('~') {
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(include.trim_start_matches("~/"))
                } else {
                    let p = Path::new(include);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        path.parent().unwrap_or(Path::new(".")).join(p)
                    }
                };
                if include.contains('*') {
                    if let Some(parent) = path.parent() {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            for entry in entries.flatten() {
                                read_ssh_hosts(&entry.path(), seen, hosts);
                            }
                        }
                    }
                } else {
                    read_ssh_hosts(&path, seen, hosts);
                }
            }
        }
    }
}
fn default_branch(repo: &str, host: Option<&str>) -> String {
    let remote = git::git(repo, "symbolic-ref --short refs/remotes/origin/HEAD", host)
        .ok()
        .map(|s| s.trim().trim_start_matches("origin/").to_string())
        .filter(|s| !s.is_empty());
    if let Some(branch) = remote {
        return branch;
    }
    for branch in ["main", "master"] {
        if git::git(
            repo,
            &format!("show-ref --verify --quiet refs/heads/{branch}"),
            host,
        )
        .is_ok()
        {
            return branch.into();
        }
    }
    git::git(repo, "branch --show-current", host)
        .map(|s| s.trim().into())
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or("main".into())
}
fn unique_name(name: &str, path: &str, projects: &[Value], host: Option<&str>) -> Option<String> {
    let base = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let parent = Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .unwrap_or_default()
        .to_string_lossy();
    [
        name.to_string(),
        base.to_string(),
        format!("{parent}/{base}"),
    ]
    .into_iter()
    .find(|candidate| {
        !projects
            .iter()
            .any(|p| text(p, "name") == candidate && p["sshHost"].as_str() == host)
    })
}
fn project_color(name: &str) -> &'static str {
    let hash = name
        .encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    [
        "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD", "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
    ][hash.rem_euclid(8) as usize]
}
fn register_project(app: &AppHandle, name: &str, path: &str, host: Option<&str>) -> Result<Value> {
    let host = host.filter(|s| !s.is_empty());
    if name.is_empty() || path.is_empty() {
        return Ok(json!({"success":false,"error":"이름과 경로는 필수입니다."}));
    }
    if git::git(path, "rev-parse --git-dir", host).is_err() {
        return Ok(json!({"success":false,"error":"유효한 git 저장소가 아닙니다."}));
    }
    let common = git::git(
        path,
        "rev-parse --path-format=absolute --git-common-dir",
        host,
    )?;
    let root = Path::new(common.trim())
        .parent()
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .to_string();
    let db = app.state::<Database>();
    let projects = db.projects()?;
    if projects
        .iter()
        .any(|p| text(p, "repoPath") == root && p["sshHost"].as_str() == host)
    {
        return Ok(json!({"success":false,"error":"이미 등록된 프로젝트입니다."}));
    }
    let Some(name) = unique_name(name, &root, &projects, host) else {
        return Ok(
            json!({"success":false,"error":"같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다."}),
        );
    };
    let saved_color = git::read(&git::join(&root, ".kanvibe/project.json"), host)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|s| s["projectColor"].as_str().map(str::to_string));
    let project=db.save_project(&json!({"name":name,"repoPath":root,"defaultBranch":default_branch(&root,host),"sshHost":host,"isWorktree":false,"color":saved_color.unwrap_or_else(||project_color(&name).into())}))?;
    git::persist_color(&project)?;
    if let Err(error) = ensure_root_task(app, &project) {
        db.lock()?
            .execute("DELETE FROM projects WHERE id=?1", [text(&project, "id")])
            .map_err(|e| e.to_string())?;
        return Ok(json!({"success":false,"error":error}));
    }
    schedule_project_icon(app, &project);
    board_update(app);
    Ok(json!({"success":true,"project":project}))
}
fn schedule_project_icon(app: &AppHandle, project: &Value) {
    if !project["iconDataUrl"].is_null() {
        return;
    }
    static PENDING: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    let project_id = text(project, "id").to_string();
    let pending = PENDING.get_or_init(Default::default);
    if let Ok(mut pending) = pending.lock() {
        if pending
            .get(&project_id)
            .is_some_and(|started| started.elapsed() < std::time::Duration::from_secs(600))
        {
            return;
        }
        pending.insert(project_id.clone(), std::time::Instant::now());
    } else {
        return;
    }
    let project = project.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(icon) = git::project_icon(&project) {
            let db = app.state::<Database>();
            let updated=db.lock().and_then(|connection|connection.execute("UPDATE projects SET icon_data_url=?1 WHERE id=?2 AND icon_data_url IS NULL",params![icon,project_id]).map_err(|e|e.to_string()));
            if matches!(updated,Ok(n)if n>0) {
                board_update(&app);
            }
        }
    });
}

fn shared_state(path: &str, filename: &str, host: Option<&str>) -> Value {
    git::read(&git::join(path, filename), host)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}
fn ensure_root_task(app: &AppHandle, project: &Value) -> Result<(Value, bool)> {
    let (task, changed) = reconcile_root_task(&app.state::<Database>(), project)?;
    if changed {
        install_task_hooks(app, &task, None);
    }
    Ok((task, changed))
}
fn reconcile_root_task(db: &Database, project: &Value) -> Result<(Value, bool)> {
    let project_id = text(project, "id");
    let branch = text(project, "defaultBranch");
    let root = text(project, "repoPath");
    let host = project["sshHost"].as_str();
    let trees = git::worktrees(project)?;
    let working = trees
        .iter()
        .find(|w| text(w, "branch") == branch)
        .map(|w| text(w, "path"))
        .unwrap_or(root);
    let tasks = db.tasks()?;
    let existing = tasks
        .iter()
        .find(|t| text(t, "projectId") == project_id && text(t, "branchName") == branch);
    let orphan = tasks.iter().find(|t| {
        t["projectId"].is_null()
            && text(t, "branchName") == branch
            && text(t, "worktreePath") == working
            && t["sshHost"] == project["sshHost"]
    });
    let mut task=existing.or(orphan).cloned().unwrap_or_else(||json!({"title":branch,"branchName":branch,"projectId":project_id,"baseBranch":branch,"worktreePath":working,"sshHost":project["sshHost"],"status":"todo"}));
    let original = task.clone();
    task["projectId"] = project["id"].clone();
    task["worktreePath"] = json!(working);
    task["baseBranch"] = json!(branch);
    task["sshHost"] = project["sshHost"].clone();
    let state = shared_state(working, ".kanvibe/status.json", host);
    if STATUSES.contains(&text(&state, "status")) {
        task["status"] = state["status"].clone();
    }
    let metadata = shared_state(working, ".kanvibe/task.json", host);
    if let Some(description) = metadata.get("description") {
        task["description"] = description.clone();
    }
    let project_state = shared_state(root, ".kanvibe/project.json", host);
    if let Some(priority) = project_state.get("priority") {
        task["priority"] = priority.clone();
    }
    if existing.is_none() && text(&task, "sessionType").is_empty() && host.is_none() {
        task["sessionType"] = settings(db, "getDefaultSessionType", &[])?;
        task["sessionName"] = json!(git::session_name(root, branch));
    }
    let changed = task != original || existing.is_none();
    if changed {
        task = db.save_task(&task)?;
        if let Err(error) = git::persist_task(&task, Some(project), false) {
            eprintln!("기본 작업 상태 파일 저장 실패: {error}");
        }
    }

    Ok((task, changed))
}
fn scan_projects(app: &AppHandle, path: &str, host: Option<&str>) -> Result<Value> {
    let mut result = json!({"registered":[],"skipped":[],"errors":[],"worktreeTasks":[],"registeredWorktrees":[],"hooksSetup":[]});
    let mut candidates = Vec::new();
    if host.is_some() {
        let output = git::run(
            &format!(
                "find {} -maxdepth 5 -name .git -prune",
                expand_path(path, host)
            ),
            host,
        )?;
        for line in output.lines() {
            if let Some(parent) = Path::new(line).parent() {
                candidates.push(parent.to_string_lossy().to_string());
            }
        }
    } else {
        let root = if let Some(suffix) = path.strip_prefix('~') {
            format!(
                "{}{}",
                dirs::home_dir().unwrap_or_default().to_string_lossy(),
                suffix
            )
        } else {
            path.into()
        };
        for entry in walkdir::WalkDir::new(root)
            .max_depth(6)
            .into_iter()
            .filter_entry(|e| {
                e.file_name() != "node_modules"
                    && e.file_name() != "target"
                    && e.file_name() != ".git"
            })
            .filter_map(std::result::Result::ok)
        {
            if entry.file_type().is_dir() && entry.path().join(".git").exists() {
                candidates.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    let mut roots = HashSet::new();
    for path in candidates {
        if !git::git(&path, "rev-parse --show-superproject-working-tree", host)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            continue;
        }
        let common = git::git(
            &path,
            "rev-parse --path-format=absolute --git-common-dir",
            host,
        )
        .unwrap_or_default();
        let root = Path::new(common.trim())
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path.clone());
        if !roots.insert(root.clone()) {
            continue;
        }
        let name = Path::new(&root)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match register_project(app, &name, &root, host) {
            Ok(v) if v["success"] == true => {
                result["registered"]
                    .as_array_mut()
                    .unwrap()
                    .push(v["project"]["name"].clone());
            }
            Ok(v) => {
                if text(&v, "error") == "이미 등록된 프로젝트입니다." {
                    result["skipped"].as_array_mut().unwrap().push(json!(name));
                } else {
                    result["errors"]
                        .as_array_mut()
                        .unwrap()
                        .push(v["error"].clone());
                }
            }
            Err(e) => result["errors"]
                .as_array_mut()
                .unwrap()
                .push(json!(format!("{name}: {e}"))),
        }
    }
    let synced = sync_worktrees(app)?;
    for k in ["worktreeTasks", "registeredWorktrees", "hooksSetup"] {
        result[k] = synced[k].clone();
    }
    if let Some(errors) = synced["errors"].as_array() {
        result["errors"]
            .as_array_mut()
            .unwrap()
            .extend(errors.iter().cloned());
    }
    board_update(app);
    Ok(result)
}
fn project_hooks(app: &AppHandle, method: &str, args: &[Value]) -> Result<Value> {
    let db = app.state::<Database>();
    let is_project = method.contains("Project");
    let provider = if method.contains("Gemini") {
        "gemini"
    } else if method.contains("Codex") {
        "codex"
    } else if method.contains("OpenCode") {
        "opencode"
    } else {
        "claude"
    };
    let (task, project) = if is_project {
        let Some(project) = db.project(arg(args, 0))? else {
            return Ok(Value::Null);
        };
        let task = db.tasks()?.into_iter().find(|t| {
            t["projectId"] == project["id"] && t["branchName"] == project["defaultBranch"]
        });
        (task, Some(project))
    } else {
        let task = db.task(arg(args, 0))?;
        let project = if let Some(t) = task.as_ref() {
            project_for_task(&db, t)?
        } else {
            None
        };
        (task, project)
    };
    let Some(task) = task else {
        return Ok(Value::Null);
    };
    let root = task["worktreePath"]
        .as_str()
        .or_else(|| project.as_ref().and_then(|p| p["repoPath"].as_str()))
        .ok_or("프로젝트 경로가 없습니다")?;
    let host = task["sshHost"]
        .as_str()
        .or_else(|| project.as_ref().and_then(|p| p["sshHost"].as_str()));
    if method.starts_with("install") {
        match git::install_hooks(root, text(&task, "id"), host, Some(provider)) {
            Ok(_) => Ok(
                json!({"success":true,"status":git::hooks_status(root,text(&task,"id"),host,provider)?}),
            ),
            Err(e) => Ok(json!({"success":false,"error":e})),
        }
    } else {
        git::hooks_status(root, text(&task, "id"), host, provider)
    }
}
fn hooks(app: &AppHandle, method: &str, args: &[Value]) -> Result<Value> {
    let input = args.first().cloned().unwrap_or(Value::Null);
    if method == "startHookTask" {
        if text(&input, "title").is_empty() {
            return Ok(json!({"success":false,"error":"title은 필수입니다.","status":400}));
        }
        let task = create_task(app, &input)?;
        return Ok(
            json!({"success":true,"data":{"id":task["id"],"status":task["status"],"sessionName":task["sessionName"]}}),
        );
    }
    if method != "updateHookTaskStatus" {
        return Err(format!("알 수 없는 hooks 메서드: {method}"));
    }
    let id = text(&input, "taskId");
    let status = text(&input, "status").to_lowercase();
    if id.is_empty() || status.is_empty() {
        return Ok(json!({"success":false,"error":"taskId, status는 필수입니다.","status":400}));
    }
    if validate_status(&status).is_err() {
        return Ok(
            json!({"success":false,"error":format!("유효하지 않은 상태입니다: {status}"),"status":400}),
        );
    }
    let db = app.state::<Database>();
    let Some(mut task) = db.task(id)? else {
        let _=app.emit("kanvibe:board-event",json!({"type":"hook-status-target-missing","taskId":id,"requestedStatus":status,"reason":"task-not-found"}));
        return Ok(
            json!({"success":false,"error":format!("작업을 찾을 수 없습니다: {id}"),"status":404}),
        );
    };
    let project = project_for_task(&db, &task)?;
    let name = project
        .as_ref()
        .map(|p| text(p, "name"))
        .unwrap_or("Unknown project");
    task["status"] = json!(status);
    let saved = save_task(app, &task, false)?;
    let event = json!({"type":"task-status-changed","projectName":name,"branchName":saved["branchName"],"taskTitle":saved["title"],"description":saved["description"],"newStatus":status,"taskId":saved["id"]});
    let _ = app.emit("kanvibe:board-event", event);
    Ok(
        json!({"success":true,"data":{"id":saved["id"],"status":saved["status"],"branchName":saved["branchName"],"projectName":name}}),
    )
}

pub fn sync_worktrees(app: &AppHandle) -> Result<Value> {
    let db = app.state::<Database>();
    let mut result = json!({"worktreeTasks":[],"registeredWorktrees":[],"hooksSetup":[],"errors":[],"changed":false});
    for mut project in db.projects()? {
        let synced = (|| -> Result<()> {
            let host = project["sshHost"].as_str().map(str::to_string);
            let host = host.as_deref();
            let shared = git::sync_project_state(&db, &project)?;
            if let Some(color) = shared["projectColor"].as_str() {
                if color.len() == 7
                    && color.starts_with('#')
                    && color[1..].chars().all(|c| c.is_ascii_hexdigit())
                    && text(&project, "color") != color
                {
                    let c = db.lock()?;
                    let affected=c.execute("UPDATE projects SET color=?1 WHERE id=?2 AND (color=?3 OR (color IS NULL AND ?3 IS NULL))",params![color,text(&project,"id"),nullable(&project,"color")]).map_err(|e|e.to_string())?;
                    if affected > 0 {
                        project["color"] = json!(color);
                        result["changed"] = json!(true);
                    }
                }
            }
            let (_, root_changed) = ensure_root_task(app, &project)?;
            if root_changed {
                result["changed"] = json!(true);
            }
            for tree in git::worktrees(&project)? {
                let branch = text(&tree, "branch");
                let path = text(&tree, "path");
                if branch.is_empty() || tree["isBare"] == true {
                    continue;
                }
                let tasks = db.tasks()?;
                let existing = tasks
                    .iter()
                    .find(|t| t["projectId"] == project["id"] && text(t, "branchName") == branch);
                let orphan = tasks.iter().find(|t| {
                    t["projectId"].is_null()
                        && text(t, "branchName") == branch
                        && text(t, "worktreePath") == path
                        && t["sshHost"] == project["sshHost"]
                });
                let is_new = existing.is_none();
                let mut task = existing
                    .or(orphan)
                    .cloned()
                    .unwrap_or(json!({"title":branch,"branchName":branch,"status":"todo"}));
                let original = task.clone();
                task["projectId"] = project["id"].clone();
                task["worktreePath"] = json!(path);
                task["sshHost"] = project["sshHost"].clone();
                task["baseBranch"] = project["defaultBranch"].clone();
                let state = shared_state(path, ".kanvibe/status.json", host);
                if STATUSES.contains(&text(&state, "status")) {
                    task["status"] = state["status"].clone();
                }
                let metadata = shared_state(path, ".kanvibe/task.json", host);
                for key in ["description", "priority"] {
                    if key == "priority" && branch == text(&project, "defaultBranch") {
                        continue;
                    }
                    if let Some(v) = metadata.get(key) {
                        task[key] = v.clone();
                    }
                }
                if is_new && text(&task, "sessionType").is_empty() {
                    let session = git::session_name(text(&project, "repoPath"), branch);
                    if git::run(
                        &format!("tmux has-session -t {}", git::quote(&session)),
                        host,
                    )
                    .is_ok()
                    {
                        task["sessionType"] = json!("tmux");
                        task["sessionName"] = json!(session);
                    }
                }
                if task != original || is_new {
                    let saved = db.save_task(&task)?;
                    git::persist_task(&saved, Some(&project), false)?;
                    if let Err(error) = git::install_hooks(path, text(&saved, "id"), host, None) {
                        result["errors"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(format!("{branch} hooks 설정 실패: {error}")));
                    }
                    result["changed"] = json!(true);
                    if is_new {
                        result["worktreeTasks"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(branch));
                        result["registeredWorktrees"].as_array_mut().unwrap().push(json!({"taskId":saved["id"],"projectName":project["name"],"branchName":branch,"worktreePath":path,"sshHost":project["sshHost"]}));
                    }
                }
            }
            git::sync_project_state(&db, &project)?;
            if project["iconDataUrl"].is_null() {
                if let Some(icon) = git::project_icon(&project) {
                    project["iconDataUrl"] = json!(icon);
                    db.save_project(&project)?;
                    result["changed"] = json!(true);
                }
            }
            Ok(())
        })();
        if let Err(error) = synced {
            result["errors"].as_array_mut().unwrap().push(json!(format!(
                "{} worktree 스캔 실패: {error}",
                text(&project, "name")
            )));
        }
    }
    if result["changed"] == true {
        board_update(app);
    }
    Ok(result)
}
pub fn sync_pull_requests(app: &AppHandle) -> Result<Value> {
    let db = app.state::<Database>();
    let mut result =
        json!({"updatedTaskIds":[],"mergeEventKeys":[],"mergedPullRequests":[],"failures":[]});
    static EMITTED: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
        std::sync::OnceLock::new();
    let emitted = EMITTED.get_or_init(Default::default);
    for mut task in db
        .tasks()?
        .into_iter()
        .filter(|t| text(t, "status") != "done" && !text(t, "branchName").is_empty())
    {
        let project = project_for_task(&db, &task)?;
        if project
            .as_ref()
            .is_some_and(|p| p["defaultBranch"] == task["branchName"])
        {
            continue;
        }
        let root = project
            .as_ref()
            .map(|p| text(p, "repoPath"))
            .unwrap_or_else(|| text(&task, "worktreePath"));
        let host = task["sshHost"]
            .as_str()
            .or_else(|| project.as_ref().and_then(|p| p["sshHost"].as_str()));
        let command = format!(
            "cd {} && gh pr list --head {} --state all --json url,state,mergedAt,updatedAt",
            git::quote(root),
            git::quote(text(&task, "branchName"))
        );
        match git::run(&command, host) {
            Ok(output) => {
                let mut prs: Vec<Value> = serde_json::from_str(&output).unwrap_or_default();
                prs.sort_by(|a, b| text(b, "updatedAt").cmp(text(a, "updatedAt")));
                if let Some(pr) = prs.first().filter(|p| !text(p, "url").is_empty()) {
                    if task["prUrl"] != pr["url"] {
                        task["prUrl"] = pr["url"].clone();
                        db.save_task(&task)?;
                        result["updatedTaskIds"]
                            .as_array_mut()
                            .unwrap()
                            .push(task["id"].clone());
                    }
                    if text(pr, "state") == "MERGED" && !text(pr, "mergedAt").is_empty() {
                        let key = format!(
                            "{}:{}:{}",
                            text(&task, "id"),
                            text(pr, "url"),
                            text(pr, "mergedAt")
                        );
                        if emitted
                            .lock()
                            .map_err(|e| e.to_string())?
                            .insert(key.clone())
                        {
                            result["mergeEventKeys"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!(key));
                            result["mergedPullRequests"].as_array_mut().unwrap().push(json!({"taskId":task["id"],"taskTitle":task["title"],"branchName":task["branchName"],"prUrl":pr["url"],"mergedAt":pr["mergedAt"]}));
                        }
                    }
                }
            }
            Err(error) => {
                if !error.contains("not found") && !error.contains("No such file or directory") {
                    result["failures"].as_array_mut().unwrap().push(json!({"operation":"pull-request-sync","target":task["title"],"reason":error,"taskId":task["id"],"branchName":task["branchName"],"sshHost":task["sshHost"]}));
                }
            }
        }
    }
    if result["mergedPullRequests"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
    {
        let _=app.emit("kanvibe:board-event",json!({"type":"task-pr-merged-detected-batch","mergedPullRequests":result["mergedPullRequests"]}));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("app.db")).unwrap();
        (dir, db)
    }
    #[test]
    fn root_only_shared_changes_report_changed_without_adding_tasks() {
        let (dir, db) = db();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let root = root.to_str().unwrap();
        git::git(root, "init -b main", None).unwrap();
        let project = db.save_project(&json!({"repoPath":root,"name":"repo","defaultBranch":"main","iconDataUrl":"existing-icon"})).unwrap();
        let (task, changed) = reconcile_root_task(&db, &project).unwrap();
        assert!(changed);
        assert!(!reconcile_root_task(&db, &project).unwrap().1);
        for (file, key, value) in [
            ("status.json", "status", "progress"),
            ("task.json", "description", "external description"),
            ("project.json", "priority", "high"),
        ] {
            git::write(
                &git::join(root, &format!(".kanvibe/{file}")),
                &json!({key:value}).to_string(),
                None,
                false,
            )
            .unwrap();
            let (updated, changed) = reconcile_root_task(&db, &project).unwrap();
            assert!(changed, "{key} must trigger board refresh");
            assert_eq!(updated[key], value);
            assert_eq!(updated["id"], task["id"]);
            assert_eq!(db.tasks().unwrap().len(), 1);
            assert!(!reconcile_root_task(&db, &project).unwrap().1);
        }
    }
    #[test]
    fn settings_defaults_and_restart_persistence_match_renderer_contracts() {
        let (dir, db) = db();
        assert_eq!(settings(&db, "getDefaultSessionType", &[]).unwrap(), "tmux");
        assert_eq!(
            settings(&db, "getNotificationSettings", &[]).unwrap(),
            json!({"isEnabled":true,"enabledStatuses":["progress","pending","review"]})
        );
        assert_eq!(settings(&db, "getVimModeEnabled", &[]).unwrap(), true);
        settings(&db, "dismissSidebarHint", &[]).unwrap();
        settings(&db, "setDefaultSessionType", &[json!("terminal")]).unwrap();
        drop(db);
        let reopened = Database::open(&dir.path().join("app.db")).unwrap();
        assert_eq!(
            settings(&reopened, "getSidebarHintDismissed", &[]).unwrap(),
            true
        );
        assert_eq!(
            settings(&reopened, "getDefaultSessionType", &[]).unwrap(),
            "terminal"
        );
    }
    #[test]
    fn shortcut_legacy_override_migrates_without_discarding_defaults() {
        let (_, db) = db();
        db.set_setting("task_search_shortcut", "Meta+Shift+K")
            .unwrap();
        let bindings = settings(&db, "getShortcutBindings", &[]).unwrap();
        assert_eq!(bindings["taskSearch"], "Mod+Shift+K");
        assert_eq!(bindings["createTask"], "Mod+N");
        assert!(db.setting("shortcut_bindings").unwrap().is_some());
    }
    #[test]
    fn pane_layout_project_override_and_global_fallback() {
        let (_, db) = db();
        let project = db
            .save_project(&json!({"name":"p","repoPath":"/repo"}))
            .unwrap();
        let global=pane_layout(&db,"savePaneLayout",&[json!({"isGlobal":true,"layoutType":"single","panes":[{"position":0,"command":"echo hello"}]})]).unwrap();
        let local = pane_layout(
            &db,
            "savePaneLayout",
            &[json!({"projectId":project["id"],"layoutType":"quad","panes":[]})],
        )
        .unwrap();
        assert_eq!(
            pane_layout(&db, "getEffectivePaneLayout", &[project["id"].clone()]).unwrap()["id"],
            local["id"]
        );
        pane_layout(&db, "deletePaneLayout", &[local["id"].clone()]).unwrap();
        assert_eq!(
            pane_layout(&db, "getEffectivePaneLayout", &[project["id"].clone()]).unwrap()["id"],
            global["id"]
        );
        assert!(pane_layout(
            &db,
            "savePaneLayout",
            &[json!({"layoutType":"unsupported","panes":[]})]
        )
        .is_err());
    }
    #[test]
    fn sort_preference_deduplicates_and_rejects_unknown_fields() {
        let (_, db) = db();
        db.set_setting("board_sort_preference",r#"{"mode":"manual","keys":[{"field":"priority","direction":"desc"},{"field":"priority","direction":"asc"},{"field":"unknown"},{"field":"title","direction":"invalid"}]}"#).unwrap();
        assert_eq!(
            settings(&db, "getBoardSortPreference", &[]).unwrap(),
            json!({"keys":[{"field":"priority","direction":"desc"},{"field":"title","direction":"asc"}]})
        );
    }
    #[test]
    fn known_state_validation_rejects_unknown_new_writes() {
        assert!(validate_status("pending").is_ok());
        assert!(validate_status("archived").is_err());
    }
}
