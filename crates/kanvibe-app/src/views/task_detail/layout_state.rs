//! 작업 상세의 패널·메인 화면 상태(TS `TaskDetailContent`의 `activePanel`·`mainView`·`defaultPanelDismissed`).
//! dock 항목 목록과 `Mod+{n}` 번호도 여기서 만든다. 번호는 `TASK_DETAIL_DOCK_ITEM_IDS` 배열 순서에서만 나온다.

use kanvibe_domain::task_detail_dock::{TASK_DETAIL_DOCK_ITEM_IDS, TaskDetailDockItemId};

/// 왼쪽 dock 옆에 겹쳐 여는 360px 패널
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPanel {
    Overview,
    Status,
    LiveSessions,
    Usage,
}

/// 메인 영역이 터미널인지 AI 대화 기록인지
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    Terminal,
    History,
}

/// dock 항목을 눌렀을 때 할 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockCommand {
    TogglePanel(DetailPanel),
    ToggleHistory,
    OpenPullRequest,
    OpenInVsCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockEntry {
    pub id: TaskDetailDockItemId,
    pub is_active: bool,
    /// PR 링크가 있으면 링크 모양(PR 색)으로 그린다
    pub href: Option<String>,
    /// 외부 명령(PR 조회·VS Code 실행)을 기다리는 중
    pub is_busy: bool,
    /// `Mod+{n}`의 n. 배열 위치 + 1이다
    pub shortcut_position: u8,
}

impl DockEntry {
    pub fn command(&self) -> DockCommand {
        dock_command(self.id)
    }
}

fn dock_command(id: TaskDetailDockItemId) -> DockCommand {
    match id {
        TaskDetailDockItemId::Overview => DockCommand::TogglePanel(DetailPanel::Overview),
        TaskDetailDockItemId::Status => DockCommand::TogglePanel(DetailPanel::Status),
        TaskDetailDockItemId::Chat => DockCommand::ToggleHistory,
        TaskDetailDockItemId::PullRequest => DockCommand::OpenPullRequest,
        TaskDetailDockItemId::LiveSessions => DockCommand::TogglePanel(DetailPanel::LiveSessions),
        TaskDetailDockItemId::Vscode => DockCommand::OpenInVsCode,
    }
}

/// 패널·메인 화면 상태. 작업 데이터를 읽기 전에는 기본 패널을 열지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutState {
    /// 사용자가 패널을 한 번이라도 닫거나 바꿨으면 기본 패널(작업 정보)을 다시 자동으로 열지 않는다
    pub is_default_panel_dismissed: bool,
    /// AppSettings `sidebar_default_collapsed`를 이번 조회에서 읽은 값. 읽기 전에는 `None`
    pub sidebar_default_collapsed: Option<bool>,
    pub active_panel: Option<DetailPanel>,
    pub main_view: MainView,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            is_default_panel_dismissed: false,
            sidebar_default_collapsed: None,
            active_panel: None,
            main_view: MainView::Terminal,
        }
    }
}

impl LayoutState {
    /// 지금 보이는 패널. 기본 펼침 설정이고 닫은 적이 없으면 작업 정보 패널이 열려 있다
    pub fn visible_panel(&self, has_task: bool) -> Option<DetailPanel> {
        let should_show_default_overview = has_task
            && self.sidebar_default_collapsed == Some(false)
            && !self.is_default_panel_dismissed;
        self.active_panel
            .or(should_show_default_overview.then_some(DetailPanel::Overview))
    }

    pub fn toggle_panel(&mut self, panel: DetailPanel, has_task: bool) {
        let visible = self.visible_panel(has_task);
        self.is_default_panel_dismissed = true;
        self.active_panel = (visible != Some(panel)).then_some(panel);
    }

    pub fn close_panel(&mut self) {
        self.is_default_panel_dismissed = true;
        self.active_panel = None;
    }

    pub fn toggle_history(&mut self) {
        self.close_panel();
        self.main_view = match self.main_view {
            MainView::History => MainView::Terminal,
            MainView::Terminal => MainView::History,
        };
    }

