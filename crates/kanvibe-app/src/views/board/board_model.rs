//! 칸반 보드의 화면 상태와 순수 규칙(TS `Board.tsx`의 `buildDragMovePlan`·`restoreMovedTask`·Done 페이지·요청 상태).
//! GPUI 없이 시험할 수 있도록 뷰와 떨어뜨려 둔다.

use std::collections::{BTreeMap, HashMap};

use kanvibe_domain::board_sort::BoardSortPreference;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TASK_STATUS_ORDER, TaskPriority, TaskStatus};
use kanvibe_services::kanban::TasksByStatus;

use super::task_filters::{
    BoardSortContext, ProjectDisplay, TaskKindFilter, build_project_display_map,
    build_project_filter_set, build_project_root_priority_map, is_on_desktops,
    matches_project_filter, matches_task_kind_filter, pick_available_desktops,
    sort_tasks_for_board,
};

pub fn column(tasks: &TasksByStatus, status: TaskStatus) -> &Vec<KanbanTask> {
    match status {
        TaskStatus::Todo => &tasks.todo,
        TaskStatus::Progress => &tasks.progress,
        TaskStatus::Pending => &tasks.pending,
        TaskStatus::Review => &tasks.review,
        TaskStatus::Done => &tasks.done,
    }
}

pub fn column_mut(tasks: &mut TasksByStatus, status: TaskStatus) -> &mut Vec<KanbanTask> {
    match status {
        TaskStatus::Todo => &mut tasks.todo,
        TaskStatus::Progress => &mut tasks.progress,
        TaskStatus::Pending => &mut tasks.pending,
        TaskStatus::Review => &mut tasks.review,
        TaskStatus::Done => &mut tasks.done,
    }
}

pub fn find_task<'a>(tasks: &'a TasksByStatus, task_id: &str) -> Option<&'a KanbanTask> {
    TASK_STATUS_ORDER
        .into_iter()
        .flat_map(|status| column(tasks, status))
        .find(|task| task.id == task_id)
}

/// 보드에서 시작한 삭제 요청의 진행(task의 업무 상태와 따로 둔다)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDeletionState {
    Deleting,
    Failed { reason: String },
}

/// 옮긴 카드의 저장 진행. 실패하면 카드는 원래 자리로 돌아가고 다시 옮길 목표를 기억한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskMoveState {
    Saving,
    Failed {
        reason: String,
        target_status: TaskStatus,
    },
}

/// 카드를 다른 열로 옮기는 낙관적 변경과 저장할 값
#[derive(Debug, Clone, PartialEq)]
pub struct DragMovePlan {
    pub updated_tasks: TasksByStatus,
    pub done_delta: i64,
    pub task_id: String,
    pub target_status: TaskStatus,
    /// 저장에 실패하면 이 task만 원래 열·자리로 돌린다
    pub original: KanbanTask,
    pub original_index: usize,
}

/// 다른 열로 옮긴 카드는 방금 수정한 카드이므로 목적지 맨 위에 놓는다. 같은 열 안의 드롭은 아무것도 바꾸지 않는다
pub fn build_drag_move_plan(
    tasks: &TasksByStatus,
    task_id: &str,
    target_status: TaskStatus,
) -> Option<DragMovePlan> {
    let (source_status, source_index) = TASK_STATUS_ORDER.into_iter().find_map(|status| {
        column(tasks, status)
            .iter()
            .position(|task| task.id == task_id)
            .map(|index| (status, index))
    })?;
    if source_status == target_status {
        return None;
    }
    let original = column(tasks, source_status)[source_index].clone();
    let mut updated_tasks = tasks.clone();
    column_mut(&mut updated_tasks, source_status).remove(source_index);
    let moved = KanbanTask {
        status: target_status,
        ..original.clone()
    };
    column_mut(&mut updated_tasks, target_status).insert(0, moved);
    let done_delta =
        i64::from(target_status == TaskStatus::Done) - i64::from(source_status == TaskStatus::Done);
    Some(DragMovePlan {
        updated_tasks,
        done_delta,
        task_id: task_id.to_string(),
        target_status,
        original,
        original_index: source_index,
    })
}

