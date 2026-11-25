//! 프로젝트 색상·priority를 `.kanvibe/project.json`으로 공유한다(TS `kanvibeProjectColorService.ts`).
//!
//! 프로젝트 색상의 권위는 프로젝트 루트의 `.kanvibe/project.json` 하나뿐이다.
//! 루트 파일을 쓰는 주체는 사용자 색상 편집과, 파일이 아직 없을 때의 씨앗 기록 둘로 한정한다.
//! background sync는 루트를 읽기만 하고 worktree 전파와 DB 반영만 담당한다.
//! sync가 루트를 쓰게 두면 sync 시작 시점에 읽은 낡은 색상이 그 사이의 사용자 편집을 되돌리고,
//! 되돌아간 루트 값이 다음 sync에서 다시 DB로 내려가 색이 왕복하게 된다.

use kanvibe_domain::project::Project;
use kanvibe_domain::task::TaskPriority;

use crate::kanvibe_project_state::parse_project_color;
use crate::kanvibe_state_store::{KanvibeStateStore, ProjectRecords};

pub struct ProjectColorService<'a> {
    state_store: &'a dyn KanvibeStateStore,
    records: &'a dyn ProjectRecords,
}

fn host_label(ssh_host: Option<&str>) -> String {
    ssh_host.map_or("null".into(), |host| format!("{host:?}"))
}

impl<'a> ProjectColorService<'a> {
    pub fn new(state_store: &'a dyn KanvibeStateStore, records: &'a dyn ProjectRecords) -> Self {
        Self {
            state_store,
            records,
        }
    }

    /// 사용자가 고른 색상을 `.kanvibe/project.json`에 기록해 같은 저장소를 보는 다른 KanVibe client와 공유한다.
    /// 권위인 프로젝트 루트를 먼저 확정한 뒤 소속 task worktree에 퍼뜨려, 어느 경로로 열어도 같은 색상을 읽게 한다.
    pub fn persist_project_color_to_kanvibe_state(&self, project: &Project) {
        let Some(project_color) = project.color.as_deref().and_then(parse_project_color) else {
            return;
        };

        // worktree들은 git common dir의 info/exclude를 공유하므로 exclude 갱신은 저장소당 한 번이면 된다
        self.exclude_kanvibe_state_directory(project);
        let ssh_host = project.ssh_host.as_deref();
        self.write_project_color_quietly(&project.repo_path, &project_color, ssh_host);

        for worktree_path in self.project_worktree_paths(project) {
            self.write_project_color_quietly(&worktree_path, &project_color, ssh_host);
        }
    }

    /// root task priority를 프로젝트 공유 값으로 저장하고 소속 worktree에도 전파한다
    pub fn persist_project_priority_to_kanvibe_state(
        &self,
        project: &Project,
        priority: Option<TaskPriority>,
    ) {
        self.exclude_kanvibe_state_directory(project);
        let ssh_host = project.ssh_host.as_deref();
        for repo_path in self.resolve_project_state_repo_paths(project) {
            if let Err(error) = self
                .state_store
                .write_project_priority(&repo_path, priority, ssh_host)
            {
                eprintln!(
                    "[project-priority] .kanvibe priority 기록 실패 {{ repoPath: {repo_path:?}, sshHost: {}, error: {:?} }}",
                    host_label(ssh_host),
                    error.to_string()
                );
            }
        }
    }