    /// dock 항목 목록. PR 자리는 링크가 없어도 비우지 않는다(뒤 항목 번호가 밀리지 않게)
    pub fn dock_entries(
        &self,
        has_task: bool,
        pr_url: Option<&str>,
        busy: DockBusyState,
    ) -> Vec<DockEntry> {
        let visible = self.visible_panel(has_task);
        TASK_DETAIL_DOCK_ITEM_IDS
            .into_iter()
            .zip(1_u8..)
            .map(|(id, shortcut_position)| {
                let is_active = match dock_command(id) {
                    DockCommand::TogglePanel(panel) => visible == Some(panel),
                    DockCommand::ToggleHistory => self.main_view == MainView::History,
                    DockCommand::OpenPullRequest | DockCommand::OpenInVsCode => false,
                };
                DockEntry {
                    id,
                    is_active,
                    href: (id == TaskDetailDockItemId::PullRequest)
                        .then(|| pr_url.map(str::to_string))
                        .flatten(),
                    is_busy: match id {
                        TaskDetailDockItemId::PullRequest => busy.is_opening_pull_request,
                        TaskDetailDockItemId::Vscode => busy.is_opening_vscode,
                        _ => false,
                    },
                    shortcut_position,
                }
            })
            .collect()
    }
}

/// 외부 명령을 기다리는 dock 자리
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DockBusyState {
    pub is_opening_pull_request: bool,
    pub is_opening_vscode: bool,
}