/// 저장에 실패한 task 하나만 원래 자리로 돌린다. 그 사이 다른 카드에 생긴 변화는 덮지 않는다
pub fn restore_moved_task(tasks: &mut TasksByStatus, plan: &DragMovePlan) {
    let moved_column = column_mut(tasks, plan.target_status);
    let Some(moved_index) = moved_column.iter().position(|task| task.id == plan.task_id) else {
        return;
    };
    moved_column.remove(moved_index);
    let original_column = column_mut(tasks, plan.original.status);
    let index = plan.original_index.min(original_column.len());
    original_column.insert(index, plan.original.clone());
}

/// 지운 task를 모든 열에서 뺀다. 뺐으면 `true`
pub fn remove_task(tasks: &mut TasksByStatus, task_id: &str) -> bool {
    let mut is_removed = false;
    for status in TASK_STATUS_ORDER {
        let tasks = column_mut(tasks, status);
        let before = tasks.len();
        tasks.retain(|task| task.id != task_id);
        is_removed |= tasks.len() != before;
    }
    is_removed
}

/// Done 다음 페이지를 붙인다. 이미 있는 task는 한 번만 남긴다
pub fn append_done_page(tasks: &mut TasksByStatus, page: Vec<KanbanTask>) {
    for task in page {
        if let Some(existing) = tasks
            .done
            .iter_mut()
            .find(|existing| existing.id == task.id)
        {
            *existing = task;
        } else {
            tasks.done.push(task);
        }
    }
}

/// Done 더 보기 진행. 새로 받은 첫 페이지가 목록을 바꾸면 세대가 올라가 이전 요청의 응답을 버린다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DonePagination {
    pub total: usize,
    pub offset: usize,
    pub generation: u64,
    pub loading_generation: Option<u64>,
    pub error: Option<String>,
}

impl DonePagination {
    pub fn new(total: usize, first_page_size: usize) -> Self {
        Self {
            total,
            offset: first_page_size,
            ..Self::default()
        }
    }

    pub fn has_more(&self) -> bool {
        self.offset < self.total
    }

    pub fn is_loading(&self) -> bool {
        self.loading_generation == Some(self.generation)
    }

    /// 새로 받은 첫 페이지로 바꾼다. 진행 중이던 더 보기는 버린다
    pub fn reset(&mut self, total: usize, first_page_size: usize) {
        let generation = self.generation + 1;
        *self = Self {
            generation,
            ..Self::new(total, first_page_size)
        };
    }

    /// 요청을 시작한다. 이미 같은 세대 요청이 있으면 `None`
    pub fn begin(&mut self) -> Option<(u64, usize)> {
        if self.is_loading() {
            return None;
        }
        self.loading_generation = Some(self.generation);
        self.error = None;
        Some((self.generation, self.offset))
    }

    /// 응답을 반영할지. 세대가 바뀌었으면 버린다
    pub fn finish(&mut self, generation: u64, result: Result<(usize, usize), String>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading_generation = None;
        match result {
            Ok((fetched_count, total)) => {
                self.offset += fetched_count;
                self.total = total;
                true
            }
            Err(reason) => {
                self.error = Some(reason);
                false
            }
        }
    }

    pub fn apply_delta(&mut self, delta: i64) {
        self.total = (self.total as i64 + delta).max(0) as usize;
        self.offset = (self.offset as i64 + delta).max(0) as usize;
    }
}

/// 헤더 필터 선택
#[derive(Debug, Clone, PartialEq)]
pub struct BoardFilters {
    pub task_kind: TaskKindFilter,
    pub selected_project_ids: Vec<String>,
    pub selected_desktops: Vec<String>,
}

impl Default for BoardFilters {
    fn default() -> Self {
        Self {
            task_kind: TaskKindFilter::All,
            selected_project_ids: Vec::new(),
            selected_desktops: Vec::new(),
        }
    }
}

/// 보드가 받은 프로젝트로 만든 조회표
#[derive(Debug, Clone, Default)]
pub struct ProjectCatalog {
    pub projects: Vec<Project>,
    pub by_id: HashMap<String, Project>,
    pub displays: BTreeMap<String, ProjectDisplay>,
    pub names: HashMap<String, String>,
}

