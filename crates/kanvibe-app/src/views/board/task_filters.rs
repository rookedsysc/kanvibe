//! 보드·워크스페이스가 함께 쓰는 거르기·정렬·프로젝트 표시 규칙.
//! TS `boardTaskSort.ts`, `useTaskKindFilterParams.ts`, `projectDisplay.ts`, `DesktopFilter.tsx`의 순수 함수를 옮겼다.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::text_collation::LocaleCollator;
use kanvibe_domain::board_sort::{
    BoardSortDirection, BoardSortField, BoardSortKey, BoardSortPreference,
};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskPriority};
use kanvibe_process::project_color::compute_project_color;

/// 헤더 세그먼트 `Project/Task/All`의 값(TS `TaskKindFilter`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKindFilter {
    Project,
    Task,
    All,
}

impl TaskKindFilter {
    pub const ORDER: [TaskKindFilter; 3] = [Self::Project, Self::Task, Self::All];
    /// URL 쿼리 이름(`?taskKind=project`)
    pub const QUERY_KEY: &'static str = "taskKind";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Task => "task",
            Self::All => "all",
        }
    }

    /// 모르는 값은 `All`이다
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("project") => Self::Project,
            Some("task") => Self::Task,
            _ => Self::All,
        }
    }
}

/// 프로젝트의 기본 브랜치를 들고 있는 task가 그 프로젝트의 root task다
pub fn is_project_root_task(task: &KanbanTask, projects: &HashMap<String, Project>) -> bool {
    let (Some(project_id), Some(branch_name)) = (&task.project_id, &task.branch_name) else {
        return false;
    };
    projects
        .get(project_id)
        .is_some_and(|project| !project.is_worktree && *branch_name == project.default_branch)
}

pub fn matches_task_kind_filter(
    task: &KanbanTask,
    filter: TaskKindFilter,
    projects: &HashMap<String, Project>,
) -> bool {
    match filter {
        TaskKindFilter::All => true,
        TaskKindFilter::Project => is_project_root_task(task, projects),
        TaskKindFilter::Task => !is_project_root_task(task, projects),
    }
}

/// 선택한 메인 프로젝트와 그 worktree 프로젝트의 ID 집합. 선택이 없으면 `None`이라 전체를 보여 준다
pub fn build_project_filter_set(
    selected_project_ids: &[String],
    projects: &[Project],
) -> Option<HashSet<String>> {
    let mut matching_ids = HashSet::new();
    for selected_id in selected_project_ids {
        let Some(main_project) = projects.iter().find(|project| &project.id == selected_id) else {
            continue;
        };
        matching_ids.insert(main_project.id.clone());
        let worktree_prefix = format!("{}__worktrees", main_project.repo_path);
        for project in projects {
            if project.repo_path.starts_with(&worktree_prefix) {
                matching_ids.insert(project.id.clone());
            }
        }
    }
    (!matching_ids.is_empty()).then_some(matching_ids)
}

pub fn matches_project_filter(task: &KanbanTask, filter_set: Option<&HashSet<String>>) -> bool {
    match filter_set {
        None => true,
        Some(ids) => task
            .project_id
            .as_ref()
            .is_some_and(|project_id| ids.contains(project_id)),
    }
}

/// 필터 선택값 형식의 데스크톱 이름(`local`, `ssh:<host>`)
pub fn desktop_key(ssh_host: Option<&str>) -> String {
    match ssh_host {
        Some(host) if !host.is_empty() => format!("ssh:{host}"),
        _ => "local".to_string(),
    }
}

/// 지금 보드에 없는 호스트를 고른 값은 빼서, 사라진 호스트 때문에 보드가 비지 않게 한다
pub fn pick_available_desktops(selected: &[String], hosts: &[String]) -> Vec<String> {
    let mut available: HashSet<String> = hosts.iter().map(|host| desktop_key(Some(host))).collect();
    available.insert("local".to_string());
    selected
        .iter()
        .filter(|desktop| available.contains(*desktop))
        .cloned()
        .collect()
}

/// 데스크톱 선택이 비어 있으면 전체를 뜻한다
pub fn is_on_desktops(ssh_host: Option<&str>, active_desktops: &[String]) -> bool {
    active_desktops.is_empty() || active_desktops.contains(&desktop_key(ssh_host))
}

/// 프로젝트별 root task 우선순위. 같은 프로젝트의 다른 task가 이 값을 물려받는다
pub fn build_project_root_priority_map<'a>(
    tasks: impl IntoIterator<Item = &'a KanbanTask>,
    projects: &HashMap<String, Project>,
) -> HashMap<String, TaskPriority> {
    let mut root_priorities = HashMap::new();
    for task in tasks {
        let (Some(priority), Some(project_id)) = (task.priority, &task.project_id) else {
            continue;
        };
        if is_project_root_task(task, projects) {
            root_priorities.insert(project_id.clone(), priority);
        }
    }
    root_priorities
}