    /// 다른 client가 `.kanvibe/project.json`에 남긴 프로젝트 색상을 읽는다.
    /// 프로젝트를 새로 등록할 때 기존 client가 정한 색상을 그대로 이어받기 위해 쓴다.
    pub fn read_shared_project_color(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Option<String> {
        match self.state_store.read_project_color(repo_path, ssh_host) {
            Ok(project_color) => project_color,
            Err(error) => {
                eprintln!(
                    "[project-color] .kanvibe 색상 읽기 실패 {{ repoPath: {repo_path:?}, sshHost: {}, error: {:?} }}",
                    host_label(ssh_host),
                    error.to_string()
                );
                None
            }
        }
    }

    /// 권위인 프로젝트 루트의 공유 색상을 읽어 worktree에 퍼뜨리고, DB에 반영할 값을 돌려준다.
    /// 공유 파일이 아직 없으면 현재 색상을 씨앗으로 기록한다. 씨앗이 없으면 이 기능 이전에 등록된
    /// 프로젝트끼리는 서로 다른 DB 색상을 계속 유지하게 된다.
    ///
    /// 프로젝트 색상을 여기서 직접 바꾸지 않고 값만 돌려주는 이유는, 호출한 쪽이 DB에 쓸 때
    /// 자신이 읽어둔 색상이 아직 최신인지 확인(compare-and-set)할 수 있어야 하기 때문이다.
    /// 반영할 변경이 없으면 `None`이다.
    pub fn sync_project_color_with_kanvibe_state(&self, project: &Project) -> Option<String> {
        let ssh_host = project.ssh_host.as_deref();
        let Some(shared_color) = self.read_shared_project_color(&project.repo_path, ssh_host)
        else {
            self.seed_shared_project_color(project);
            return None;
        };

        self.propagate_shared_project_color_to_worktrees(project, &shared_color);
        let is_unchanged = project.color.as_deref() == Some(shared_color.as_str());
        (!is_unchanged).then_some(shared_color)
    }

    /// 공유 색상 파일이 없는 프로젝트에 DB의 현재 색상을 씨앗으로 남긴다.
    /// sync가 시작될 때 읽은 메모리 값은 그 사이 사용자 편집으로 낡았을 수 있어, 낡은 값이 권위 파일의 초기값으로 굳지 않게 한다.
    /// 존재 검사와 기록 사이에 사용자 편집이 루트 파일을 만들 수 있으므로, 원자적 쓰기로 이미 만들어진 권위 파일을 덮지 않는다.
    fn seed_shared_project_color(&self, project: &Project) {
        let current_color = self
            .read_current_project_color(&project.id)
            .or_else(|| project.color.clone());
        let Some(project_color) = current_color.as_deref().and_then(parse_project_color) else {
            return;
        };

        self.exclude_kanvibe_state_directory(project);
        let ssh_host = project.ssh_host.as_deref();
        let seeded = self.state_store.write_project_color_if_absent(
            &project.repo_path,
            &project_color,
            ssh_host,
        );
        if let Err(error) = seeded {
            eprintln!(
                "[project-color] .kanvibe 색상 씨앗 기록 실패 {{ repoPath: {:?}, sshHost: {}, error: {:?} }}",
                project.repo_path,
                host_label(ssh_host),
                error.to_string()
            );
        }
    }

    /// 씨앗으로 남길 색상은 sync 시작 시점의 스냅샷이 아니라 DB에 저장된 현재 값이어야 한다
    fn read_current_project_color(&self, project_id: &str) -> Option<String> {
        match self.records.find_project(project_id) {
            Ok(project) => project.and_then(|project| project.color),
            Err(error) => {
                eprintln!(
                    "[project-color] 프로젝트 색상 조회 실패 {{ projectId: {project_id:?}, error: {error:?} }}"
                );
                None
            }
        }
    }

    /// 프로젝트 루트에 기록된 공유 색상을 소속 worktree들에 퍼뜨린다.
    /// 기록하는 값은 방금 루트에서 읽은 색상이고, 루트 파일은 사용자 편집과 씨앗 기록만 쓰는 권위 경로라 건드리지 않는다.
    fn propagate_shared_project_color_to_worktrees(&self, project: &Project, shared_color: &str) {
        let ssh_host = project.ssh_host.as_deref();
        let outdated_paths: Vec<String> = self
            .project_worktree_paths(project)
            .into_iter()
            .filter(|repo_path| {
                self.read_shared_project_color(repo_path, ssh_host)
                    .as_deref()
                    != Some(shared_color)
            })
            .collect();

        // 이미 같은 색상인 경로만 남으면 sync 주기마다 파일을 다시 쓰지 않는다
        if outdated_paths.is_empty() {
            return;
        }

        self.exclude_kanvibe_state_directory(project);
        for repo_path in outdated_paths {
            self.write_project_color_quietly(&repo_path, shared_color, ssh_host);
        }
    }

    /// 한 경로의 색상 기록 실패가 나머지 경로 기록을 막지 않도록 경고만 남긴다
    fn write_project_color_quietly(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) {
        if let Err(error) = self
            .state_store
            .write_project_color(repo_path, project_color, ssh_host)
        {
            eprintln!(
                "[project-color] .kanvibe 색상 기록 실패 {{ repoPath: {repo_path:?}, sshHost: {}, error: {:?} }}",
                host_label(ssh_host),
                error.to_string()
            );
        }
    }

    fn exclude_kanvibe_state_directory(&self, project: &Project) {
        let ssh_host = project.ssh_host.as_deref();
        if let Err(error) = self
            .state_store
            .add_ai_tool_patterns_to_git_exclude(&project.repo_path, ssh_host)
        {
            eprintln!(
                "[project-color] git exclude 패턴 추가 실패 {{ repoPath: {:?}, sshHost: {}, error: {:?} }}",
                project.repo_path,
                host_label(ssh_host),
                error.to_string()
            );
        }
    }

    /// 루트를 뺀 소속 task worktree 경로
    fn project_worktree_paths(&self, project: &Project) -> Vec<String> {
        self.resolve_project_state_repo_paths(project)
            .into_iter()
            .filter(|repo_path| *repo_path != project.repo_path)
            .collect()
    }

    /// 프로젝트 공유 상태를 기록할 경로 목록. 프로젝트 루트와 소속 task worktree를 처음 나온 순서대로 담는다
    fn resolve_project_state_repo_paths(&self, project: &Project) -> Vec<String> {
        let mut repo_paths = vec![project.repo_path.clone()];
        match self.records.project_task_worktree_paths(&project.id) {
            Ok(worktree_paths) => {
                for worktree_path in worktree_paths.into_iter().flatten() {
                    if !worktree_path.is_empty() && !repo_paths.contains(&worktree_path) {
                        repo_paths.push(worktree_path);
                    }
                }
            }
            Err(error) => eprintln!(
                "[project-color] worktree 경로 조회 실패 {{ projectId: {:?}, error: {error:?} }}",
                project.id
            ),
        }
        repo_paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_file_access::HostFileError;
    use crate::kanvibe_state_store::testing::{
        RecordingProjectRecords, RecordingStateStore, project,
    };

    const ROOT: &str = "/workspace/kanvibe";
    const FEATURE: &str = "/workspace/kanvibe__worktrees/feature";

    fn records_with_worktrees(worktree_paths: &[Option<&str>]) -> RecordingProjectRecords {
        let records = RecordingProjectRecords::default();
        *records.worktree_paths.lock().unwrap() = worktree_paths
            .iter()
            .map(|path| path.map(str::to_string))
            .collect();
        records
    }

    fn shared_colors(
        root_color: Option<&'static str>,
        worktree_color: Option<&'static str>,
    ) -> RecordingStateStore {
        RecordingStateStore {
            project_color: Box::new(move |repo_path| {
                Ok(if repo_path == ROOT {
                    root_color
                } else {
                    worktree_color
                }
                .map(str::to_string))
            }),
            ..RecordingStateStore::default()
        }
    }

    // parity: TS-main-kanvibeProjectColorService-002a58
    #[test]
    fn writes_the_color_to_the_project_root_and_every_task_worktree() {
        let store = RecordingStateStore::default();
        let records = records_with_worktrees(&[Some(FEATURE), None, Some(FEATURE)]);

        ProjectColorService::new(&store, &records)
            .persist_project_color_to_kanvibe_state(&project(Some("#65D08A"), Some("remote-host")));

        assert_eq!(
            store.calls_of("write_project_color"),
            [
                format!("write_project_color {ROOT} #65D08A remote-host"),
                format!("write_project_color {FEATURE} #65D08A remote-host"),
            ]
        );
    }

    // parity: TS-main-kanvibeProjectColorService-12fa83
    #[test]
    fn writes_the_root_task_priority_to_the_root_and_every_worktree() {
        let store = RecordingStateStore::default();
        let records = records_with_worktrees(&[Some(FEATURE), None]);

        ProjectColorService::new(&store, &records).persist_project_priority_to_kanvibe_state(
            &project(Some("#65D08A"), Some("remote-host")),
            Some(TaskPriority::High),
        );

        assert_eq!(
            store.calls_of("write_project_priority"),
            [
                format!("write_project_priority {ROOT} high remote-host"),
                format!("write_project_priority {FEATURE} high remote-host"),
            ]
        );
    }

    // parity: TS-main-kanvibeProjectColorService-9c0949
    #[test]
    fn missing_or_malformed_colors_are_not_written() {
        let store = RecordingStateStore::default();
        let records = records_with_worktrees(&[]);
        let service = ProjectColorService::new(&store, &records);

        service.persist_project_color_to_kanvibe_state(&project(None, None));
        service.persist_project_color_to_kanvibe_state(&project(Some("not-a-color"), None));

        assert!(store.calls_of("write_project_color").is_empty());
    }

    // parity: TS-main-kanvibeProjectColorService-c5f84f
    #[test]
    fn one_failed_path_does_not_stop_the_remaining_writes() {
        let is_first_write = std::sync::atomic::AtomicBool::new(true);
        let store = RecordingStateStore {
            failure: Box::new(move |function_name| {
                let should_fail = function_name == "write_project_color"
                    && is_first_write.swap(false, std::sync::atomic::Ordering::SeqCst);
                should_fail.then(|| "read-only".to_string())
            }),
            ..RecordingStateStore::default()
        };
        let records = records_with_worktrees(&[Some(FEATURE)]);

        ProjectColorService::new(&store, &records)
            .persist_project_color_to_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(store.calls_of("write_project_color").len(), 2);
    }

    // parity: TS-main-kanvibeProjectColorService-52383a
    #[test]
    fn only_a_color_changed_by_another_client_is_returned_for_the_database() {
        let store = shared_colors(Some("#0064FF"), Some("#0064FF"));
        let records = records_with_worktrees(&[]);
        let service = ProjectColorService::new(&store, &records);

        assert_eq!(
            service.sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None)),
            Some("#0064FF".into())
        );
        assert_eq!(
            service.sync_project_color_with_kanvibe_state(&project(Some("#0064FF"), None)),
            None
        );
    }

