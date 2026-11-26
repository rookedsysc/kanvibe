//! 작업 정보 카드의 편집기(TS `PriorityEditor`·`SessionTypeEditor`·`ProjectColorEditor`)와
//! 타이틀 카드의 같은 프로젝트 작업 모달(TS `ProjectBranchTasksModal`).
//! 저장은 TS `useTrackedAction`처럼 진행 중에는 다시 보내지 않고, 실패하면 사유를 남긴다.
//! 저장이 끝나면 받은 작업을 바로 반영하고 TS `router.refresh()`처럼 작업 상세 새로 고침 신호를 낸다.

pub mod branch_tasks;
pub mod color_picker;
pub mod priority;
pub mod project_color;
pub mod session_type;

use gpui_kit::{Context, Task};
use kanvibe_domain::task::KanbanTask;

use super::{DetailLoad, TaskDetailView};
use crate::app_events::{RefreshScope, trigger_refresh};

/// 진행 중인 저장 하나와 마지막 실패 사유(TS `useTrackedAction`의 `isPending`·`error`)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedSave {
    pub is_pending: bool,
    pub error: Option<String>,
}

impl TrackedSave {
    /// 진행 중이면 `false`를 돌려주고 아무것도 바꾸지 않는다
    pub fn begin(&mut self) -> bool {
        if self.is_pending {
            return false;
        }
        self.is_pending = true;
        self.error = None;
        true
    }

    pub fn finish(&mut self, error: Option<String>) {
        self.is_pending = false;
        self.error = error;
    }
}

#[derive(Default)]
pub struct EditorState {
    pub priority: TrackedSave,
    pub session_type: session_type::SessionTypeChange,
    pub project_color: project_color::ProjectColorEdit,
    pub branch_tasks: Option<branch_tasks::BranchTasksModal>,
    pub diff_summary: super::overview::DiffSummaryState,
    _priority_save: Option<Task<()>>,
    _session_type_change: Option<Task<()>>,
    _color_save: Option<Task<()>>,
    _branch_tasks_load: Option<Task<()>>,
}

impl TaskDetailView {
    /// 저장이 돌려준 작업을 화면에 바로 반영하고 새로 고침 신호를 낸다(TS `router.refresh()`)
    pub(in crate::views::task_detail) fn apply_saved_task(
        &mut self,
        saved: Option<KanbanTask>,
        cx: &mut Context<Self>,
    ) {
        if let (Some(saved), DetailLoad::Loaded(snapshot)) = (saved, &mut self.load) {
            std::rc::Rc::make_mut(snapshot).task = saved;
        }
        trigger_refresh(RefreshScope::TaskDetail, cx);
    }
}
