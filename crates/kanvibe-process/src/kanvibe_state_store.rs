//! 프로젝트 색상·task 상태 서비스가 쓰는 `.kanvibe/` 파일과 DB 조회의 경계.
//! 서비스는 이 trait만 받아, 실제 앱은 `HostFileAccess`와 `Database`를 넘기고 테스트는 호출을 기록하는 가짜를 넘긴다.

use std::collections::BTreeMap;

use kanvibe_db::Database;
use kanvibe_db::repositories::projects::find_project;
use kanvibe_db::repositories::tasks::list_tasks;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{TaskPriority, TaskStatus};

use crate::exec::ExecGitOptions;
use crate::git_exclude::add_ai_tool_patterns_to_git_exclude;
use crate::host_file_access::{HostFileAccess, HostFileError, TextFileReadResult};
use crate::kanvibe_project_state::{
    KanvibeTaskMetadataUpdate, KanvibeTaskSession, KanvibeTaskState, KanvibeTaskSyncState,
    read_kanvibe_project_color, read_kanvibe_task_state, read_kanvibe_task_sync_state,
    write_kanvibe_project_color, write_kanvibe_project_color_if_absent,
    write_kanvibe_project_priority, write_kanvibe_task_metadata, write_kanvibe_task_session,
    write_kanvibe_task_status,
};

