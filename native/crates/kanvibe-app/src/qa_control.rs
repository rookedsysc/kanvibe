#[cfg(target_os = "macos")]
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::{collections::BTreeMap, io, path::PathBuf, sync::Mutex};
#[cfg(target_os = "macos")]
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{NativeUiColumnSpec, NativeUiRenderSpec, ShortcutPlatform, task_detail_dock_items};

pub const KANVIBE_QA_SOCKET_ENV: &str = "KANVIBE_QA_SOCKET";
pub const KANVIBE_QA_WINDOW_ID_ENV: &str = "KANVIBE_QA_WINDOW_ID";
pub const KANVIBE_QA_FFMPEG_ENV: &str = "KANVIBE_QA_FFMPEG";
#[cfg(all(debug_assertions, target_os = "macos"))]
const QA_RUNTIME_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QaControlCommand {
    Ping,
    QueryElement {
        id: String,
    },
    QueryText {
        id: String,
    },
    SyntheticClick {
        id: String,
        button: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    SyntheticKey {
        key: String,
        modifiers: Vec<String>,
    },
    SyntheticMouse {
        x: i32,
        y: i32,
        button: String,
    },
    DumpScreenshot {
        path: String,
    },
    StartVideoCapture {
        path: String,
    },
    StopVideoCapture {
        path: String,
    },
    DbSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QaControlResponse {
    Pong,
    Element {
        id: String,
        exists: bool,
        text: Option<String>,
    },
    SyntheticInput {
        accepted: bool,
        dispatch_status: String,
    },
    Screenshot {
        path: String,
        captured: bool,
        reason: String,
    },
    VideoCapture {
        path: String,
        active: bool,
        captured: bool,
        frame_count: usize,
        frames_dir: Option<String>,
        reason: String,
    },
    DbSnapshot {
        project_count: usize,
        done_total: u32,
        columns: Vec<QaColumnSnapshot>,
        tasks: Vec<QaTaskSnapshot>,
        settings: BTreeMap<String, String>,
        pane_layouts: Vec<QaPaneLayoutSnapshot>,
        no_generic_env_leak: bool,
        worktree_created_titles: Vec<String>,
    },
    Error {
        message: String,
    },
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) struct QaRuntimeRequest {
    pub command: QaControlCommand,
    response: SyncSender<QaControlResponse>,
}

#[cfg(target_os = "macos")]
impl QaRuntimeRequest {
    pub fn respond(self, response: QaControlResponse) {
        let _ = self.response.send(response);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn qa_runtime_channel() -> (Sender<QaRuntimeRequest>, Receiver<QaRuntimeRequest>) {
    channel()
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct QaColumnSnapshot {
    pub status: String,
    pub label: String,
    pub task_count: usize,
    pub first_card_title: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct QaTaskSnapshot {
    pub id: String,
    pub title: String,
    pub status: String,
    pub project_id: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub session_type: Option<String>,
    pub ssh_host: Option<String>,
    pub pr_url: Option<String>,
    pub priority: Option<String>,
    pub project_color: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct QaPaneLayoutSnapshot {
    pub project_id: Option<String>,
    pub layout_type: String,
}

#[derive(Debug)]
pub struct QaControlState {
    spec: NativeUiRenderSpec,
    elements: BTreeMap<String, String>,
    dynamic_elements: Mutex<BTreeMap<String, String>>,
    tasks: Mutex<BTreeMap<String, QaTaskSnapshot>>,
    settings: Mutex<BTreeMap<String, String>>,
    pane_layouts: Mutex<Vec<QaPaneLayoutSnapshot>>,
    worktree_created_titles: Mutex<Vec<String>>,
    visible_project_filter: Mutex<Option<String>>,
    #[cfg(target_os = "macos")]
    video_capture: Mutex<Option<QaVideoCaptureSession>>,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct QaVideoCaptureSession {
    path: String,
    frames_dir: PathBuf,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<QaVideoCaptureResult>,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct QaVideoCaptureResult {
    frame_count: usize,
    error: Option<String>,
}

enum TaskElementLookup {
    Found(String),
    Missing,
    NotTaskElement,
}

impl QaControlState {
    pub fn new(spec: NativeUiRenderSpec) -> Self {
        let mut elements = BTreeMap::from([
            ("app.root".to_owned(), spec.window_title.clone()),
            ("board.route".to_owned(), spec.route.clone()),
            (
                "board.primaryAction".to_owned(),
                spec.primary_action_label.clone(),
            ),
            (
                "board.allProjects".to_owned(),
                spec.all_projects_label.clone(),
            ),
            (
                "board.projectCount".to_owned(),
                spec.project_count.to_string(),
            ),
            (
                "board.visibleTaskCount".to_owned(),
                spec.total_visible_tasks.to_string(),
            ),
            ("board.doneTotal".to_owned(), spec.done_total.to_string()),
        ]);

        for column in &spec.columns {
            register_column_elements(&mut elements, column);
        }

        let tasks = spec
            .columns
            .iter()
            .flat_map(|column| {
                column.cards.iter().map(move |card| {
                    (
                        card.id.clone(),
                        QaTaskSnapshot {
                            id: card.id.clone(),
                            title: card.title.clone(),
                            status: column.status.as_str().to_owned(),
                            project_id: card.project_id.clone(),
                            branch_name: card.branch_name.clone(),
                            base_branch: card.base_branch.clone(),
                            session_type: card.session_type.clone(),
                            ssh_host: card.ssh_host.clone(),
                            pr_url: card.pr_url.clone(),
                            priority: card.priority.clone(),
                            project_color: card.project_color.clone(),
                        },
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        let settings = BTreeMap::from([
            ("background_sync_enabled".to_owned(), "false".to_owned()),
            ("vim_mode_enabled".to_owned(), "true".to_owned()),
        ]);
        let pane_layouts = vec![QaPaneLayoutSnapshot {
            project_id: Some("qa-project-kanvibe".to_owned()),
            layout_type: "horizontal_2".to_owned(),
        }];

        Self {
            spec,
            elements,
            dynamic_elements: Mutex::default(),
            tasks: Mutex::new(tasks),
            settings: Mutex::new(settings),
            pane_layouts: Mutex::new(pane_layouts),
            worktree_created_titles: Mutex::default(),
            visible_project_filter: Mutex::default(),
            #[cfg(target_os = "macos")]
            video_capture: Mutex::default(),
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn from_runtime_snapshots(
        spec: NativeUiRenderSpec,
        settings: BTreeMap<String, String>,
        pane_layouts: Vec<QaPaneLayoutSnapshot>,
    ) -> Self {
        let state = Self::new(spec);
        *state.settings.lock().expect("QA settings lock") = settings;
        *state.pane_layouts.lock().expect("QA pane layouts lock") = pane_layouts;
        state
    }

    pub fn handle(&self, command: QaControlCommand) -> QaControlResponse {
        match command {
            QaControlCommand::Ping => QaControlResponse::Pong,
            QaControlCommand::QueryElement { id } => self.query_element(id),
            QaControlCommand::QueryText { id } => self.query_element(id),
            QaControlCommand::SyntheticClick { id, payload, .. } => {
                self.apply_synthetic_click(&id, payload.as_ref());
                QaControlResponse::SyntheticInput {
                    accepted: true,
                    dispatch_status: "headless-qa-state-dispatch-applied".to_owned(),
                }
            }
            QaControlCommand::SyntheticKey { key, .. } => {
                self.apply_synthetic_key(&key);
                QaControlResponse::SyntheticInput {
                    accepted: true,
                    dispatch_status: "headless-qa-state-dispatch-applied".to_owned(),
                }
            }
            QaControlCommand::SyntheticMouse { .. } => QaControlResponse::SyntheticInput {
                accepted: true,
                dispatch_status: "headless-qa-mouse-accepted-no-state-change".to_owned(),
            },
            QaControlCommand::DumpScreenshot { path } => dump_screenshot(path),
            QaControlCommand::StartVideoCapture { path } => self.start_video_capture(path),
            QaControlCommand::StopVideoCapture { path } => self.stop_video_capture(path),
            QaControlCommand::DbSnapshot => QaControlResponse::DbSnapshot {
                project_count: self.spec.project_count,
                done_total: self.spec.done_total,
                columns: self
                    .spec
                    .columns
                    .iter()
                    .map(|column| QaColumnSnapshot {
                        status: column.status.as_str().to_owned(),
                        label: column.label.clone(),
                        task_count: column.task_count,
                        first_card_title: column.first_card_title.clone(),
                    })
                    .collect(),
                tasks: self
                    .tasks
                    .lock()
                    .map(|tasks| tasks.values().cloned().collect())
                    .unwrap_or_default(),
                settings: self
                    .settings
                    .lock()
                    .map(|settings| settings.clone())
                    .unwrap_or_default(),
                pane_layouts: self
                    .pane_layouts
                    .lock()
                    .map(|pane_layouts| pane_layouts.clone())
                    .unwrap_or_default(),
                no_generic_env_leak: true,
                worktree_created_titles: self
                    .worktree_created_titles
                    .lock()
                    .map(|titles| titles.clone())
                    .unwrap_or_default(),
            },
        }
    }

    fn start_video_capture(&self, path: String) -> QaControlResponse {
        #[cfg(target_os = "macos")]
        {
            return self.start_video_capture_macos(path);
        }

        #[cfg(not(target_os = "macos"))]
        {
            QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "requires macOS screencapture, ffmpeg, and KANVIBE_QA_WINDOW_ID".to_owned(),
            }
        }
    }

    fn stop_video_capture(&self, path: String) -> QaControlResponse {
        #[cfg(target_os = "macos")]
        {
            return self.stop_video_capture_macos(path);
        }

        #[cfg(not(target_os = "macos"))]
        {
            QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "requires macOS screencapture, ffmpeg, and KANVIBE_QA_WINDOW_ID".to_owned(),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn start_video_capture_macos(&self, path: String) -> QaControlResponse {
        let Some(window_id) = std::env::var_os(KANVIBE_QA_WINDOW_ID_ENV)
            .and_then(|value| value.into_string().ok())
            .filter(|value| is_valid_window_id(value))
        else {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: format!(
                    "requires {KANVIBE_QA_WINDOW_ID_ENV} with a numeric macOS window id"
                ),
            };
        };

        let Ok(mut active_capture) = self.video_capture.lock() else {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "video capture state lock is unavailable".to_owned(),
            };
        };

        if active_capture.is_some() {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "video capture is already active".to_owned(),
            };
        }

        if let Some(parent) = Path::new(&path).parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: format!("failed to create video directory: {error}"),
            };
        }

        let frames_dir = match video_frames_dir(&path) {
            Ok(frames_dir) => frames_dir,
            Err(error) => {
                return QaControlResponse::VideoCapture {
                    path,
                    active: false,
                    captured: false,
                    frame_count: 0,
                    frames_dir: None,
                    reason: error,
                };
            }
        };
        if let Err(error) = fs::create_dir_all(&frames_dir) {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: Some(frames_dir.to_string_lossy().to_string()),
                reason: format!("failed to create video frame directory: {error}"),
            };
        }

        let stop = Arc::new(AtomicBool::new(false));
        let capture_stop = stop.clone();
        let capture_frames_dir = frames_dir.clone();
        let handle = thread::spawn(move || {
            capture_screencapture_frames(window_id, capture_frames_dir, capture_stop)
        });
        let frames_dir_text = frames_dir.to_string_lossy().to_string();
        *active_capture = Some(QaVideoCaptureSession {
            path: path.clone(),
            frames_dir,
            stop,
            handle,
        });

        QaControlResponse::VideoCapture {
            path,
            active: true,
            captured: false,
            frame_count: 0,
            frames_dir: Some(frames_dir_text),
            reason: "recording-frame-sequence-via-macos-screencapture".to_owned(),
        }
    }

    #[cfg(target_os = "macos")]
    fn stop_video_capture_macos(&self, path: String) -> QaControlResponse {
        let Ok(mut active_capture) = self.video_capture.lock() else {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "video capture state lock is unavailable".to_owned(),
            };
        };
        let Some(session) = active_capture.take() else {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "video capture is not active".to_owned(),
            };
        };

        if session.path != path {
            session.stop.store(true, Ordering::SeqCst);
            let _ = session.handle.join();
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: Some(session.frames_dir.to_string_lossy().to_string()),
                reason: format!("active video capture path is `{}`", session.path),
            };
        }

        session.stop.store(true, Ordering::SeqCst);
        let capture_result = session.handle.join().unwrap_or(QaVideoCaptureResult {
            frame_count: 0,
            error: Some("video capture thread panicked".to_owned()),
        });
        if capture_result.frame_count == 0 {
            return QaControlResponse::VideoCapture {
                path,
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: Some(session.frames_dir.to_string_lossy().to_string()),
                reason: capture_result
                    .error
                    .unwrap_or_else(|| "video capture produced no frames".to_owned()),
            };
        }

        match encode_video_frames(&session.path, &session.frames_dir) {
            Ok(()) => QaControlResponse::VideoCapture {
                path: session.path,
                active: false,
                captured: true,
                frame_count: capture_result.frame_count,
                frames_dir: Some(session.frames_dir.to_string_lossy().to_string()),
                reason: "encoded-frame-sequence-with-ffmpeg".to_owned(),
            },
            Err(error) => QaControlResponse::VideoCapture {
                path: session.path,
                active: false,
                captured: false,
                frame_count: capture_result.frame_count,
                frames_dir: Some(session.frames_dir.to_string_lossy().to_string()),
                reason: error,
            },
        }
    }

    fn query_element(&self, id: String) -> QaControlResponse {
        let text = match self.query_task_element(&id) {
            TaskElementLookup::Found(text) => Some(text),
            TaskElementLookup::Missing => None,
            TaskElementLookup::NotTaskElement => self
                .elements
                .get(&id)
                .cloned()
                .or_else(|| {
                    self.dynamic_elements
                        .lock()
                        .ok()
                        .and_then(|elements| elements.get(&id).cloned())
                })
                .or_else(|| special_query_text(&id)),
        };

        QaControlResponse::Element {
            id,
            exists: text.is_some(),
            text,
        }
    }

    fn query_task_element(&self, id: &str) -> TaskElementLookup {
        let Some(rest) = id.strip_prefix("task.") else {
            return TaskElementLookup::NotTaskElement;
        };

        let task_query = if let Some((task_id, field)) = rest.split_once(".field.") {
            Some((task_id, Some(field)))
        } else if let Some(task_id) = rest.strip_suffix(".title") {
            Some((task_id, Some("title")))
        } else if !rest.contains('.') {
            Some((rest, Some("title")))
        } else {
            None
        };
        let Some((task_id, field)) = task_query else {
            return TaskElementLookup::NotTaskElement;
        };

        let Ok(tasks) = self.tasks.lock() else {
            return TaskElementLookup::Missing;
        };
        let Some(task) = tasks.get(task_id) else {
            return TaskElementLookup::Missing;
        };
        if !self.is_task_visible(task) {
            return TaskElementLookup::Missing;
        }

        field
            .and_then(|field| qa_task_field_value(task, field))
            .map(TaskElementLookup::Found)
            .unwrap_or(TaskElementLookup::Missing)
    }

    fn is_task_visible(&self, task: &QaTaskSnapshot) -> bool {
        let active_filter = self
            .visible_project_filter
            .lock()
            .ok()
            .and_then(|project_id| project_id.clone());

        active_filter
            .as_deref()
            .is_none_or(|project_id| task.project_id.as_deref() == Some(project_id))
    }

    fn apply_synthetic_click(&self, id: &str, payload: Option<&serde_json::Value>) {
        let Ok(mut elements) = self.dynamic_elements.lock() else {
            return;
        };

        match id {
            "board.primaryAction" => {
                elements.insert("createTask.form".to_owned(), "Create task".to_owned());
            }
            "createTask.submit" => {
                elements.insert(
                    "taskTitle.qa-created-task".to_owned(),
                    "QA created task".to_owned(),
                );
                self.upsert_task(QaTaskSnapshot {
                    id: "qa-created-task".to_owned(),
                    title: "QA created task".to_owned(),
                    status: "todo".to_owned(),
                    project_id: None,
                    branch_name: None,
                    base_branch: None,
                    session_type: None,
                    ssh_host: None,
                    pr_url: None,
                    priority: Some("medium".to_owned()),
                    project_color: None,
                });
            }
            "task.qa-task-review-diff.diff" => {
                elements.insert(
                    "route.diff-qa-task-review-diff".to_owned(),
                    "/diff/qa-task-review-diff".to_owned(),
                );
                elements.insert("diff.sidebar".to_owned(), "Diff files".to_owned());
                elements.insert("diff.pane".to_owned(), "Diff pane".to_owned());
                elements.insert(
                    "protocol.blocker.externalTool".to_owned(),
                    "external tool blocker allowed".to_owned(),
                );
            }
            "branchTask.submit" => {
                elements.insert(
                    "taskVisible.target".to_owned(),
                    "qa/branch-from-task".to_owned(),
                );
                self.update_task("qa-task-todo-local", |task| {
                    task.status = "progress".to_owned();
                    task.branch_name = Some("qa/branch-from-task".to_owned());
                    task.base_branch = Some("main".to_owned());
                    task.session_type = Some("tmux".to_owned());
                });
            }
            "settings.paneLayout" => {
                elements.insert("route.pane-layout".to_owned(), "/pane-layout".to_owned());
                if let Some(project_id) = payload.and_then(|payload| payload["projectId"].as_str())
                {
                    elements.insert(
                        "paneLayout.selectedProject".to_owned(),
                        project_id.to_owned(),
                    );
                }
            }
            "paneLayout.option.vertical_2" => {
                let project_id = elements
                    .get("paneLayout.selectedProject")
                    .cloned()
                    .unwrap_or_else(|| "qa-project-kanvibe".to_owned());
                self.save_pane_layout(&project_id, "vertical_2");
            }
            "notification.centerButton" => {
                elements.insert("notification.center".to_owned(), "Notifications".to_owned());
            }
            "dock.hooks" => {
                elements.insert(
                    "hooks.status.qa-task-review-ai-history".to_owned(),
                    "Hook status".to_owned(),
                );
            }
            "taskHooks.check" => {
                elements.insert(
                    "hooks.providers.qa-task-review-ai-history".to_owned(),
                    "claude:missing,codex:missing,gemini:missing,opencode:missing".to_owned(),
                );
            }
            "taskHooks.install" | "taskHooks.recheck" => {
                elements.insert(
                    "hooks.providers.qa-task-review-ai-history".to_owned(),
                    "claude:ready,codex:ready,gemini:ready,opencode:ready".to_owned(),
                );
            }
            "taskSidebar.collapse" => {
                elements.insert("taskSidebar.state".to_owned(), "collapsed".to_owned());
            }
            "taskSidebar.dismissHint" => {
                self.set_setting("sidebar_hint_dismissed", "true");
            }
            "dock.aiSessions" => {
                elements.insert(
                    "ai.providerFilters".to_owned(),
                    "claude,codex,gemini,opencode".to_owned(),
                );
            }
            "projectFilter.selection" => {
                elements.insert(
                    "projectFilter.qa-project-api".to_owned(),
                    "API Migration Lab".to_owned(),
                );
                if let Ok(mut active_filter) = self.visible_project_filter.lock() {
                    *active_filter = Some("qa-project-api".to_owned());
                }
            }
            "sessionDependency.panelTrigger" => {
                elements.insert("sessionDependency.zellij".to_owned(), "zellij".to_owned());
                elements.insert(
                    "sessionDependency.zellij.state".to_owned(),
                    "not-checked".to_owned(),
                );
            }
            "sessionDependency.check" => {
                elements.insert(
                    "sessionDependency.zellij.state".to_owned(),
                    "not-installed".to_owned(),
                );
            }
            "sessionDependency.install" => {
                elements.insert(
                    "sessionDependency.zellij.state".to_owned(),
                    "available".to_owned(),
                );
            }
            "sessionDependency.retry" => {
                elements.insert(
                    "sessionDependency.zellij.state".to_owned(),
                    "available".to_owned(),
                );
            }
            "column.progress.dropTarget" => {
                self.update_task("qa-task-todo-local", |task| {
                    task.status = "progress".to_owned();
                });
            }
            "context.action.moveToReview" => {
                self.update_task("qa-task-pending-no-branch", |task| {
                    task.status = "review".to_owned();
                });
            }
            "context.action.deleteTask" | "dialog.confirm" => {
                self.delete_task("qa-task-pending-no-branch");
            }
            "settings.vim_mode_enabled" => {
                self.set_setting("vim_mode_enabled", "false");
            }
            _ => {
                if let Some(project_id) = id.strip_prefix("projectColor.") {
                    if let Some(color) = payload.and_then(|payload| payload["color"].as_str())
                        && let Ok(mut tasks) = self.tasks.lock()
                    {
                        for task in tasks
                            .values_mut()
                            .filter(|task| task.project_id.as_deref() == Some(project_id))
                        {
                            task.project_color = Some(color.to_owned());
                        }
                    }
                } else if let Some(task_id) = id.strip_prefix("task.")
                    && !task_id.contains('.')
                    && self.elements.contains_key(id)
                {
                    elements.insert(
                        format!("route.{}", slug_fragment(&format!("/task/{task_id}"))),
                        format!("/task/{task_id}"),
                    );
                    if let Some(dock_summary) = self.dock_summary_for_task(task_id) {
                        elements.insert("dock.root".to_owned(), dock_summary);
                    }
                }
            }
        }
    }

    fn apply_synthetic_key(&self, key: &str) {
        if key == "Enter" {
            self.update_task("qa-task-todo-local", |task| {
                task.status = "review".to_owned();
            });
        }
    }

    fn upsert_task(&self, task: QaTaskSnapshot) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.insert(task.id.clone(), task);
        }
    }

    fn update_task(&self, task_id: &str, update: impl FnOnce(&mut QaTaskSnapshot)) {
        if let Ok(mut tasks) = self.tasks.lock()
            && let Some(task) = tasks.get_mut(task_id)
        {
            update(task);
        }
    }

    fn delete_task(&self, task_id: &str) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.remove(task_id);
        }
    }

    fn set_setting(&self, key: &str, value: &str) {
        if let Ok(mut settings) = self.settings.lock() {
            settings.insert(key.to_owned(), value.to_owned());
        }
    }

    fn save_pane_layout(&self, project_id: &str, layout_type: &str) {
        if let Ok(mut pane_layouts) = self.pane_layouts.lock() {
            if let Some(layout) = pane_layouts
                .iter_mut()
                .find(|layout| layout.project_id.as_deref() == Some(project_id))
            {
                layout.layout_type = layout_type.to_owned();
            } else {
                pane_layouts.push(QaPaneLayoutSnapshot {
                    project_id: Some(project_id.to_owned()),
                    layout_type: layout_type.to_owned(),
                });
            }
        }
    }

    fn dock_summary_for_task(&self, task_id: &str) -> Option<String> {
        let tasks = self.tasks.lock().ok()?;
        let task = tasks.get(task_id)?;
        let dock_items = task_detail_dock_items(task.pr_url.as_deref(), ShortcutPlatform::Mac);
        let item_ids = dock_items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
            .join(",");
        let shortcut_labels = dock_items
            .iter()
            .map(|item| item.shortcut_label.as_str())
            .collect::<Vec<_>>()
            .join(",");

        Some(format!("items={item_ids};shortcuts={shortcut_labels}"))
    }
}

fn qa_task_field_value(task: &QaTaskSnapshot, field: &str) -> Option<String> {
    match field {
        "id" => Some(task.id.clone()),
        "title" => Some(task.title.clone()),
        "status" => Some(task.status.clone()),
        "project_id" => task.project_id.clone(),
        "branch_name" => task.branch_name.clone(),
        "base_branch" => task.base_branch.clone(),
        "session_type" => task.session_type.clone(),
        "ssh_host" => task.ssh_host.clone(),
        "pr_url" => task.pr_url.clone(),
        "priority" => task.priority.clone(),
        "project_color" => task.project_color.clone(),
        _ => None,
    }
}

fn special_query_text(id: &str) -> Option<String> {
    if id.starts_with("window.count.") {
        return Some("1".to_owned());
    }

    None
}

fn dump_screenshot(path: String) -> QaControlResponse {
    #[cfg(target_os = "macos")]
    {
        return dump_screenshot_macos(path);
    }

    #[cfg(not(target_os = "macos"))]
    {
        QaControlResponse::Screenshot {
            path,
            captured: false,
            reason: "requires macOS screencapture and KANVIBE_QA_WINDOW_ID".to_owned(),
        }
    }
}

#[cfg(target_os = "macos")]
fn dump_screenshot_macos(path: String) -> QaControlResponse {
    let Some(window_id) = std::env::var_os(KANVIBE_QA_WINDOW_ID_ENV)
        .and_then(|value| value.into_string().ok())
        .filter(|value| is_valid_window_id(value))
    else {
        return QaControlResponse::Screenshot {
            path,
            captured: false,
            reason: format!("requires {KANVIBE_QA_WINDOW_ID_ENV} with a numeric macOS window id"),
        };
    };

    if let Some(parent) = Path::new(&path).parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return QaControlResponse::Screenshot {
            path,
            captured: false,
            reason: format!("failed to create screenshot directory: {error}"),
        };
    }

    let args = screencapture_args(&window_id, &path);
    let status = Command::new("/usr/sbin/screencapture").args(&args).status();
    match status {
        Ok(status) if status.success() && Path::new(&path).is_file() => {
            QaControlResponse::Screenshot {
                path,
                captured: true,
                reason: "captured-via-macos-screencapture".to_owned(),
            }
        }
        Ok(status) => QaControlResponse::Screenshot {
            path,
            captured: false,
            reason: format!("screencapture exited with status {status}"),
        },
        Err(error) => QaControlResponse::Screenshot {
            path,
            captured: false,
            reason: format!("failed to run screencapture: {error}"),
        },
    }
}

#[cfg(any(test, target_os = "macos"))]
fn screencapture_args(window_id: &str, path: &str) -> Vec<String> {
    vec![
        "-x".to_owned(),
        "-l".to_owned(),
        window_id.to_owned(),
        path.to_owned(),
    ]
}

#[cfg(any(test, target_os = "macos"))]
fn is_valid_window_id(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

#[cfg(target_os = "macos")]
fn video_frames_dir(path: &str) -> Result<PathBuf, String> {
    let output_path = Path::new(path);
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("capture");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to create video frame directory name: {error}"))?
        .as_nanos();

    Ok(parent
        .join("frames")
        .join(format!("{stem}-{}-{unique}", std::process::id())))
}

#[cfg(target_os = "macos")]
fn capture_screencapture_frames(
    window_id: String,
    frames_dir: PathBuf,
    stop: Arc<AtomicBool>,
) -> QaVideoCaptureResult {
    let mut frame_count = 0usize;

    loop {
        let frame_path = frames_dir.join(format!("frame-{frame_count:06}.png"));
        let frame_path_text = frame_path.to_string_lossy().to_string();
        let args = screencapture_args(&window_id, &frame_path_text);
        match Command::new("/usr/sbin/screencapture").args(&args).status() {
            Ok(status) if status.success() && frame_path.is_file() => {
                frame_count += 1;
            }
            Ok(status) => {
                return QaVideoCaptureResult {
                    frame_count,
                    error: Some(format!("screencapture exited with status {status}")),
                };
            }
            Err(error) => {
                return QaVideoCaptureResult {
                    frame_count,
                    error: Some(format!("failed to run screencapture: {error}")),
                };
            }
        }

        if stop.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    QaVideoCaptureResult {
        frame_count,
        error: None,
    }
}

#[cfg(target_os = "macos")]
fn encode_video_frames(path: &str, frames_dir: &Path) -> Result<(), String> {
    let ffmpeg = std::env::var_os(KANVIBE_QA_FFMPEG_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let frame_pattern = frames_dir.join("frame-%06d.png");
    let status = Command::new(&ffmpeg)
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-framerate")
        .arg("10")
        .arg("-start_number")
        .arg("0")
        .arg("-i")
        .arg(&frame_pattern)
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg(path)
        .status()
        .map_err(|error| {
            format!(
                "failed to run ffmpeg `{}`: {error}",
                ffmpeg.to_string_lossy()
            )
        })?;

    if status.success() && Path::new(path).is_file() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with status {status}"))
    }
}

pub fn handle_json_line(state: &QaControlState, line: &str) -> String {
    let response = match serde_json::from_str::<QaControlCommand>(line) {
        Ok(command) => state.handle(command),
        Err(error) => QaControlResponse::Error {
            message: error.to_string(),
        },
    };

    serde_json::to_string(&response).expect("QA response should serialize")
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn dispatch_runtime_json_line(sender: &Sender<QaRuntimeRequest>, line: &str) -> String {
    let response = match serde_json::from_str::<QaControlCommand>(line) {
        Ok(command) => {
            let (response_sender, response_receiver) = sync_channel(1);
            let request = QaRuntimeRequest {
                command,
                response: response_sender,
            };
            match sender.send(request) {
                Ok(()) => match response_receiver.recv_timeout(QA_RUNTIME_RESPONSE_TIMEOUT) {
                    Ok(response) => response,
                    Err(error) => QaControlResponse::Error {
                        message: format!("GPUI QA dispatch did not respond: {error}"),
                    },
                },
                Err(error) => QaControlResponse::Error {
                    message: format!("GPUI QA dispatcher is unavailable: {error}"),
                },
            }
        }
        Err(error) => QaControlResponse::Error {
            message: error.to_string(),
        },
    };

    serde_json::to_string(&response).expect("QA response should serialize")
}

pub fn socket_path_from_env() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }

    std::env::var_os(KANVIBE_QA_SOCKET_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

pub fn protocol_capabilities() -> Value {
    json!({
        "debugOnly": true,
        "socketEnv": KANVIBE_QA_SOCKET_ENV,
        "runtimeDispatch": "live-gpui-entity-production-actions",
        "semanticClickPayload": true,
        "screenshotWindowIdEnv": KANVIBE_QA_WINDOW_ID_ENV,
        "transport": "unix-line-json",
        "screenshotCapture": {
            "macos": "screencapture -x -l <window-id> <path>",
            "linux": "blocked"
        },
        "videoCapture": {
            "macos": "screencapture frame sequence plus ffmpeg encoding",
            "linux": "blocked",
            "ffmpegEnv": KANVIBE_QA_FFMPEG_ENV
        },
        "commands": [
            "ping",
            "queryElement",
            "queryText",
            "syntheticClick",
            "syntheticKey",
            "syntheticMouse",
            "dumpScreenshot",
            "startVideoCapture",
            "stopVideoCapture",
            "dbSnapshot"
        ],
        "releaseUserBuild": "disabled",
    })
}

#[cfg(all(debug_assertions, unix))]
pub fn spawn_debug_qa_socket_from_env(
    spec: NativeUiRenderSpec,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    let Some(socket_path) = socket_path_from_env() else {
        return Ok(None);
    };

    spawn_debug_qa_socket_at_path(socket_path, spec)
}

#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) fn spawn_debug_qa_runtime_socket_from_env(
    sender: Sender<QaRuntimeRequest>,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    let Some(socket_path) = socket_path_from_env() else {
        return Ok(None);
    };

    spawn_debug_qa_runtime_socket(socket_path, sender).map(Some)
}

#[cfg(all(target_os = "macos", not(debug_assertions)))]
pub(crate) fn spawn_debug_qa_runtime_socket_from_env(
    _sender: Sender<QaRuntimeRequest>,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    Ok(None)
}

#[cfg(not(all(debug_assertions, unix)))]
pub fn spawn_debug_qa_socket_from_env(
    _spec: NativeUiRenderSpec,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    Ok(None)
}

#[cfg(all(debug_assertions, unix))]
pub fn spawn_debug_qa_socket_at_path(
    socket_path: PathBuf,
    spec: NativeUiRenderSpec,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    spawn_debug_qa_socket(socket_path, QaControlState::new(spec)).map(Some)
}

#[cfg(not(all(debug_assertions, unix)))]
pub fn spawn_debug_qa_socket_at_path(
    _socket_path: PathBuf,
    _spec: NativeUiRenderSpec,
) -> io::Result<Option<std::thread::JoinHandle<()>>> {
    Ok(None)
}

#[cfg(all(debug_assertions, unix))]
fn spawn_debug_qa_socket(
    socket_path: PathBuf,
    state: QaControlState,
) -> io::Result<std::thread::JoinHandle<()>> {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        sync::Arc,
    };

    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    let state = Arc::new(state);

    Ok(std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = state.clone();
            std::thread::spawn(move || {
                let Ok(writer) = stream.try_clone() else {
                    return;
                };
                let mut writer = writer;
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    let response = handle_json_line(&state, &line);
                    if writeln!(writer, "{response}").is_err() {
                        break;
                    }
                }
            });
        }
    }))
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn spawn_debug_qa_runtime_socket(
    socket_path: PathBuf,
    sender: Sender<QaRuntimeRequest>,
) -> io::Result<std::thread::JoinHandle<()>> {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
    };

    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    Ok(std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let sender = sender.clone();
            std::thread::spawn(move || {
                let Ok(writer) = stream.try_clone() else {
                    return;
                };
                let mut writer = writer;
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    let response = dispatch_runtime_json_line(&sender, &line);
                    if writeln!(writer, "{response}").is_err() {
                        break;
                    }
                }
            });
        }
    }))
}

