use crate::database::{text, Database};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Mutex};
use tauri::{AppHandle, Manager};

const AUTH_FAILURE: &str = "task CLI 인증에 실패했습니다. KanVibe에서 hooks를 다시 설치하세요.";
const TOKEN_CHECK_FAILURE: &str = "task CLI 인증을 확인하지 못했습니다. 잠시 뒤 다시 시도하세요.";
const DIRTY_WORKTREE_PREFIX: &str = "커밋하지 않은 변경이 있어 삭제하지 않았습니다:";
static CLI_CREATION: Mutex<()> = Mutex::new(());
type Provider<'a> = &'a dyn Fn(&str, &str, Vec<Value>) -> Result<Value, String>;

#[derive(Debug)]
struct CliFailure {
    status: u16,
    message: String,
}
impl CliFailure {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}
impl From<String> for CliFailure {
    fn from(message: String) -> Self {
        Self::new(500, message)
    }
}

pub fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    body: &Value,
    authorization: Option<&str>,
    source_hosts: Option<&HashSet<String>>,
) -> Result<Option<(u16, Value)>, String> {
    let Some(success_status) = route_status(method, path) else {
        return Ok(None);
    };
    if !body.is_object() {
        return Ok(Some((
            400,
            json!({"success":false,"error":"JSON 객체 본문이 필요합니다"}),
        )));
    }
    let db = app.state::<Database>();
    let result = execute(
        &db,
        path,
        body,
        authorization,
        source_hosts,
        &|namespace, method, args| crate::core::dispatch(app, namespace, method, args),
    );
    Ok(Some(match result {
        Ok(data) => (success_status, json!({"success":true,"data":data})),
        Err(failure) => (
            failure.status,
            json!({"success":false,"error":failure.message}),
        ),
    }))
}

fn route_status(method: &str, path: &str) -> Option<u16> {
    match (method, path) {
        ("POST", "/api/cli/tasks") => Some(201),
        ("POST", "/api/cli/tasks/delete" | "/api/cli/tasks/register") => Some(200),
        _ => None,
    }
}

fn execute(
    db: &Database,
    path: &str,
    body: &Value,
    authorization: Option<&str>,
    source_hosts: Option<&HashSet<String>>,
    call: Provider<'_>,
) -> Result<Value, CliFailure> {
    let (context, project) = authorize(db, body, authorization, source_hosts)?;
    match path {
        "/api/cli/tasks" => create(db, body, &context, &project, call),
        "/api/cli/tasks/delete" => delete(db, body, &context, &project, call),
        "/api/cli/tasks/register" => register(db, body, &project, call),
        _ => Err(CliFailure::new(404, "Not found")),
    }
}

fn authorize(
    db: &Database,
    body: &Value,
    authorization: Option<&str>,
    source_hosts: Option<&HashSet<String>>,
) -> Result<(Value, Value), CliFailure> {
    let id = required_string(body, "contextTaskId")?;
    let bearer = regex::Regex::new(r"(?i)^Bearer\s+(\S+)$").expect("fixed bearer expression");
    let token = authorization
        .and_then(|value| bearer.captures(value))
        .and_then(|capture| capture.get(1).map(|token| token.as_str()));
    let Some(token) = token else {
        return Err(CliFailure::new(401, AUTH_FAILURE));
    };
    let is_valid = crate::task_targets::is_valid_token(db, id, token).map_err(|error| {
        eprintln!("[task-cli] CLI 토큰을 확인하지 못했습니다: {error}");
        CliFailure::new(500, TOKEN_CHECK_FAILURE)
    })?;
    if !is_valid {
        return Err(CliFailure::new(401, AUTH_FAILURE));
    }
    let context = db
        .task(id)?
        .ok_or_else(|| CliFailure::new(404, format!("task를 찾을 수 없습니다: {id}")))?;
    let project = if let Some(project_id) = context["projectId"].as_str() {
        db.project(project_id)?
    } else {
        None
    };
    if !crate::hook_transport::task_in_scope(&context, project.as_ref(), source_hosts) {
        return Err(CliFailure::new(401, AUTH_FAILURE));
    }
    let project = project.ok_or_else(|| {
        CliFailure::new(
            409,
            "프로젝트에 연결되지 않은 task에서는 task를 만들거나 지울 수 없습니다.",
        )
    })?;
    Ok((context, project))
}