/// 저장소의 `.kanvibe/` 상태 파일과 git exclude를 읽고 쓰는 함수들(TS `kanvibeProjectState`·`gitExclude`)
pub trait KanvibeStateStore: Send + Sync {
    fn read_project_color(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HostFileError>;
    fn write_project_color(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn write_project_color_if_absent(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn write_project_priority(
        &self,
        repo_path: &str,
        priority: Option<TaskPriority>,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn add_ai_tool_patterns_to_git_exclude(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn read_task_state(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<KanvibeTaskState>, HostFileError>;
    fn read_task_sync_state(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        project_repo_path: Option<&str>,
    ) -> Result<KanvibeTaskSyncState, HostFileError>;
    fn write_task_status(
        &self,
        repo_path: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn write_task_metadata(
        &self,
        repo_path: &str,
        metadata: &KanvibeTaskMetadataUpdate,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    fn write_task_session(
        &self,
        repo_path: &str,
        session: &KanvibeTaskSession,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError>;
    /// 한 호스트의 여러 파일을 한 번의 왕복으로 읽는다
    fn read_text_files(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError>;
}

/// 실제 로컬·원격 파일을 다루는 구현
pub struct HostKanvibeStateStore<'a> {
    files: &'a HostFileAccess,
}

impl<'a> HostKanvibeStateStore<'a> {
    pub fn new(files: &'a HostFileAccess) -> Self {
        Self { files }
    }
}

impl KanvibeStateStore for HostKanvibeStateStore<'_> {
    fn read_project_color(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HostFileError> {
        read_kanvibe_project_color(self.files, repo_path, ssh_host)
    }

    fn write_project_color(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_project_color(self.files, repo_path, project_color, ssh_host)
    }

    fn write_project_color_if_absent(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_project_color_if_absent(self.files, repo_path, project_color, ssh_host)
    }

    fn write_project_priority(
        &self,
        repo_path: &str,
        priority: Option<TaskPriority>,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_project_priority(self.files, repo_path, priority, ssh_host)
    }

    fn add_ai_tool_patterns_to_git_exclude(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        add_ai_tool_patterns_to_git_exclude(self.files, repo_path, ssh_host)
    }

    fn read_task_state(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<KanvibeTaskState>, HostFileError> {
        read_kanvibe_task_state(self.files, repo_path, ssh_host)
    }

    fn read_task_sync_state(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        project_repo_path: Option<&str>,
    ) -> Result<KanvibeTaskSyncState, HostFileError> {
        read_kanvibe_task_sync_state(self.files, repo_path, ssh_host, project_repo_path)
    }

    fn write_task_status(
        &self,
        repo_path: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_task_status(self.files, repo_path, status, ssh_host)
    }

    fn write_task_metadata(
        &self,
        repo_path: &str,
        metadata: &KanvibeTaskMetadataUpdate,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_task_metadata(self.files, repo_path, metadata, ssh_host)
    }

    fn write_task_session(
        &self,
        repo_path: &str,
        session: &KanvibeTaskSession,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        write_kanvibe_task_session(self.files, repo_path, session, ssh_host)
    }

    fn read_text_files(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
        self.files
            .read_text_files(paths, ssh_host, &ExecGitOptions::default())
    }
}

/// 프로젝트와 소속 task 조회(TS `getProjectRepository().findOneBy`, `getTaskRepository().findBy({ projectId })`)
pub trait ProjectRecords: Send + Sync {
    fn find_project(&self, project_id: &str) -> Result<Option<Project>, String>;
    /// 프로젝트에 속한 task들의 worktree 경로. 경로가 없는 task는 `None`이다
    fn project_task_worktree_paths(&self, project_id: &str) -> Result<Vec<Option<String>>, String>;
}

impl ProjectRecords for Database {
    fn find_project(&self, project_id: &str) -> Result<Option<Project>, String> {
        find_project(&self.connection(), project_id).map_err(|error| error.to_string())
    }

    fn project_task_worktree_paths(&self, project_id: &str) -> Result<Vec<Option<String>>, String> {
        let tasks = list_tasks(&self.connection()).map_err(|error| error.to_string())?;
        Ok(tasks
            .into_iter()
            .filter(|task| task.project_id.as_deref() == Some(project_id))
            .map(|task| task.worktree_path)
            .collect())
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! 서비스 테스트가 TS `vi.mock("@/lib/kanvibeProjectState")`처럼 쓰는 기록용 가짜

    use std::sync::Mutex;

    use super::*;

    type ColorReader = dyn Fn(&str) -> Result<Option<String>, HostFileError> + Send + Sync;
    type FailureRule = dyn Fn(&str) -> Option<String> + Send + Sync;
    type TextFilesReader = dyn Fn(&[String], Option<&str>) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError>
        + Send
        + Sync;

    /// 호출을 `"<함수> <인자...>"` 문자열로 기록한다
    pub(crate) struct RecordingStateStore {
        pub calls: Mutex<Vec<String>>,
        pub project_color: Box<ColorReader>,
        /// 함수 이름을 받아 실패 메시지를 돌려주면 그 호출을 실패시킨다
        pub failure: Box<FailureRule>,
        pub task_state: Mutex<Option<KanvibeTaskState>>,
        pub text_files: Box<TextFilesReader>,
    }

    impl Default for RecordingStateStore {
        fn default() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                project_color: Box::new(|_| Ok(None)),
                failure: Box::new(|_| None),
                task_state: Mutex::new(None),
                text_files: Box::new(|_, _| Ok(BTreeMap::new())),
            }
        }
    }

    fn host_label(ssh_host: Option<&str>) -> &str {
        ssh_host.unwrap_or("null")
    }

    impl RecordingStateStore {
        pub fn calls_of(&self, function_name: &str) -> Vec<String> {
            let prefix = format!("{function_name} ");
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with(&prefix))
                .cloned()
                .collect()
        }

        fn record(&self, function_name: &str, arguments: &[&str]) -> Result<(), HostFileError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{function_name} {}", arguments.join(" ")));
            match (self.failure)(function_name) {
                Some(message) => Err(HostFileError::Message(message)),
                None => Ok(()),
            }
        }
    }

    impl KanvibeStateStore for RecordingStateStore {
        fn read_project_color(
            &self,
            repo_path: &str,
            ssh_host: Option<&str>,
        ) -> Result<Option<String>, HostFileError> {
            self.record("read_project_color", &[repo_path, host_label(ssh_host)])?;
            (self.project_color)(repo_path)
        }

        fn write_project_color(
            &self,
            repo_path: &str,
            project_color: &str,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            self.record(
                "write_project_color",
                &[repo_path, project_color, host_label(ssh_host)],
            )
        }

        fn write_project_color_if_absent(
            &self,
            repo_path: &str,
            project_color: &str,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            self.record(
                "write_project_color_if_absent",
                &[repo_path, project_color, host_label(ssh_host)],
            )
        }

        fn write_project_priority(
            &self,
            repo_path: &str,
            priority: Option<TaskPriority>,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            let priority = priority.map_or("null", TaskPriority::as_str);
            self.record(
                "write_project_priority",
                &[repo_path, priority, host_label(ssh_host)],
            )
        }

        fn add_ai_tool_patterns_to_git_exclude(
            &self,
            repo_path: &str,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            self.record(
                "add_ai_tool_patterns_to_git_exclude",
                &[repo_path, host_label(ssh_host)],
            )
        }

        fn read_task_state(
            &self,
            repo_path: &str,
            ssh_host: Option<&str>,
        ) -> Result<Option<KanvibeTaskState>, HostFileError> {
            self.record("read_task_state", &[repo_path, host_label(ssh_host)])?;
            Ok(self.task_state.lock().unwrap().clone())
        }

        fn read_task_sync_state(
            &self,
            repo_path: &str,
            ssh_host: Option<&str>,
            project_repo_path: Option<&str>,
        ) -> Result<KanvibeTaskSyncState, HostFileError> {
            let project_repo_path = project_repo_path.unwrap_or("null");
            self.record(
                "read_task_sync_state",
                &[repo_path, host_label(ssh_host), project_repo_path],
            )?;
            Ok(KanvibeTaskSyncState::default())
        }

        fn write_task_status(
            &self,
            repo_path: &str,
            status: TaskStatus,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            self.record(
                "write_task_status",
                &[repo_path, status.as_str(), host_label(ssh_host)],
            )
        }

        fn write_task_metadata(
            &self,
            repo_path: &str,
            metadata: &KanvibeTaskMetadataUpdate,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            let metadata = format!("{metadata:?}");
            self.record(
                "write_task_metadata",
                &[repo_path, &metadata, host_label(ssh_host)],
            )
        }

        fn write_task_session(
            &self,
            repo_path: &str,
            session: &KanvibeTaskSession,
            ssh_host: Option<&str>,
        ) -> Result<(), HostFileError> {
            let session = format!("{}:{}", session.session_type.as_str(), session.session_name);
            self.record(
                "write_task_session",
                &[repo_path, &session, host_label(ssh_host)],
            )
        }

        fn read_text_files(
            &self,
            paths: &[String],
            ssh_host: Option<&str>,
        ) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
            self.record("read_text_files", &[&paths.join(","), host_label(ssh_host)])?;
            (self.text_files)(paths, ssh_host)
        }
    }

    /// 프로젝트 하나와 그 task들의 worktree 경로를 돌려주고 조회를 기록한다
    #[derive(Default)]
    pub(crate) struct RecordingProjectRecords {
        pub project: Mutex<Option<Project>>,
        pub worktree_paths: Mutex<Vec<Option<String>>>,
        pub lookups: Mutex<Vec<String>>,
    }

    impl ProjectRecords for RecordingProjectRecords {
        fn find_project(&self, project_id: &str) -> Result<Option<Project>, String> {
            self.lookups
                .lock()
                .unwrap()
                .push(format!("find_project {project_id}"));
            Ok(self.project.lock().unwrap().clone())
        }

        fn project_task_worktree_paths(
            &self,
            project_id: &str,
        ) -> Result<Vec<Option<String>>, String> {
            self.lookups
                .lock()
                .unwrap()
                .push(format!("worktree_paths {project_id}"));
            Ok(self.worktree_paths.lock().unwrap().clone())
        }
    }

    pub(crate) fn project(color: Option<&str>, ssh_host: Option<&str>) -> Project {
        Project {
            id: "project-1".into(),
            name: "kanvibe".into(),
            repo_path: "/workspace/kanvibe".into(),
            default_branch: "main".into(),
            ssh_host: ssh_host.map(str::to_string),
            is_worktree: false,
            color: color.map(str::to_string),
            icon_data_url: None,
            created_at: "2026-11-25 00:00:00".into(),
        }
    }
}