/// task 자기 우선순위가 이기고, 없으면 프로젝트 root task의 값을 물려받는다
pub fn resolve_effective_priority(
    task: &KanbanTask,
    root_priorities: &HashMap<String, TaskPriority>,
) -> Option<TaskPriority> {
    task.priority.or_else(|| {
        task.project_id
            .as_ref()
            .and_then(|project_id| root_priorities.get(project_id).copied())
    })
}

pub fn is_inherited_priority(
    task: &KanbanTask,
    root_priorities: &HashMap<String, TaskPriority>,
) -> bool {
    task.priority.is_none() && resolve_effective_priority(task, root_priorities).is_some()
}

/// 정렬에 필요한 보드 상태. 제목·프로젝트 이름은 보고 있는 언어의 콜레이션(TS `localeCompare(locale)`)으로 비교한다
pub struct BoardSortContext<'a> {
    pub root_priorities: &'a HashMap<String, TaskPriority>,
    pub project_names: &'a HashMap<String, String>,
    pub collator: &'a LocaleCollator,
}

#[derive(Debug, PartialEq)]
enum SortValue<'a> {
    Number(i64),
    Text(&'a str),
}

/// 우선순위 오름차순은 "높은 순"이다
fn priority_rank(priority: TaskPriority) -> i64 {
    match priority {
        TaskPriority::High => 0,
        TaskPriority::Medium => 1,
        TaskPriority::Low => 2,
    }
}

/// DB 시각(`YYYY-MM-DD HH:MM:SS`)과 ISO 시각(`...T...Z`)을 같은 모양으로 맞춰 문자열 비교로 시간순을 얻는다
fn timestamp_key(value: &str) -> String {
    value.replace('T', " ").chars().take(23).collect()
}

fn read_sort_value<'a>(
    task: &'a KanbanTask,
    field: BoardSortField,
    context: &'a BoardSortContext<'a>,
) -> Option<SortValue<'a>> {
    match field {
        BoardSortField::Priority => resolve_effective_priority(task, context.root_priorities)
            .map(|priority| SortValue::Number(priority_rank(priority))),
        BoardSortField::CreatedAt => Some(SortValue::Text(task.created_at.as_str())),
        BoardSortField::UpdatedAt => Some(SortValue::Text(task.updated_at.as_str())),
        BoardSortField::Title => Some(SortValue::Text(task.title.as_str())),
        BoardSortField::Project => task
            .project_id
            .as_ref()
            .and_then(|project_id| context.project_names.get(project_id))
            .map(|name| SortValue::Text(name.as_str())),
    }
}

fn compare_sort_values(
    field: BoardSortField,
    left: &SortValue,
    right: &SortValue,
    collator: &LocaleCollator,
) -> Ordering {
    match (left, right) {
        (SortValue::Number(left), SortValue::Number(right)) => left.cmp(right),
        (SortValue::Text(left), SortValue::Text(right)) => match field {
            BoardSortField::CreatedAt | BoardSortField::UpdatedAt => {
                timestamp_key(left).cmp(&timestamp_key(right))
            }
            _ => collator.compare(left, right),
        },
        _ => Ordering::Equal,
    }
}