fn create(
    db: &Database,
    body: &Value,
    context: &Value,
    project: &Value,
    call: Provider<'_>,
) -> Result<Value, CliFailure> {
    let branch = git_ref(body, "branchName")?;
    let title = optional_string(body, "title")?.unwrap_or(branch);
    let requested_kind = optional_string(body, "sessionType")?;
    if requested_kind.is_some_and(|kind| !["tmux", "zellij", "herdr", "terminal"].contains(&kind)) {
        return Err(CliFailure::new(
            400,
            format!(
                "sessionType 값이 올바르지 않습니다: {}",
                requested_kind.unwrap()
            ),
        ));
    }
    let (base, inherited_kind) = if let Some(id) = optional_string(body, "fromTaskId")? {
        let task = project_task(
            db,
            project,
            |task| text(task, "id") == id,
            format!("기준 task를 찾을 수 없습니다: {id}"),
        )?;
        let base = text(&task, "branchName");
        if !crate::git::is_safe_git_ref(base) {
            return Err(CliFailure::new(
                409,
                "기준 task에 기준으로 쓸 수 있는 브랜치가 없습니다.",
            ));
        }
        (
            base.to_owned(),
            task["sessionType"].as_str().map(str::to_owned),
        )
    } else {
        (
            git_ref(body, "baseBranch")?.to_owned(),
            context["sessionType"].as_str().map(str::to_owned),
        )
    };
    let kind = requested_kind
        .or(inherited_kind.as_deref())
        .unwrap_or("terminal");
    let created = {
        // 중복 확인과 생성만 묶는다. 상대 client 전파까지 잠그면 서로의 등록 요청을 기다릴 수 있다.
        let _guard = CLI_CREATION.lock().map_err(|error| error.to_string())?;
        if find_branch(db, text(project, "id"), branch)?.is_some() {
            return Err(CliFailure::new(
                409,
                format!("이미 같은 브랜치의 task가 있습니다: {branch}"),
            ));
        }
        call(
            "kanban",
            "createTask",
            vec![
                json!({"title":title,"branchName":branch,"baseBranch":base,"sessionType":kind,"projectId":project["id"]}),
            ],
        )?
    };
    let peers = crate::task_targets::propagate_registration(
        text(project, "repoPath"),
        branch,
        project["sshHost"].as_str(),
    );
    Ok(
        json!({"id":created["id"],"projectId":created["projectId"],"branchName":created["branchName"],"baseBranch":created["baseBranch"],"worktreePath":created["worktreePath"],"sessionType":created["sessionType"],"status":created["status"],"peers":peers}),
    )
}

fn delete(
    db: &Database,
    body: &Value,
    context: &Value,
    project: &Value,
    call: Provider<'_>,
) -> Result<Value, CliFailure> {
    if body["confirmed"] != true {
        return Err(CliFailure::new(
            400,
            "삭제를 확인하는 confirmed: true가 필요합니다.",
        ));
    }
    let target = if body.get("branchName").is_none() {
        context.clone()
    } else {
        let branch = git_ref(body, "branchName")?;
        project_task(
            db,
            project,
            |task| text(task, "branchName") == branch,
            format!("{branch} 브랜치의 task를 찾을 수 없습니다."),
        )?
    };
    let deletion=call("kanban","deleteTaskAcrossTargets",vec![target["id"].clone(),json!({"shouldRejectDirtyWorktree":body["force"]!=true,"shouldPropagateToPeers":body["propagateToPeers"]!=false})]).map_err(|error| {
        if error.starts_with(DIRTY_WORKTREE_PREFIX) {CliFailure::new(409,format!("{error} 그래도 지우려면 --force를 붙이세요."))} else {error.into()}
    })?;
    if deletion["isDeleted"] != true {
        return Err(CliFailure::new(
            404,
            format!("task를 찾을 수 없습니다: {}", text(&target, "id")),
        ));
    }
    Ok(
        json!({"id":target["id"],"branchName":target["branchName"],"deleted":true,"peers":deletion["peerResults"]}),
    )
}

fn register(
    db: &Database,
    body: &Value,
    project: &Value,
    call: Provider<'_>,
) -> Result<Value, CliFailure> {
    let branch = git_ref(body, "branchName")?;
    if let Some(task) = find_branch(db, text(project, "id"), branch)? {
        return Ok(json!({"id":task["id"],"branchName":branch}));
    }
    call(
        "project",
        "syncRegisteredProjectWorktrees",
        vec![json!([project])],
    )?;
    let task = find_branch(db, text(project, "id"), branch)?.ok_or_else(|| {
        CliFailure::new(
            404,
            format!("프로젝트에서 {branch} 브랜치의 worktree를 찾지 못했습니다."),
        )
    })?;
    Ok(json!({"id":task["id"],"branchName":branch}))
}