    // parity: TS-main-kanvibeProjectColorService-9325b8
    #[test]
    fn sync_does_not_change_the_project_color_itself() {
        let store = shared_colors(Some("#0064FF"), Some("#0064FF"));
        let records = records_with_worktrees(&[]);
        let synced_project = project(Some("#65D08A"), None);

        ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&synced_project);

        assert_eq!(synced_project.color.as_deref(), Some("#65D08A"));
    }

    // parity: TS-main-kanvibeProjectColorService-5034ca
    #[test]
    fn paths_that_already_hold_the_same_color_are_not_rewritten() {
        let store = shared_colors(Some("#65D08A"), Some("#65D08A"));
        let records = records_with_worktrees(&[Some(FEATURE)]);

        let synced = ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(synced, None);
        assert!(store.calls_of("write_project_color").is_empty());
        assert!(
            store
                .calls_of("add_ai_tool_patterns_to_git_exclude")
                .is_empty()
        );
    }

    // parity: TS-main-kanvibeProjectColorService-029fc7
    #[test]
    fn worktrees_created_after_the_color_was_fixed_receive_the_shared_color() {
        let store = shared_colors(Some("#65D08A"), None);
        let records = records_with_worktrees(&[Some(FEATURE)]);

        let synced = ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(synced, None);
        assert_eq!(
            store.calls_of("write_project_color"),
            [format!("write_project_color {FEATURE} #65D08A null")]
        );
    }

    // parity: TS-main-kanvibeProjectColorService-8e4793
    #[test]
    fn sync_spreads_the_new_root_color_instead_of_the_stale_in_memory_color() {
        let store = shared_colors(Some("#0064FF"), Some("#0064FF"));
        let records = records_with_worktrees(&[Some(FEATURE)]);

        let synced = ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(synced, Some("#0064FF".into()));
        assert!(store.calls_of("write_project_color").is_empty());
    }

    // parity: TS-main-kanvibeProjectColorService-62d6ba
    #[test]
    fn the_project_root_color_file_is_not_a_sync_propagation_target() {
        let store = shared_colors(Some("#0064FF"), Some("#65D08A"));
        let records = records_with_worktrees(&[Some(ROOT), Some(FEATURE)]);

        ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#0064FF"), None));

        assert_eq!(
            store.calls_of("write_project_color"),
            [format!("write_project_color {FEATURE} #0064FF null")]
        );
    }

    fn records_with_database_color(
        color: &str,
        worktree_paths: &[Option<&str>],
    ) -> RecordingProjectRecords {
        let records = records_with_worktrees(worktree_paths);
        *records.project.lock().unwrap() = Some(project(Some(color), None));
        records
    }

    // parity: TS-main-kanvibeProjectColorService-471239
    #[test]
    fn missing_shared_color_is_seeded_while_keeping_the_existing_color() {
        let store = shared_colors(None, None);
        let records = records_with_database_color("#65D08A", &[]);
        let synced_project = project(Some("#65D08A"), None);

        let synced = ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&synced_project);

        assert_eq!(synced, None);
        assert_eq!(synced_project.color.as_deref(), Some("#65D08A"));
        assert_eq!(
            store.calls_of("write_project_color_if_absent"),
            [format!("write_project_color_if_absent {ROOT} #65D08A null")]
        );
    }

    // parity: TS-main-kanvibeProjectColorService-cedd6d
    #[test]
    fn the_seed_uses_the_current_database_color_not_the_stale_snapshot() {
        let store = shared_colors(None, None);
        let records = records_with_database_color("#0064FF", &[]);

        ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(
            store.calls_of("write_project_color_if_absent"),
            [format!("write_project_color_if_absent {ROOT} #0064FF null")]
        );
    }

    // parity: TS-main-kanvibeProjectColorService-558d15
    #[test]
    fn the_seed_is_written_only_to_the_authoritative_root() {
        let store = shared_colors(None, None);
        let records = records_with_database_color("#65D08A", &[Some(FEATURE)]);

        ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(
            store.calls_of("write_project_color_if_absent"),
            [format!("write_project_color_if_absent {ROOT} #65D08A null")]
        );
        assert!(store.calls_of("write_project_color").is_empty());
    }

    // parity: TS-main-kanvibeProjectColorService-f430ac
    #[test]
    fn a_failed_seed_does_not_stop_the_sync() {
        let store = RecordingStateStore {
            failure: Box::new(|function_name| {
                (function_name == "write_project_color_if_absent").then(|| "read-only".to_string())
            }),
            ..RecordingStateStore::default()
        };
        let records = records_with_database_color("#65D08A", &[]);

        let synced = ProjectColorService::new(&store, &records)
            .sync_project_color_with_kanvibe_state(&project(Some("#65D08A"), None));

        assert_eq!(synced, None);
    }

    // parity: TS-main-kanvibeProjectColorService-c5b9d4
    #[test]
    fn a_failed_color_read_does_not_stop_registration() {
        let store = RecordingStateStore {
            project_color: Box::new(|_| Err(HostFileError::Message("permission denied".into()))),
            ..RecordingStateStore::default()
        };
        let records = records_with_worktrees(&[]);

        assert_eq!(
            ProjectColorService::new(&store, &records).read_shared_project_color(ROOT, None),
            None
        );
    }
}
