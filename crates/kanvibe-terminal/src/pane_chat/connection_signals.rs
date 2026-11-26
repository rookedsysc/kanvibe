//! pane 채팅 연결 신호. pane의 CLI가 새 대화 ID를 알린 순간을 받아, 채팅 화면이 다음 조회 주기를 기다리지 않고 그 Task의 pane을
//! 한 번 다시 확인하게 한다. 신호는 다시 읽으라는 알림일 뿐이고 연결 여부는 늘 조회(대상 두 번 확인)가 정한다.
//!
//! 신호의 출처
//! - hook이 대화를 묶음(Gemini `SessionStart`, Claude·Codex 상태 hook). 바인딩 표가 [`PaneChatConnectionSignals::notify_task`]를 부른다
//! - Claude Code가 `~/.claude/sessions/<pid>.json`에 새 대화 ID를 씀(시작, `/clear`, `/resume`)
//! - KanVibe OpenCode 플러그인이 `<작업 경로>/.kanvibe/pane-chat/opencode-<pid>.json`의 대화나 revision을 바꿈
//!
//! 파일은 이 데스크톱의 로컬 Task만 감시한다. 같은 내용을 다시 쓴 경우(OpenCode의 1초 heartbeat, 다른 필드만 바뀐 Claude 기록)는
//! 대화 ID·revision이 그대로이므로 알리지 않는다. 그래서 신호 하나가 재확인 한 번으로만 이어진다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

/// 신호를 받을 Task와, 그 Task의 CLI가 쓰는 로컬 작업 경로(worktree·저장소). 원격 Task는 경로가 비어 hook 신호만 받는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatConnectionScope {
    pub task_id: String,
    pub local_workspaces: Vec<String>,
}

/// 신호를 받은 쪽이 부르는 알림. 감시 스레드에서 불리므로 가볍게(채널에 보내기 등) 끝내야 한다
pub type PaneChatConnectionListener = Arc<dyn Fn() + Send + Sync>;

const OPENCODE_ROUTE_DIRECTORY: [&str; 2] = [".kanvibe", "pane-chat"];

struct Subscriber {
    id: u64,
    scope: PaneChatConnectionScope,
    listener: PaneChatConnectionListener,
}

struct DirectoryWatch {
    _watcher: RecommendedWatcher,
    users: usize,
}

#[derive(Default)]
struct State {
    next_id: u64,
    subscribers: Vec<Subscriber>,
    watches: HashMap<PathBuf, DirectoryWatch>,
    /// 파일마다 마지막으로 본 대화 ID(와 revision). 바뀌었을 때만 알린다
    fingerprints: HashMap<PathBuf, String>,
}