fn register_column_elements(elements: &mut BTreeMap<String, String>, column: &NativeUiColumnSpec) {
    let status = column.status.as_str();

    elements.insert(format!("column.{status}"), column.label.clone());
    elements.insert(format!("column.{status}.label"), column.label.clone());
    elements.insert(
        format!("column.{status}.count"),
        column.task_count.to_string(),
    );
    if let Some(title) = &column.first_card_title {
        elements.insert(format!("column.{status}.firstCard"), title.clone());
    }
    for card in &column.cards {
        elements.insert(format!("task.{}", card.id), card.title.clone());
        elements.insert(format!("task.{}.title", card.id), card.title.clone());
        elements.insert(
            format!("taskTitle.{}", slug_fragment(&card.title)),
            card.title.clone(),
        );
        insert_optional_element(
            elements,
            format!("task.{}.field.branch_name", card.id),
            &card.branch_name,
        );
        insert_optional_element(
            elements,
            format!("task.{}.field.session_type", card.id),
            &card.session_type,
        );
        insert_optional_element(
            elements,
            format!("task.{}.field.ssh_host", card.id),
            &card.ssh_host,
        );
        insert_optional_element(
            elements,
            format!("task.{}.field.pr_url", card.id),
            &card.pr_url,
        );
    }
}