fn compare_by_sort_keys(
    left: &KanbanTask,
    right: &KanbanTask,
    keys: &[BoardSortKey],
    context: &BoardSortContext,
) -> Ordering {
    for key in keys {
        let left_value = read_sort_value(left, key.field, context);
        let right_value = read_sort_value(right, key.field, context);
        let ordering = match (&left_value, &right_value) {
            (None, None) => Ordering::Equal,
            // 값이 없는 항목은 방향과 상관없이 뒤로 간다
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(left_value), Some(right_value)) => {
                let ordering =
                    compare_sort_values(key.field, left_value, right_value, context.collator);
                match key.direction {
                    BoardSortDirection::Asc => ordering,
                    BoardSortDirection::Desc => ordering.reverse(),
                }
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

/// 한 열의 task를 정렬 설정대로 늘어놓는다. 기준이 없으면 들어온 순서(최근 수정순)를 그대로 둔다
pub fn sort_tasks_for_board(
    tasks: &[KanbanTask],
    preference: &BoardSortPreference,
    context: &BoardSortContext,
) -> Vec<KanbanTask> {
    let mut sorted = tasks.to_vec();
    if preference.keys.is_empty() {
        return sorted;
    }
    sorted.sort_by(|left, right| {
        compare_by_sort_keys(left, right, &preference.keys, context)
            .then_with(|| timestamp_key(&right.updated_at).cmp(&timestamp_key(&left.updated_at)))
    });
    sorted
}

/// 프로젝트 하나를 카드·필터에서 보여 줄 이름과 묶음 키. worktree 프로젝트는 메인 프로젝트 설정을 따른다
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDisplay {
    pub name: String,
    pub group_key: String,
    pub main_project: Option<Project>,
}

impl ProjectDisplay {
    /// 사용자가 고른 색, 없으면 이름에서 정해지는 색
    pub fn color(&self) -> String {
        self.main_project
            .as_ref()
            .and_then(|project| project.color.clone())
            .filter(|color| !color.is_empty())
            .unwrap_or_else(|| compute_project_color(&self.name).to_string())
    }

    pub fn icon_data_url(&self) -> Option<&str> {
        self.main_project
            .as_ref()
            .and_then(|project| project.icon_data_url.as_deref())
            .filter(|icon| !icon.is_empty())
    }
}

fn main_repo_path(repo_path: &str) -> Option<&str> {
    repo_path
        .find("__worktrees")
        .map(|index| &repo_path[..index])
}

/// 같은 호스트·같은 메인 경로 묶음 키(TS `JSON.stringify([sshHost, path])`)
fn location_key(ssh_host: Option<&str>, path: &str) -> String {
    serde_json::to_string(&(ssh_host, path)).unwrap_or_default()
}

pub fn build_project_display_map(projects: &[Project]) -> BTreeMap<String, ProjectDisplay> {
    let main_projects: HashMap<String, &Project> = projects
        .iter()
        .filter(|project| main_repo_path(&project.repo_path).is_none())
        .map(|project| {
            let key = location_key(project.ssh_host.as_deref(), &project.repo_path);
            (key, project)
        })
        .collect();
    projects
        .iter()
        .map(|project| {
            let main_path = main_repo_path(&project.repo_path);
            let group_key = location_key(
                project.ssh_host.as_deref(),
                main_path.unwrap_or(&project.repo_path),
            );
            let main_project = main_projects.get(&group_key).copied().cloned();
            let fallback_name = main_path
                .and_then(|path| path.rsplit('/').next())
                .filter(|name| !name.is_empty())
                .unwrap_or(&project.name)
                .to_string();
            let display = ProjectDisplay {
                name: main_project
                    .as_ref()
                    .map(|main| main.name.clone())
                    .unwrap_or(fallback_name),
                group_key,
                main_project,
            };
            (project.id.clone(), display)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kanvibe_domain::task::TaskStatus;

    pub fn task(id: &str) -> KanbanTask {
        KanbanTask {
            id: id.to_string(),
            title: id.to_string(),
            description: None,
            status: TaskStatus::Todo,
            branch_name: None,
            worktree_path: None,
            session_type: None,
            session_name: None,
            ssh_host: None,
            agent_type: None,
            project_id: None,
            base_branch: None,
            pr_url: None,
            priority: None,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
        }
    }

    pub fn project(id: &str) -> Project {
        Project {
            id: id.to_string(),
            name: id.to_string(),
            repo_path: format!("/repo/{id}"),
            default_branch: "main".to_string(),
            ssh_host: None,
            is_worktree: false,
            color: None,
            icon_data_url: None,
            created_at: "2026-01-01 00:00:00".to_string(),
        }
    }

    fn lookup(projects: &[Project]) -> HashMap<String, Project> {
        projects
            .iter()
            .map(|project| (project.id.clone(), project.clone()))
            .collect()
    }

    fn with_project(id: &str, project_id: &str) -> KanbanTask {
        KanbanTask {
            project_id: Some(project_id.to_string()),
            ..task(id)
        }
    }

    fn keys(keys: &[(BoardSortField, BoardSortDirection)]) -> BoardSortPreference {
        BoardSortPreference {
            keys: keys
                .iter()
                .map(|(field, direction)| BoardSortKey {
                    field: *field,
                    direction: *direction,
                })
                .collect(),
        }
    }

    #[test]
    fn title_sort_follows_the_collation_of_the_viewed_language() {
        let mut lee = task("lee");
        lee.title = "李".into();
        let mut chen = task("chen");
        chen.title = "陈".into();
        let preference = BoardSortPreference {
            keys: vec![kanvibe_domain::board_sort::BoardSortKey {
                field: BoardSortField::Title,
                direction: kanvibe_domain::board_sort::BoardSortDirection::Asc,
            }],
        };
        let sort_in = |locale: kanvibe_domain::i18n::Locale| {
            let (priorities, names) = (HashMap::new(), HashMap::new());
            let collator = LocaleCollator::for_locale(locale);
            let context = BoardSortContext {
                root_priorities: &priorities,
                project_names: &names,
                collator: &collator,
            };
            sort_tasks_for_board(&[chen.clone(), lee.clone()], &preference, &context)
                .into_iter()
                .map(|task| task.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(sort_in(kanvibe_domain::i18n::Locale::Ko), ["lee", "chen"]);
        assert_eq!(sort_in(kanvibe_domain::i18n::Locale::Zh), ["chen", "lee"]);
    }

    fn sorted_ids(
        tasks: &[KanbanTask],
        preference: &BoardSortPreference,
        root_priorities: &HashMap<String, TaskPriority>,
    ) -> Vec<String> {
        let names = HashMap::new();
        let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
        let context = BoardSortContext {
            root_priorities,
            project_names: &names,
            collator: &collator,
        };
        sort_tasks_for_board(tasks, preference, &context)
            .into_iter()
            .map(|task| task.id)
            .collect()
    }

    fn prioritized(id: &str, title: &str, priority: Option<TaskPriority>) -> KanbanTask {
        KanbanTask {
            title: title.to_string(),
            priority,
            ..task(id)
        }
    }

    #[test]
    fn tasks_without_priority_inherit_the_project_root_priority() {
        let child = with_project("child", "project-1");
        let roots = HashMap::from([("project-1".to_string(), TaskPriority::High)]);
        assert_eq!(
            resolve_effective_priority(&child, &roots),
            Some(TaskPriority::High)
        );
        assert!(is_inherited_priority(&child, &roots));
    }

    #[test]
    fn own_priority_overrides_the_inherited_one() {
        let child = KanbanTask {
            priority: Some(TaskPriority::Low),
            ..with_project("child", "project-1")
        };
        let roots = HashMap::from([("project-1".to_string(), TaskPriority::High)]);
        assert_eq!(
            resolve_effective_priority(&child, &roots),
            Some(TaskPriority::Low)
        );
        assert!(!is_inherited_priority(&child, &roots));
    }

    #[test]
    fn no_root_or_no_project_means_no_priority() {
        let roots = HashMap::from([("project-1".to_string(), TaskPriority::High)]);
        assert_eq!(resolve_effective_priority(&task("orphan"), &roots), None);
        assert_eq!(
            resolve_effective_priority(&with_project("other", "project-2"), &roots),
            None
        );
    }

    #[test]
    fn only_default_branch_tasks_count_as_roots() {
        let projects = lookup(&[project("project-1")]);
        let root = KanbanTask {
            branch_name: Some("main".into()),
            priority: Some(TaskPriority::Medium),
            ..with_project("root", "project-1")
        };
        let child = KanbanTask {
            branch_name: Some("feat/a".into()),
            priority: Some(TaskPriority::High),
            ..with_project("child", "project-1")
        };
        let roots = build_project_root_priority_map([&root, &child], &projects);
        assert_eq!(roots.get("project-1"), Some(&TaskPriority::Medium));
    }

    #[test]
    fn worktree_projects_are_never_roots() {
        let projects = lookup(&[Project {
            is_worktree: true,
            ..project("worktree-1")
        }]);
        let root = KanbanTask {
            branch_name: Some("main".into()),
            priority: Some(TaskPriority::High),
            ..with_project("root", "worktree-1")
        };
        assert!(build_project_root_priority_map([&root], &projects).is_empty());
    }

    #[test]
    fn no_sort_keys_keep_the_incoming_order() {
        let tasks = [
            prioritized("high", "c", Some(TaskPriority::High)),
            prioritized("medium", "a", Some(TaskPriority::Medium)),
            prioritized("none", "b", None),
        ];
        assert_eq!(
            sorted_ids(&tasks, &BoardSortPreference::default(), &HashMap::new()),
            ["high", "medium", "none"]
        );
    }

    #[test]
    fn later_keys_only_break_ties() {
        let tasks = [
            prioritized("first", "b", Some(TaskPriority::High)),
            prioritized("second", "a", Some(TaskPriority::High)),
            prioritized("last", "a", Some(TaskPriority::Low)),
        ];
        let preference = keys(&[
            (BoardSortField::Priority, BoardSortDirection::Asc),
            (BoardSortField::Title, BoardSortDirection::Asc),
        ]);
        assert_eq!(
            sorted_ids(&tasks, &preference, &HashMap::new()),
            ["second", "first", "last"]
        );
    }

    #[test]
    fn missing_values_stay_last_in_both_directions() {
        let tasks = [
            prioritized("none", "b", None),
            prioritized("medium", "a", Some(TaskPriority::Medium)),
            prioritized("high", "c", Some(TaskPriority::High)),
        ];
        let ascending = keys(&[(BoardSortField::Priority, BoardSortDirection::Asc)]);
        let descending = keys(&[(BoardSortField::Priority, BoardSortDirection::Desc)]);
        assert_eq!(
            sorted_ids(&tasks, &ascending, &HashMap::new()),
            ["high", "medium", "none"]
        );
        assert_eq!(
            sorted_ids(&tasks, &descending, &HashMap::new()),
            ["medium", "high", "none"]
        );
    }

    #[test]
    fn inherited_priority_is_used_for_sorting() {
        let tasks = [
            prioritized("medium", "a", Some(TaskPriority::Medium)),
            with_project("inheriting", "project-1"),
        ];
        let roots = HashMap::from([("project-1".to_string(), TaskPriority::High)]);
        let preference = keys(&[(BoardSortField::Priority, BoardSortDirection::Asc)]);
        assert_eq!(
            sorted_ids(&tasks, &preference, &roots),
            ["inheriting", "medium"]
        );
    }

    #[test]
    fn ties_fall_back_to_most_recently_updated() {
        let older = KanbanTask {
            updated_at: "2026-01-01 00:00:00".into(),
            ..prioritized("older", "older", Some(TaskPriority::High))
        };
        let newer = KanbanTask {
            updated_at: "2026-02-01T00:00:00.000Z".into(),
            ..prioritized("newer", "newer", Some(TaskPriority::High))
        };
        let preference = keys(&[(BoardSortField::Priority, BoardSortDirection::Asc)]);
        assert_eq!(
            sorted_ids(&[older, newer], &preference, &HashMap::new()),
            ["newer", "older"]
        );
    }

    #[test]
    fn missing_task_kind_query_defaults_to_all() {
        assert_eq!(TaskKindFilter::parse(None), TaskKindFilter::All);
    }

    #[test]
    fn unknown_task_kind_values_fall_back_to_all() {
        assert_eq!(TaskKindFilter::parse(Some("weird")), TaskKindFilter::All);
        assert_eq!(
            TaskKindFilter::parse(Some("project")),
            TaskKindFilter::Project
        );
    }

    #[test]
    fn worktrees_use_their_hosts_main_project_display_settings() {
        let local_main = Project {
            color: Some("#111111".into()),
            ..project("local-main")
        };
        let remote_main = Project {
            ssh_host: Some("devbox".into()),
            color: Some("#222222".into()),
            ..project("remote-main")
        };
        let remote_worktree = Project {
            id: "remote-wt".into(),
            name: "local-main".into(),
            repo_path: "/repo/remote-main__worktrees/feat".into(),
            ssh_host: Some("devbox".into()),
            is_worktree: true,
            ..project("remote-wt")
        };
        let displays = build_project_display_map(&[local_main, remote_main, remote_worktree]);
        let worktree = &displays["remote-wt"];
        assert_eq!(worktree.name, "remote-main");
        assert_eq!(worktree.color(), "#222222");
        assert_eq!(worktree.group_key, displays["remote-main"].group_key);
    }

    #[test]
    fn same_name_projects_on_different_hosts_get_different_groups() {
        let local = project("app");
        let remote = Project {
            id: "app-remote".into(),
            name: "app".into(),
            ssh_host: Some("devbox".into()),
            repo_path: "/repo/app".into(),
            ..project("app-remote")
        };
        let remote_worktree = Project {
            id: "app-remote-wt".into(),
            repo_path: "/repo/app__worktrees/feat".into(),
            ssh_host: Some("devbox".into()),
            is_worktree: true,
            ..project("app-remote-wt")
        };
        let displays = build_project_display_map(&[local, remote, remote_worktree]);
        assert_ne!(displays["app"].group_key, displays["app-remote"].group_key);
        assert_eq!(
            displays["app-remote"].group_key,
            displays["app-remote-wt"].group_key
        );
    }

    #[test]
    fn desktop_filters_drop_unknown_hosts_and_match_local_by_default() {
        let hosts = vec!["devbox".to_string()];
        let picked =
            pick_available_desktops(&["ssh:gone".to_string(), "ssh:devbox".to_string()], &hosts);
        assert_eq!(picked, ["ssh:devbox"]);
        assert!(is_on_desktops(None, &[]));
        assert!(!is_on_desktops(None, &picked));
        assert!(is_on_desktops(Some("devbox"), &picked));
    }
}