/// `Mod+{position}`이 가리키는 dock 항목. 범위 밖이면 `None`(TS `activateDockItem`이 false를 돌려 키를 흘려보낸다)
pub fn dock_entry_at(entries: &[DockEntry], position: u8) -> Option<&DockEntry> {
    let index = usize::from(position).checked_sub(1)?;
    entries.get(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_layout(sidebar_default_collapsed: bool) -> LayoutState {
        LayoutState {
            sidebar_default_collapsed: Some(sidebar_default_collapsed),
            ..LayoutState::default()
        }
    }

    // parity: TS-renderer-TaskDetailRoute-edc85b
    #[test]
    fn dock_order_follows_the_shared_dock_item_list() {
        let entries = loaded_layout(true).dock_entries(true, None, DockBusyState::default());
        let ids: Vec<TaskDetailDockItemId> = entries.iter().map(|entry| entry.id).collect();
        assert_eq!(ids, TASK_DETAIL_DOCK_ITEM_IDS.to_vec());
        let positions: Vec<u8> = entries
            .iter()
            .map(|entry| entry.shortcut_position)
            .collect();
        assert_eq!(positions, [1, 2, 3, 4, 5, 6]);
    }

    // parity: TS-renderer-TaskDetailRoute-64fce3
    #[test]
    fn live_sessions_stay_on_slot_five_without_a_pull_request() {
        let entries = loaded_layout(true).dock_entries(true, None, DockBusyState::default());
        let entry = dock_entry_at(&entries, 5).unwrap();
        assert_eq!(
            entry.command(),
            DockCommand::TogglePanel(DetailPanel::LiveSessions)
        );
        assert_eq!(
            dock_entry_at(&entries, 4).unwrap().command(),
            DockCommand::OpenPullRequest
        );
    }

    // parity: TS-renderer-TaskDetailRoute-5ecd4b
    #[test]
    fn pull_request_link_takes_slot_four_and_live_sessions_slot_five() {
        let pr_url = "https://github.com/acme/kanvibe-demo/pull/42";
        let entries =
            loaded_layout(true).dock_entries(true, Some(pr_url), DockBusyState::default());
        assert_eq!(
            dock_entry_at(&entries, 4).unwrap().href.as_deref(),
            Some(pr_url)
        );
        assert_eq!(
            dock_entry_at(&entries, 5).unwrap().id,
            TaskDetailDockItemId::LiveSessions
        );
    }

    // parity: TS-renderer-TaskDetailRoute-4d8119
    #[test]
    fn usage_is_not_a_dock_entry_and_does_not_shift_numbers() {
        let entries = loaded_layout(true).dock_entries(true, None, DockBusyState::default());
        assert_eq!(entries.len(), 6);
        assert!(dock_entry_at(&entries, 7).is_none());
        assert!(dock_entry_at(&entries, 0).is_none());
        assert!(
            entries
                .iter()
                .all(|entry| entry.command() != DockCommand::TogglePanel(DetailPanel::Usage))
        );
    }

    // parity: TS-renderer-TaskDetailRoute-ffb745
    #[test]
    fn default_expanded_setting_opens_the_overview_panel_until_dismissed() {
        let mut layout = loaded_layout(false);
        assert_eq!(layout.visible_panel(true), Some(DetailPanel::Overview));
        assert_eq!(layout.visible_panel(false), None);

        layout.close_panel();
        assert_eq!(layout.visible_panel(true), None);
    }

    // parity: TS-renderer-TaskDetailRoute-3e905c
    #[test]
    fn default_collapsed_setting_does_not_open_the_overview_panel() {
        assert_eq!(loaded_layout(true).visible_panel(true), None);
        assert_eq!(LayoutState::default().visible_panel(true), None);
    }

    // parity: TS-renderer-TaskDetailRoute-b4fb9c
    #[test]
    fn overview_button_opens_and_closes_the_panel() {
        let mut layout = loaded_layout(true);
        layout.toggle_panel(DetailPanel::Overview, true);
        assert_eq!(layout.visible_panel(true), Some(DetailPanel::Overview));
        layout.toggle_panel(DetailPanel::Overview, true);
        assert_eq!(layout.visible_panel(true), None);
    }

    // parity: TS-renderer-TaskDetailRoute-06c6d5
    #[test]
    fn closing_the_default_panel_survives_a_settings_reload() {
        let mut layout = loaded_layout(false);
        layout.toggle_panel(DetailPanel::Overview, true);
        layout.sidebar_default_collapsed = Some(false);
        assert_eq!(layout.visible_panel(true), None);
    }

    // parity: TS-renderer-TaskDetailRoute-0a584c
    #[test]
    fn usage_toggle_opens_and_closes_the_usage_panel() {
        let mut layout = loaded_layout(true);
        layout.toggle_panel(DetailPanel::Usage, true);
        assert_eq!(layout.visible_panel(true), Some(DetailPanel::Usage));
        layout.toggle_panel(DetailPanel::Usage, true);
        assert_eq!(layout.visible_panel(true), None);
    }

    // parity: TS-renderer-TaskDetailRoute-6f4f4f
    #[test]
    fn chat_entry_switches_the_main_area_instead_of_opening_a_drawer() {
        let mut layout = loaded_layout(false);
        layout.toggle_history();
        assert_eq!(layout.main_view, MainView::History);
        assert_eq!(layout.visible_panel(true), None);
        let entries = layout.dock_entries(true, None, DockBusyState::default());
        assert!(dock_entry_at(&entries, 3).unwrap().is_active);
        layout.toggle_history();
        assert_eq!(layout.main_view, MainView::Terminal);
    }

    // parity: TS-renderer-TaskDetailRoute-74b0c3
    #[test]
    fn dock_slot_three_switches_to_the_chat_history() {
        let entries = loaded_layout(true).dock_entries(true, None, DockBusyState::default());
        assert_eq!(
            dock_entry_at(&entries, 3).unwrap().command(),
            DockCommand::ToggleHistory
        );
    }

    #[test]
    fn busy_flags_mark_only_their_dock_entries() {
        let busy = DockBusyState {
            is_opening_pull_request: true,
            is_opening_vscode: false,
        };
        let entries = loaded_layout(true).dock_entries(true, None, busy);
        let busy_ids: Vec<TaskDetailDockItemId> = entries
            .iter()
            .filter(|entry| entry.is_busy)
            .map(|entry| entry.id)
            .collect();
        assert_eq!(busy_ids, [TaskDetailDockItemId::PullRequest]);
    }
}