impl ProjectCatalog {
    pub fn new(projects: Vec<Project>) -> Self {
        let displays = build_project_display_map(&projects);
        let names = displays
            .iter()
            .map(|(id, display)| (id.clone(), display.name.clone()))
            .collect();
        Self {
            by_id: projects
                .iter()
                .map(|project| (project.id.clone(), project.clone()))
                .collect(),
            projects,
            displays,
            names,
        }
    }

    /// 데스크톱 필터 후보: 등록된 SSH 호스트, 프로젝트·task가 쓰는 호스트(정렬, 중복 없음)
    pub fn desktop_hosts(&self, ssh_hosts: &[String], tasks: &TasksByStatus) -> Vec<String> {
        let project_hosts = self
            .projects
            .iter()
            .filter_map(|project| project.ssh_host.clone());
        let task_hosts = TASK_STATUS_ORDER
            .into_iter()
            .flat_map(|status| column(tasks, status))
            .filter_map(|task| task.ssh_host.clone());
        let mut hosts: Vec<String> = ssh_hosts
            .iter()
            .cloned()
            .chain(project_hosts)
            .chain(task_hosts)
            .filter(|host| !host.is_empty())
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// 프로젝트 필터 목록: worktree를 뺀 메인 프로젝트 중 고른 데스크톱의 것
    pub fn filterable_projects(&self, active_desktops: &[String]) -> Vec<Project> {
        self.projects
            .iter()
            .filter(|project| {
                !project.is_worktree && is_on_desktops(project.ssh_host.as_deref(), active_desktops)
            })
            .cloned()
            .collect()
    }
}

/// 필터·정렬을 거쳐 화면에 늘어놓을 열
pub fn displayed_tasks(
    tasks: &TasksByStatus,
    filters: &BoardFilters,
    catalog: &ProjectCatalog,
    desktop_hosts: &[String],
    sort_preference: &BoardSortPreference,
) -> TasksByStatus {
    let filter_set = build_project_filter_set(&filters.selected_project_ids, &catalog.projects);
    let active_desktops = pick_available_desktops(&filters.selected_desktops, desktop_hosts);
    let root_priorities = root_priorities(tasks, catalog);
    let context = BoardSortContext {
        root_priorities: &root_priorities,
        project_names: &catalog.names,
    };
    let is_visible = |task: &&KanbanTask| {
        let host = task.ssh_host.clone().or_else(|| {
            task.project_id
                .as_ref()
                .and_then(|id| catalog.by_id.get(id))
                .and_then(|project| project.ssh_host.clone())
        });
        matches_project_filter(task, filter_set.as_ref())
            && matches_task_kind_filter(task, filters.task_kind, &catalog.by_id)
            && is_on_desktops(host.as_deref(), &active_desktops)
    };
    let mut displayed = TasksByStatus::default();
    for status in TASK_STATUS_ORDER {
        let visible: Vec<KanbanTask> = column(tasks, status)
            .iter()
            .filter(is_visible)
            .cloned()
            .collect();
        *column_mut(&mut displayed, status) =
            sort_tasks_for_board(&visible, sort_preference, &context);
    }
    displayed
}

pub fn root_priorities(
    tasks: &TasksByStatus,
    catalog: &ProjectCatalog,
) -> HashMap<String, TaskPriority> {
    let all_tasks = TASK_STATUS_ORDER
        .into_iter()
        .flat_map(|status| column(tasks, status));
    build_project_root_priority_map(all_tasks, &catalog.by_id)
}

/// 열 안에서 같은 프로젝트 묶음이 이어지는 구간. 프로젝트가 없는 task는 묶지 않는다
#[derive(Debug, Clone, PartialEq)]
pub struct TaskGroup<'a> {
    pub project_name: Option<String>,
    pub tasks: Vec<&'a KanbanTask>,
}