fn find_branch(db: &Database, project: &str, branch: &str) -> Result<Option<Value>, String> {
    Ok(db
        .tasks()?
        .into_iter()
        .find(|task| text(task, "projectId") == project && text(task, "branchName") == branch))
}
fn project_task(
    db: &Database,
    project: &Value,
    predicate: impl Fn(&Value) -> bool,
    message: String,
) -> Result<Value, CliFailure> {
    db.tasks()?
        .into_iter()
        .find(|task| text(task, "projectId") == text(project, "id") && predicate(task))
        .ok_or_else(|| CliFailure::new(404, message))
}
fn optional_string<'a>(body: &'a Value, key: &str) -> Result<Option<&'a str>, CliFailure> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok((!value.trim().is_empty()).then(|| value.trim())),
        _ => Err(CliFailure::new(
            400,
            format!("{key}는 문자열이어야 합니다."),
        )),
    }
}
fn required_string<'a>(body: &'a Value, key: &str) -> Result<&'a str, CliFailure> {
    optional_string(body, key)?.ok_or_else(|| CliFailure::new(400, format!("{key}는 필수입니다.")))
}
fn git_ref<'a>(body: &'a Value, key: &str) -> Result<&'a str, CliFailure> {
    let value = required_string(body, key)?;
    if !crate::git::is_safe_git_ref(value) {
        return Err(CliFailure::new(
            400,
            format!("{key}에 쓸 수 없는 문자가 있습니다: {value}"),
        ));
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn database() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().unwrap();
        let db = Database::open(&directory.path().join("cli.db")).unwrap();
        db.lock().unwrap().execute("INSERT INTO projects(id,name,repo_path) VALUES('project','Project','/fixture'),('foreign','Foreign','/foreign')", []).unwrap();
        for task in [
            json!({"id":"context","projectId":"project","branchName":"main","sessionType":"herdr"}),
            json!({"id":"base","projectId":"project","branchName":"feature/base","sessionType":"tmux"}),
            json!({"id":"foreign-task","projectId":"foreign","branchName":"secret"}),
        ] {
            db.save_task(&task).unwrap();
        }
        (directory, db)
    }

    fn authorization(db: &Database) -> String {
        format!(
            "Bearer {}",
            crate::task_targets::build_token(db, "context").unwrap()
        )
    }

    #[test]
    fn authorization_hides_task_existence_and_internal_token_failure() {
        let (_directory, db) = database();
        let body = json!({"contextTaskId":"missing","branchName":"new","baseBranch":"main"});
        let failure = execute(
            &db,
            "/api/cli/tasks",
            &body,
            Some("Bearer forged"),
            None,
            &|_, _, _| panic!("unauthorized provider call"),
        )
        .unwrap_err();
        assert_eq!(failure.status, 401);
        db.lock()
            .unwrap()
            .execute_batch("DROP TABLE app_settings")
            .unwrap();
        let failure = execute(
            &db,
            "/api/cli/tasks",
            &body,
            Some("Bearer forged"),
            None,
            &|_, _, _| panic!("unauthorized provider call"),
        )
        .unwrap_err();
        assert_eq!(failure.status, 500);
        assert_eq!(
            failure.message,
            "task CLI 인증을 확인하지 못했습니다. 잠시 뒤 다시 시도하세요."
        );
    }

    #[test]
    fn authenticated_context_is_limited_to_tunnel_host_and_requires_a_project() {
        let (_directory, db) = database();
        let auth = authorization(&db);
        db.lock()
            .unwrap()
            .execute("UPDATE projects SET ssh_host='dev' WHERE id='project'", [])
            .unwrap();
        let body = json!({"contextTaskId":"context","confirmed":true});
        let foreign_hosts = HashSet::from(["other".to_owned()]);
        assert_eq!(
            execute(
                &db,
                "/api/cli/tasks/delete",
                &body,
                Some(&auth),
                Some(&foreign_hosts),
                &|_, _, _| panic!("foreign tunnel must not mutate")
            )
            .unwrap_err()
            .status,
            401
        );
        let hosts = HashSet::from(["dev".to_owned()]);
        assert!(execute(
            &db,
            "/api/cli/tasks/delete",
            &body,
            Some(&auth),
            Some(&hosts),
            &|_, _, _| Ok(json!({"isDeleted":true,"peerResults":[]}))
        )
        .is_ok());
        db.lock()
            .unwrap()
            .execute(
                "UPDATE kanban_tasks SET project_id=NULL WHERE id='context'",
                [],
            )
            .unwrap();
        assert_eq!(
            execute(
                &db,
                "/api/cli/tasks/delete",
                &body,
                Some(&auth),
                None,
                &|_, _, _| panic!("orphan must not mutate")
            )
            .unwrap_err()
            .status,
            409
        );
        let missing_auth = format!(
            "Bearer {}",
            crate::task_targets::build_token(&db, "missing").unwrap()
        );
        assert_eq!(
            execute(
                &db,
                "/api/cli/tasks/delete",
                &json!({"contextTaskId":"missing","confirmed":true}),
                Some(&missing_auth),
                None,
                &|_, _, _| panic!("missing task must not mutate")
            )
            .unwrap_err()
            .status,
            404
        );
    }

    #[test]
    fn route_status_only_accepts_exact_post_contracts() {
        assert_eq!(route_status("POST", "/api/cli/tasks"), Some(201));
        assert_eq!(route_status("POST", "/api/cli/tasks/delete"), Some(200));
        assert_eq!(route_status("POST", "/api/cli/tasks/register"), Some(200));
        assert_eq!(route_status("GET", "/api/cli/tasks"), None);
        assert_eq!(route_status("POST", "/api/cli/tasks?other=1"), None);
    }

    #[test]
    fn create_inherits_same_project_base_and_never_calls_provider_for_invalid_input() {
        let (_directory, db) = database();
        let auth = authorization(&db);
        for (extra, status) in [
            (json!({"fromTaskId":"foreign-task"}), 404),
            (json!({"branchName":"$(touch bad)"}), 400),
            (json!({"sessionType":"unknown"}), 400),
            (json!({"branchName":"main"}), 409),
        ] {
            let mut body =
                json!({"contextTaskId":"context","branchName":"new","baseBranch":"main"});
            for (key, value) in extra.as_object().unwrap() {
                body[key] = value.clone();
            }
            let failure = execute(
                &db,
                "/api/cli/tasks",
                &body,
                Some(&auth),
                None,
                &|_, _, _| panic!("invalid input must not mutate"),
            )
            .unwrap_err();
            assert_eq!(failure.status, status);
        }
        let body = json!({"contextTaskId":"context","branchName":"new","fromTaskId":"base","title":" Custom "});
        let result=execute(&db,"/api/cli/tasks",&body,Some(&auth),None,&|namespace,method,args| {
            assert_eq!((namespace,method),("kanban","createTask"));
            assert_eq!(args[0],json!({"title":"Custom","branchName":"new","baseBranch":"feature/base","sessionType":"tmux","projectId":"project"}));
            Ok(json!({"id":"created","projectId":"project","branchName":"new","baseBranch":"feature/base","sessionType":"tmux","status":"todo"}))
        }).unwrap();
        assert_eq!(result["id"], "created");
    }

    #[test]
    fn delete_preserves_force_confirmation_project_boundary_and_peer_loop_guard() {
        let (_directory, db) = database();
        let auth = authorization(&db);
        for (body, status) in [
            (json!({"contextTaskId":"context"}), 400),
            (
                json!({"contextTaskId":"context","confirmed":true,"branchName":"secret"}),
                404,
            ),
        ] {
            assert_eq!(
                execute(
                    &db,
                    "/api/cli/tasks/delete",
                    &body,
                    Some(&auth),
                    None,
                    &|_, _, _| panic!("invalid delete")
                )
                .unwrap_err()
                .status,
                status
            );
        }
        let body = json!({"contextTaskId":"context","confirmed":true,"force":true,"propagateToPeers":false});
        let result=execute(&db,"/api/cli/tasks/delete",&body,Some(&auth),None,&|namespace,method,args| {
            assert_eq!((namespace,method),("kanban","deleteTaskAcrossTargets"));
            assert_eq!(args,json!(["context",{"shouldRejectDirtyWorktree":false,"shouldPropagateToPeers":false}]).as_array().unwrap().clone());
            Ok(json!({"isDeleted":true,"peerResults":[]}))
        }).unwrap();
        assert_eq!(
            result,
            json!({"id":"context","branchName":"main","deleted":true,"peers":[]})
        );
        let failure = execute(
            &db,
            "/api/cli/tasks/delete",
            &body,
            Some(&auth),
            None,
            &|_, _, _| Err("커밋하지 않은 변경이 있어 삭제하지 않았습니다: /fixture".into()),
        )
        .unwrap_err();
        assert_eq!(failure.status, 409);
        assert!(failure.message.ends_with("--force를 붙이세요."));
    }

    #[test]
    fn register_is_idempotent_and_synchronizes_only_authorized_project() {
        let (_directory, db) = database();
        let auth = authorization(&db);
        let body = json!({"contextTaskId":"context","branchName":"main"});
        assert_eq!(
            execute(
                &db,
                "/api/cli/tasks/register",
                &body,
                Some(&auth),
                None,
                &|_, _, _| panic!("existing branch must not sync")
            )
            .unwrap(),
            json!({"id":"context","branchName":"main"})
        );
        let body = json!({"contextTaskId":"context","branchName":"peer"});
        let result = execute(
            &db,
            "/api/cli/tasks/register",
            &body,
            Some(&auth),
            None,
            &|namespace, method, args| {
                assert_eq!(
                    (namespace, method),
                    ("project", "syncRegisteredProjectWorktrees")
                );
                assert_eq!(args[0][0]["id"], "project");
                db.save_task(&json!({"id":"peer-id","projectId":"project","branchName":"peer"}))?;
                Ok(json!({"changed":true}))
            },
        )
        .unwrap();
        assert_eq!(result, json!({"id":"peer-id","branchName":"peer"}));
    }
}