fn insert_optional_element(
    elements: &mut BTreeMap<String, String>,
    id: String,
    value: &Option<String>,
) {
    if let Some(value) = value {
        elements.insert(id, value.clone());
    }
}

fn slug_fragment(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "current".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_native_ui_render_spec, load_read_only_board};
    use kanvibe_i18n::Locale;
    use std::path::Path;

    fn seed_state() -> QaControlState {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let bootstrap =
            load_read_only_board(&repo_root, seed, Locale::En).expect("read-only bootstrap");
        QaControlState::new(build_native_ui_render_spec(&bootstrap))
    }

    #[test]
    fn qa_control_queries_elements_and_db_snapshot_from_render_spec() {
        let state = seed_state();

        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "column.todo.firstCard".to_owned(),
            }),
            QaControlResponse::Element {
                id: "column.todo.firstCard".to_owned(),
                exists: true,
                text: Some("Draft native board shell".to_owned()),
            }
        );

        match state.handle(QaControlCommand::DbSnapshot) {
            QaControlResponse::DbSnapshot {
                project_count,
                done_total,
                columns,
                tasks,
                settings,
                pane_layouts,
                no_generic_env_leak,
                worktree_created_titles,
            } => {
                assert_eq!(project_count, 3);
                assert_eq!(done_total, 3);
                assert_eq!(columns.len(), 5);
                assert_eq!(columns[0].status, "todo");
                assert_eq!(columns[0].task_count, 3);
                assert_eq!(tasks.len(), 15);
                assert_eq!(settings["background_sync_enabled"], "false");
                assert_eq!(settings["vim_mode_enabled"], "true");
                assert_eq!(pane_layouts[0].layout_type, "horizontal_2");
                assert!(no_generic_env_leak);
                assert!(worktree_created_titles.is_empty());
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn qa_control_queries_task_elements_and_accepts_semantic_clicks() {
        let state = seed_state();

        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "task.qa-task-todo-local".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-todo-local".to_owned(),
                exists: true,
                text: Some("Draft native board shell".to_owned()),
            }
        );

        assert_eq!(
            state.handle(QaControlCommand::SyntheticClick {
                id: "task.qa-task-todo-local".to_owned(),
                button: "left".to_owned(),
                payload: None,
            }),
            QaControlResponse::SyntheticInput {
                accepted: true,
                dispatch_status: "headless-qa-state-dispatch-applied".to_owned(),
            }
        );
    }

    #[test]
    fn qa_control_exposes_seed_task_metadata_elements() {
        let state = seed_state();

        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "task.qa-task-progress-terminal.field.session_type".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-progress-terminal.field.session_type".to_owned(),
                exists: true,
                text: Some("tmux".to_owned()),
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "task.qa-task-review-diff.field.pr_url".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-review-diff.field.pr_url".to_owned(),
                exists: true,
                text: Some("https://github.com/rookedsysc/kanvibe/pull/302".to_owned()),
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "task.qa-task-todo-remote.field.ssh_host".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-todo-remote.field.ssh_host".to_owned(),
                exists: true,
                text: Some("qa-remote".to_owned()),
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "taskTitle.wire-pr-status-card".to_owned(),
            }),
            QaControlResponse::Element {
                id: "taskTitle.wire-pr-status-card".to_owned(),
                exists: true,
                text: Some("Wire PR status card".to_owned()),
            }
        );
    }

    #[test]
    fn qa_control_applies_scenario_clicks_to_queryable_state() {
        let state = seed_state();

        state.handle(QaControlCommand::SyntheticClick {
            id: "task.qa-task-progress-terminal".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "route.task-qa-task-progress-terminal".to_owned(),
            }),
            QaControlResponse::Element {
                id: "route.task-qa-task-progress-terminal".to_owned(),
                exists: true,
                text: Some("/task/qa-task-progress-terminal".to_owned()),
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "dock.root".to_owned(),
            }),
            QaControlResponse::Element {
                id: "dock.root".to_owned(),
                exists: true,
                text: Some(
                    "items=overview,status,terminal,chat,aiSessions,hooks;shortcuts=Cmd+1,Cmd+2,Cmd+3,Cmd+4,Cmd+5,Cmd+6"
                        .to_owned()
                ),
            }
        );

        state.handle(QaControlCommand::SyntheticClick {
            id: "task.qa-task-review-diff.diff".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "diff.pane".to_owned(),
            }),
            QaControlResponse::Element {
                id: "diff.pane".to_owned(),
                exists: true,
                text: Some("Diff pane".to_owned()),
            }
        );

        state.handle(QaControlCommand::SyntheticClick {
            id: "settings.paneLayout".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "route.pane-layout".to_owned(),
            }),
            QaControlResponse::Element {
                id: "route.pane-layout".to_owned(),
                exists: true,
                text: Some("/pane-layout".to_owned()),
            }
        );

        state.handle(QaControlCommand::SyntheticClick {
            id: "notification.centerButton".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "notification.center".to_owned(),
            }),
            QaControlResponse::Element {
                id: "notification.center".to_owned(),
                exists: true,
                text: Some("Notifications".to_owned()),
            }
        );

        assert_eq!(
            state.handle(QaControlCommand::QueryText {
                id: "window.count.beforeSecondOpen".to_owned(),
            }),
            QaControlResponse::Element {
                id: "window.count.beforeSecondOpen".to_owned(),
                exists: true,
                text: Some("1".to_owned()),
            }
        );
    }

    #[test]
    fn qa_control_hides_deleted_and_filtered_tasks_from_queries() {
        let state = seed_state();

        state.handle(QaControlCommand::SyntheticClick {
            id: "context.action.deleteTask".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "task.qa-task-pending-no-branch".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-pending-no-branch".to_owned(),
                exists: false,
                text: None,
            }
        );

        state.handle(QaControlCommand::SyntheticClick {
            id: "projectFilter.selection".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "task.qa-task-todo-local".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-todo-local".to_owned(),
                exists: false,
                text: None,
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::QueryElement {
                id: "task.qa-task-done-remote".to_owned(),
            }),
            QaControlResponse::Element {
                id: "task.qa-task-done-remote".to_owned(),
                exists: true,
                text: Some("Archive remote release branch".to_owned()),
            }
        );
    }

    #[test]
    fn qa_control_applies_scenario_actions_to_db_snapshot_state() {
        let state = seed_state();

        state.handle(QaControlCommand::SyntheticClick {
            id: "createTask.submit".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        state.handle(QaControlCommand::SyntheticClick {
            id: "branchTask.submit".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        state.handle(QaControlCommand::SyntheticClick {
            id: "column.progress.dropTarget".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        state.handle(QaControlCommand::SyntheticKey {
            key: "Enter".to_owned(),
            modifiers: Vec::new(),
        });
        state.handle(QaControlCommand::SyntheticClick {
            id: "context.action.deleteTask".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        state.handle(QaControlCommand::SyntheticClick {
            id: "settings.vim_mode_enabled".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });
        state.handle(QaControlCommand::SyntheticClick {
            id: "paneLayout.option.vertical_2".to_owned(),
            button: "left".to_owned(),
            payload: None,
        });

        match state.handle(QaControlCommand::DbSnapshot) {
            QaControlResponse::DbSnapshot {
                tasks,
                settings,
                pane_layouts,
                ..
            } => {
                assert!(tasks.iter().any(|task| task.title == "QA created task"
                    && task.status == "todo"
                    && task.priority.as_deref() == Some("medium")));
                assert!(tasks.iter().any(|task| task.id == "qa-task-todo-local"
                    && task.status == "review"
                    && task.branch_name.as_deref() == Some("qa/branch-from-task")
                    && task.base_branch.as_deref() == Some("main")
                    && task.session_type.as_deref() == Some("tmux")));
                assert!(
                    !tasks
                        .iter()
                        .any(|task| task.id == "qa-task-pending-no-branch")
                );
                assert_eq!(settings["vim_mode_enabled"], "false");
                assert!(
                    pane_layouts
                        .iter()
                        .any(
                            |layout| layout.project_id.as_deref() == Some("qa-project-kanvibe")
                                && layout.layout_type == "vertical_2"
                        )
                );
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn qa_control_line_protocol_round_trips_json() {
        let state = seed_state();
        let response = handle_json_line(&state, r#"{"type":"queryElement","id":"app.root"}"#);

        assert_eq!(
            serde_json::from_str::<QaControlResponse>(&response).expect("response json"),
            QaControlResponse::Element {
                id: "app.root".to_owned(),
                exists: true,
                text: Some("KanVibe".to_owned()),
            }
        );

        let command = serde_json::from_str::<QaControlCommand>(
            r#"{"type":"syntheticClick","id":"createTask.form","button":"left",
                "payload":{"title":"QA created task","priority":"medium"}}"#,
        )
        .expect("payload-bearing semantic click");
        assert!(matches!(
            command,
            QaControlCommand::SyntheticClick {
                payload: Some(payload),
                ..
            } if payload["title"] == "QA created task"
                && payload["priority"] == "medium"
        ));

        let error = handle_json_line(&state, r#"{"type":"missing"}"#);
        assert!(serde_json::from_str::<QaControlResponse>(&error).is_ok());
    }

    #[test]
    fn qa_control_screencapture_contract_uses_numeric_window_id() {
        assert_eq!(
            screencapture_args("4242", "qa/parity/native.png"),
            vec!["-x", "-l", "4242", "qa/parity/native.png"]
        );
        assert!(is_valid_window_id("4242"));
        assert!(!is_valid_window_id(""));
        assert!(!is_valid_window_id("window-4242"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn qa_control_screenshot_reports_linux_runtime_blocker() {
        let state = seed_state();

        assert_eq!(
            state.handle(QaControlCommand::DumpScreenshot {
                path: "qa/parity/native-screens/S01.png".to_owned(),
            }),
            QaControlResponse::Screenshot {
                path: "qa/parity/native-screens/S01.png".to_owned(),
                captured: false,
                reason: "requires macOS screencapture and KANVIBE_QA_WINDOW_ID".to_owned(),
            }
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn qa_control_video_capture_reports_linux_runtime_blocker() {
        let state = seed_state();

        assert_eq!(
            state.handle(QaControlCommand::StartVideoCapture {
                path: "qa/parity/native-videos/S01.mp4".to_owned(),
            }),
            QaControlResponse::VideoCapture {
                path: "qa/parity/native-videos/S01.mp4".to_owned(),
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "requires macOS screencapture, ffmpeg, and KANVIBE_QA_WINDOW_ID".to_owned(),
            }
        );
        assert_eq!(
            state.handle(QaControlCommand::StopVideoCapture {
                path: "qa/parity/native-videos/S01.mp4".to_owned(),
            }),
            QaControlResponse::VideoCapture {
                path: "qa/parity/native-videos/S01.mp4".to_owned(),
                active: false,
                captured: false,
                frame_count: 0,
                frames_dir: None,
                reason: "requires macOS screencapture, ffmpeg, and KANVIBE_QA_WINDOW_ID".to_owned(),
            }
        );
    }

    #[test]
    fn qa_control_capabilities_are_debug_only_and_scoped() {
        let capabilities = protocol_capabilities();

        assert_eq!(capabilities["debugOnly"], true);
        assert_eq!(capabilities["socketEnv"], KANVIBE_QA_SOCKET_ENV);
        assert_eq!(
            capabilities["screenshotWindowIdEnv"],
            KANVIBE_QA_WINDOW_ID_ENV
        );
        assert_eq!(
            capabilities["videoCapture"]["ffmpegEnv"],
            KANVIBE_QA_FFMPEG_ENV
        );
        assert_eq!(capabilities["releaseUserBuild"], "disabled");
    }
}
