//! `/:locale` 보드 화면. AppSettings `board_view_mode`에 따라 칸반(`kanban`)이나 워크스페이스(TS `TaskWorkspaceRoute`)를
//! 그린다. 워크스페이스 뷰는 M7에서 이 디렉터리에 더한다. 지금은 칸반 헤더·스켈레톤만 있다.

pub mod kanban;

use gpui_kit::{AnyElement, Context, IntoElement, Render, Task, Window};

use crate::notifications::NotificationCenter;
use crate::services::{AppServices, BoardHeaderSnapshot, ServiceResult};
use crate::theme::ThemeTokens;
use crate::views::ScreenContext;
use kanban::{BoardHeaderModel, TaskKindFilter, render_board_skeleton, render_loaded_board};

/// 보드 데이터 조회 상태(`useRouteLoadStatus`의 로딩·성공·실패)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardLoadState {
    Loading,
    Loaded(BoardHeaderSnapshot),
    Failed(String),
}

pub struct BoardView {
    screen: ScreenContext,
    load_state: BoardLoadState,
    _board_load: Task<()>,
}

impl BoardView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let load = cx.global::<AppServices>().load_board_header(cx);
        let board_load = cx.spawn(async move |this, cx| {
            let result: ServiceResult<BoardHeaderSnapshot> = load.await;
            let _ = this.update(cx, |this, cx| {
                this.load_state = match result {
                    Ok(snapshot) => BoardLoadState::Loaded(snapshot),
                    Err(failure) => BoardLoadState::Failed(failure.reason),
                };
                cx.notify();
            });
        });
        Self {
            screen,
            load_state: BoardLoadState::Loading,
            _board_load: board_load,
        }
    }
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let viewport = window.viewport_size();
        let board: AnyElement = match &self.load_state {
            BoardLoadState::Loaded(snapshot) => {
                let model = BoardHeaderModel {
                    locale: self.screen.route.locale,
                    task_kind_filter: TaskKindFilter::All,
                    selected_project_names: Vec::new(),
                    sort_active_count: snapshot.sort_active_count,
                    unread_notification_count: cx
                        .try_global::<NotificationCenter>()
                        .map(NotificationCenter::unread_count)
                        .unwrap_or(0),
                };
                render_loaded_board(&theme, &model, snapshot.status_counts, viewport)
                    .into_any_element()
            }
            BoardLoadState::Loading | BoardLoadState::Failed(_) => {
                render_board_skeleton(&theme, f32::from(viewport.height)).into_any_element()
            }
        };
        board
    }
}