pub fn contiguous_groups<'a>(
    tasks: &'a [KanbanTask],
    catalog: &ProjectCatalog,
) -> Vec<TaskGroup<'a>> {
    let mut groups: Vec<(Option<String>, TaskGroup<'a>)> = Vec::new();
    for task in tasks {
        let display = task
            .project_id
            .as_ref()
            .and_then(|id| catalog.displays.get(id));
        let key = task.project_id.as_ref().map(|id| {
            display
                .map(|display| display.group_key.clone())
                .unwrap_or_else(|| id.clone())
        });
        match groups.last_mut() {
            Some((last_key, group)) if *last_key == key => group.tasks.push(task),
            _ => groups.push((
                key,
                TaskGroup {
                    project_name: display.map(|display| display.name.clone()),
                    tasks: vec![task],
                },
            )),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// 이 카드에 Done 이동 안내를 보여야 하는지. 정리할 브랜치나 세션이 있는 카드만 묻는다
pub fn should_confirm_done_move(
    task: &KanbanTask,
    target_status: TaskStatus,
    is_done_alert_dismissed: bool,
) -> bool {
    target_status == TaskStatus::Done
        && task.status != TaskStatus::Done
        && !is_done_alert_dismissed
        && (task.branch_name.is_some() || task.session_type.is_some())
}

/// 진행 중 열에서 브랜치·worktree가 있는 task만 git 집계를 다시 돌린다
pub fn task_ids_to_refresh_diff_stats(displayed: &TasksByStatus) -> Vec<String> {
    displayed
        .progress
        .iter()
        .filter(|task| task.branch_name.is_some() && task.worktree_path.is_some())
        .map(|task| task.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::board::task_filters::tests::{project, task};
    use kanvibe_domain::board_sort::{BoardSortDirection, BoardSortField, BoardSortKey};
    use kanvibe_domain::task::SessionType;

    fn board(columns: &[(TaskStatus, &[&str])]) -> TasksByStatus {
        let mut tasks = TasksByStatus::default();
        for (status, ids) in columns {
            *column_mut(&mut tasks, *status) = ids
                .iter()
                .map(|id| KanbanTask {
                    status: *status,
                    ..task(id)
                })
                .collect();
        }
        tasks
    }

    fn ids(tasks: &[KanbanTask]) -> Vec<&str> {
        tasks.iter().map(|task| task.id.as_str()).collect()
    }

    // parity: TS-components-Board-cfa96d
    #[test]
    fn dropping_inside_the_same_column_saves_nothing() {
        let tasks = board(&[(TaskStatus::Todo, &["a", "b"])]);
        assert_eq!(build_drag_move_plan(&tasks, "b", TaskStatus::Todo), None);
    }

    // parity: TS-components-Board-1d27b8
    #[test]
    fn dropping_on_another_column_saves_only_the_status_and_puts_the_card_on_top() {
        let tasks = board(&[
            (TaskStatus::Todo, &["a", "b"]),
            (TaskStatus::Review, &["r"]),
        ]);
        let plan = build_drag_move_plan(&tasks, "b", TaskStatus::Review).unwrap();
        assert_eq!(
            (plan.task_id.as_str(), plan.target_status),
            ("b", TaskStatus::Review)
        );
        assert_eq!(ids(&plan.updated_tasks.review), ["b", "r"]);
        assert_eq!(plan.updated_tasks.review[0].status, TaskStatus::Review);
        assert_eq!(ids(&plan.updated_tasks.todo), ["a"]);
    }

    // parity: TS-components-Board-cfa576
    #[test]
    fn failed_move_returns_only_that_card_to_its_original_slot() {
        let tasks = board(&[
            (TaskStatus::Todo, &["a", "b", "c"]),
            (TaskStatus::Progress, &["p"]),
        ]);
        let plan = build_drag_move_plan(&tasks, "b", TaskStatus::Progress).unwrap();
        let mut current = plan.updated_tasks.clone();
        // 저장을 기다리는 동안 다른 카드가 지워져도 그 변화는 남는다
        remove_task(&mut current, "c");
        restore_moved_task(&mut current, &plan);
        assert_eq!(ids(&current.todo), ["a", "b"]);
        assert_eq!(current.todo[1].status, TaskStatus::Todo);
        assert_eq!(ids(&current.progress), ["p"]);
    }

    // parity: TS-components-Board-e9f9c5
    #[test]
    fn done_pages_are_deduplicated_while_offset_advances_by_fetched_size() {
        let mut tasks = board(&[(TaskStatus::Done, &["d1", "d2"])]);
        let mut pagination = DonePagination::new(4, 2);
        let (generation, offset) = pagination.begin().unwrap();
        assert_eq!(offset, 2);
        let page = vec![task("d2"), task("d3")];
        append_done_page(&mut tasks, page);
        assert!(pagination.finish(generation, Ok((2, 4))));
        assert_eq!(ids(&tasks.done), ["d1", "d2", "d3"]);
        assert_eq!(pagination.offset, 4);
        assert!(!pagination.has_more());
    }

    // parity: TS-components-Board-2b19ae
    #[test]
    fn refreshed_first_page_discards_a_pending_done_page() {
        let mut pagination = DonePagination::new(40, 20);
        let (stale_generation, _) = pagination.begin().unwrap();
        pagination.reset(41, 20);
        assert!(!pagination.finish(stale_generation, Ok((20, 41))));
        assert_eq!(pagination.offset, 20);
    }

    // parity: TS-components-Board-978561
    #[test]
    fn stale_done_completion_does_not_clear_the_replacement_loading_state() {
        let mut pagination = DonePagination::new(40, 20);
        let (stale_generation, _) = pagination.begin().unwrap();
        pagination.reset(40, 20);
        pagination.begin().unwrap();
        pagination.finish(stale_generation, Err("late".into()));
        assert!(pagination.is_loading());
        assert_eq!(pagination.error, None);
    }

    // parity: TS-components-Board-9098bc
    #[test]
    fn failed_more_done_keeps_the_reason_and_retries_the_same_page() {
        let mut pagination = DonePagination::new(40, 20);
        let (generation, offset) = pagination.begin().unwrap();
        pagination.finish(generation, Err("database is locked".into()));
        assert_eq!(pagination.error.as_deref(), Some("database is locked"));
        assert!(pagination.has_more());
        let (_, retry_offset) = pagination.begin().unwrap();
        assert_eq!(retry_offset, offset);
        assert_eq!(pagination.error, None);
    }

    fn root_and_child_catalog() -> (TasksByStatus, ProjectCatalog) {
        let catalog = ProjectCatalog::new(vec![project("project-1")]);
        let tasks = TasksByStatus {
            todo: vec![
                KanbanTask {
                    project_id: Some("project-1".into()),
                    branch_name: Some("main".into()),
                    ..task("root")
                },
                KanbanTask {
                    project_id: Some("project-1".into()),
                    branch_name: Some("feat/a".into()),
                    ..task("child")
                },
                task("loose"),
            ],
            ..TasksByStatus::default()
        };
        (tasks, catalog)
    }

    fn displayed_todo_ids(kind: TaskKindFilter) -> Vec<String> {
        let (tasks, catalog) = root_and_child_catalog();
        let filters = BoardFilters {
            task_kind: kind,
            ..BoardFilters::default()
        };
        let displayed = displayed_tasks(
            &tasks,
            &filters,
            &catalog,
            &[],
            &BoardSortPreference::default(),
        );
        displayed.todo.into_iter().map(|task| task.id).collect()
    }

    // parity: TS-components-Board-af4985
    #[test]
    fn project_filter_shows_only_default_branch_root_tasks() {
        assert_eq!(displayed_todo_ids(TaskKindFilter::Project), ["root"]);
    }

    // parity: TS-components-Board-f7f3cd
    #[test]
    fn task_filter_hides_project_roots() {
        assert_eq!(displayed_todo_ids(TaskKindFilter::Task), ["child", "loose"]);
    }

    // parity: TS-components-Board-7360ad
    #[test]
    fn all_filter_shows_roots_and_ordinary_tasks() {
        assert_eq!(
            displayed_todo_ids(TaskKindFilter::All),
            ["root", "child", "loose"]
        );
    }

    fn priority_sorted_todo(tasks: &TasksByStatus, catalog: &ProjectCatalog) -> Vec<String> {
        let preference = BoardSortPreference {
            keys: vec![BoardSortKey {
                field: BoardSortField::Priority,
                direction: BoardSortDirection::Asc,
            }],
        };
        displayed_tasks(tasks, &BoardFilters::default(), catalog, &[], &preference)
            .todo
            .into_iter()
            .map(|task| task.id)
            .collect()
    }

    // parity: TS-components-Board-ea02ad
    #[test]
    fn priority_sort_orders_the_cards() {
        let tasks = TasksByStatus {
            todo: vec![
                KanbanTask {
                    priority: Some(TaskPriority::Low),
                    ..task("low")
                },
                KanbanTask {
                    priority: Some(TaskPriority::High),
                    ..task("high")
                },
            ],
            ..TasksByStatus::default()
        };
        assert_eq!(
            priority_sorted_todo(&tasks, &ProjectCatalog::default()),
            ["high", "low"]
        );
    }

    // parity: TS-components-Board-e0b147
    #[test]
    fn without_sort_keys_the_database_order_is_kept() {
        let tasks = board(&[(TaskStatus::Todo, &["b", "a", "c"])]);
        let displayed = displayed_tasks(
            &tasks,
            &BoardFilters::default(),
            &ProjectCatalog::default(),
            &[],
            &BoardSortPreference::default(),
        );
        assert_eq!(ids(&displayed.todo), ["b", "a", "c"]);
    }

    // parity: TS-components-Board-154d47
    #[test]
    fn inherited_root_priority_is_used_when_sorting_cards() {
        let (mut tasks, catalog) = root_and_child_catalog();
        tasks.todo[0].priority = Some(TaskPriority::High);
        tasks.todo[2].priority = Some(TaskPriority::Medium);
        tasks.todo.reverse();
        assert_eq!(
            priority_sorted_todo(&tasks, &catalog),
            ["child", "root", "loose"]
        );
    }

    // parity: TS-components-Board-3bf9ed
    #[test]
    fn only_progress_cards_with_worktrees_are_refreshed() {
        let mut tasks = TasksByStatus::default();
        let with_worktree = KanbanTask {
            branch_name: Some("feat/a".into()),
            worktree_path: Some("/repo__worktrees/a".into()),
            ..task("p1")
        };
        tasks.progress = vec![with_worktree.clone(), task("p2")];
        tasks.review = vec![KanbanTask {
            id: "r1".into(),
            ..with_worktree
        }];
        assert_eq!(task_ids_to_refresh_diff_stats(&tasks), ["p1"]);
    }

    #[test]
    fn done_confirmation_is_only_needed_for_cards_with_resources() {
        let branch_task = KanbanTask {
            branch_name: Some("feat/a".into()),
            ..task("a")
        };
        let session_task = KanbanTask {
            session_type: Some(SessionType::Tmux),
            ..task("b")
        };
        assert!(should_confirm_done_move(
            &branch_task,
            TaskStatus::Done,
            false
        ));
        assert!(should_confirm_done_move(
            &session_task,
            TaskStatus::Done,
            false
        ));
        assert!(!should_confirm_done_move(
            &task("c"),
            TaskStatus::Done,
            false
        ));
        assert!(!should_confirm_done_move(
            &branch_task,
            TaskStatus::Done,
            true
        ));
        assert!(!should_confirm_done_move(
            &branch_task,
            TaskStatus::Review,
            false
        ));
    }

    // parity: TS-components-Board-8b6a23
    #[test]
    fn desktop_filter_narrows_project_filter_candidates() {
        let local = project("local");
        let remote = Project {
            ssh_host: Some("devbox".into()),
            ..project("remote")
        };
        let worktree = Project {
            is_worktree: true,
            ..project("wt")
        };
        let catalog = ProjectCatalog::new(vec![local, remote, worktree]);
        let names = |projects: Vec<Project>| {
            projects
                .into_iter()
                .map(|project| project.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(catalog.filterable_projects(&[])), ["local", "remote"]);
        assert_eq!(
            names(catalog.filterable_projects(&["ssh:devbox".to_string()])),
            ["remote"]
        );
    }

    #[test]
    fn contiguous_groups_merge_adjacent_tasks_of_the_same_project() {
        let catalog = ProjectCatalog::new(vec![project("p1"), project("p2")]);
        let in_project = |id: &str, project_id: &str| KanbanTask {
            project_id: Some(project_id.into()),
            ..task(id)
        };
        let tasks = vec![
            in_project("a", "p1"),
            in_project("b", "p1"),
            task("loose"),
            in_project("c", "p2"),
        ];
        let groups = contiguous_groups(&tasks, &catalog);
        let sizes: Vec<_> = groups
            .iter()
            .map(|group| (group.project_name.clone(), group.tasks.len()))
            .collect();
        assert_eq!(
            sizes,
            [
                (Some("p1".to_string()), 2),
                (None, 1),
                (Some("p2".to_string()), 1)
            ]
        );
    }
}