struct Inner {
    claude_sessions_directory: Option<PathBuf>,
    state: Mutex<State>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 연결 신호 모음. 앱이 하나 만들어 바인딩 표와 pane 채팅 런타임이 함께 쓴다
pub struct PaneChatConnectionSignals {
    inner: Arc<Inner>,
}

/// 걸어 둔 구독 하나. 값을 버리면 알림을 멈추고, 더 쓰는 곳이 없는 디렉터리 감시도 멈춘다
pub struct PaneChatConnectionSubscription {
    inner: Weak<Inner>,
    id: u64,
    directories: Vec<PathBuf>,
}

impl PaneChatConnectionSignals {
    /// `local_home`은 이 데스크톱 사용자의 홈이다. Claude 세션 기록 디렉터리를 거기서 찾는다
    pub fn new(local_home: Option<&str>) -> Self {
        let claude_sessions_directory = local_home
            .filter(|home| !home.is_empty())
            .map(|home| Path::new(home).join(".claude").join("sessions"));
        Self {
            inner: Arc::new(Inner {
                claude_sessions_directory,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// 그 Task의 연결 신호를 받는다. 감시할 디렉터리가 아직 없으면 그 출처의 신호만 빠지고 조회 주기로 연결된다
    pub fn subscribe(
        &self,
        scope: PaneChatConnectionScope,
        listener: PaneChatConnectionListener,
    ) -> PaneChatConnectionSubscription {
        let directories = self.watched_directories(&scope);
        let mut state = self.inner.lock();
        state.next_id += 1;
        let id = state.next_id;
        state.subscribers.push(Subscriber {
            id,
            scope,
            listener,
        });
        let mut acquired = Vec::new();
        for directory in directories {
            if let Some(watch) = state.watches.get_mut(&directory) {
                watch.users += 1;
                acquired.push(directory);
                continue;
            }
            if let Some(watcher) = watch_directory(&self.inner, &directory) {
                state.watches.insert(
                    directory.clone(),
                    DirectoryWatch {
                        _watcher: watcher,
                        users: 1,
                    },
                );
                acquired.push(directory);
            }
        }
        PaneChatConnectionSubscription {
            inner: Arc::downgrade(&self.inner),
            id,
            directories: acquired,
        }
    }

    /// hook이 그 Task의 대화를 새로 묶었다
    pub fn notify_task(&self, task_id: &str) {
        let listeners: Vec<PaneChatConnectionListener> = self
            .inner
            .lock()
            .subscribers
            .iter()
            .filter(|subscriber| subscriber.scope.task_id == task_id)
            .map(|subscriber| subscriber.listener.clone())
            .collect();
        for listener in listeners {
            listener();
        }
    }

    fn watched_directories(&self, scope: &PaneChatConnectionScope) -> Vec<PathBuf> {
        if scope.local_workspaces.is_empty() {
            return Vec::new();
        }
        let opencode = scope.local_workspaces.iter().map(|workspace| {
            OPENCODE_ROUTE_DIRECTORY
                .iter()
                .fold(PathBuf::from(workspace), |path, part| path.join(part))
        });
        self.inner
            .claude_sessions_directory
            .iter()
            .cloned()
            .chain(opencode)
            .collect()
    }

    /// 감시가 알린 파일 하나를 읽어, 대화가 바뀌었으면 그 작업 경로의 구독자에게 알린다
    #[cfg(test)]
    pub(crate) fn handle_file_change(&self, path: &Path) {
        handle_file_change(&self.inner, path);
    }
}

impl Drop for PaneChatConnectionSubscription {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut released = Vec::new();
        {
            let mut state = inner.lock();
            state
                .subscribers
                .retain(|subscriber| subscriber.id != self.id);
            for directory in &self.directories {
                let is_unused = state.watches.get_mut(directory).is_some_and(|watch| {
                    watch.users -= 1;
                    watch.users == 0
                });
                if is_unused {
                    released.extend(state.watches.remove(directory));
                }
            }
        }
        // 감시를 멈추는 일은 잠금 밖에서 한다. 감시 스레드가 같은 잠금을 기다리는 중일 수 있다
        drop(released);
    }
}

fn watch_directory(inner: &Arc<Inner>, directory: &Path) -> Option<RecommendedWatcher> {
    if !directory.is_dir() {
        return None;
    }
    let weak = Arc::downgrade(inner);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            return;
        };
        if matches!(event.kind, EventKind::Access(_) | EventKind::Remove(_)) {
            return;
        }
        let Some(inner) = weak.upgrade() else {
            return;
        };
        for path in &event.paths {
            handle_file_change(&inner, path);
        }
    })
    .ok()?;
    watcher.watch(directory, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}

/// 알릴 만한 파일의 (대화 지문, CLI가 일하는 경로). 임시 파일과 다른 파일은 `None`이다
fn read_connection_record(inner: &Inner, path: &Path) -> Option<(String, String)> {
    let name = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    let is_claude_record = inner.claude_sessions_directory.as_deref() == Some(parent)
        && name
            .strip_suffix(".json")
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()));
    let is_opencode_route = parent.ends_with(OPENCODE_ROUTE_DIRECTORY.iter().collect::<PathBuf>())
        && name
            .strip_prefix("opencode-")
            .and_then(|rest| rest.strip_suffix(".json"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()));
    if !is_claude_record && !is_opencode_route {
        return None;
    }
    let record: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let cwd = record.get("cwd")?.as_str()?.to_string();
    let fingerprint = match is_claude_record {
        true => record.get("sessionId")?.to_string(),
        false => format!("{}|{}", record.get("sessionId")?, record.get("revision")?),
    };
    Some((fingerprint, cwd))
}

fn is_inside(cwd: &str, workspace: &str) -> bool {
    let workspace = workspace.trim_end_matches('/');
    !workspace.is_empty()
        && (cwd == workspace
            || cwd
                .strip_prefix(workspace)
                .is_some_and(|rest| rest.starts_with('/')))
}

fn handle_file_change(inner: &Inner, path: &Path) {
    let Some((fingerprint, cwd)) = read_connection_record(inner, path) else {
        return;
    };
    let listeners: Vec<PaneChatConnectionListener> = {
        let mut state = inner.lock();
        let previous = state
            .fingerprints
            .insert(path.to_path_buf(), fingerprint.clone());
        if previous.as_deref() == Some(fingerprint.as_str()) {
            return;
        }
        state
            .subscribers
            .iter()
            .filter(|subscriber| {
                subscriber
                    .scope
                    .local_workspaces
                    .iter()
                    .any(|workspace| is_inside(&cwd, workspace))
            })
            .map(|subscriber| subscriber.listener.clone())
            .collect()
    };
    for listener in listeners {
        listener();
    }
}

#[cfg(test)]
mod tests;
