//! 작업 상세 화면이 pane 채팅 층을 붙이는 자리(TS `Terminal.tsx`가 `TerminalPaneChatLayer`에 넘기던 값).
//! 층은 터미널 뷰와 같은 작업·탭으로 만들고, 터미널이 연결되어 보일 때만 보인다고 알린다.

use gpui_kit::{App, AppContext, Context, Entity, IntoElement, ParentElement, Styled, Window, div};

use super::super::TaskDetailView;
use super::super::layout_state::{DockCommand, MainView};
use super::backend::RuntimePaneChatBackend;
use super::client::PaneChatClient;
use super::layer::{PaneChatLayer, PaneChatLayerEvent, PaneChatLayerProps, TerminalChatGrid};
use crate::components::terminal_view::TerminalTarget;
use crate::components::terminal_view::paint::measure_cell_size;
use crate::qa::targets::{QaTargetBehavior, QaTargetDescription, qa_target_layer_with};

/// 재생 상태 정의가 터미널 화면을 가리키는 TS DOM 선택자(`.xterm-screen`). 터미널 영역 전체를 그 선택자의 대상으로 남긴다
pub fn terminal_screen_target() -> impl IntoElement {
    div().absolute().inset_0().child(qa_target_layer_with(
        QaTargetDescription {
            role: "generic",
            name: String::new().into(),
            text: String::new().into(),
        },
        QaTargetBehavior {
            selector: Some(".xterm-screen".into()),
            ..QaTargetBehavior::default()
        },
    ))
}

impl TaskDetailView {
    pub(in super::super) fn new_pane_chat_layer(
        &self,
        target: &TerminalTarget,
        cx: &mut Context<Self>,
    ) -> Entity<PaneChatLayer> {
        let props = PaneChatLayerProps {
            client: PaneChatClient::new(RuntimePaneChatBackend::from_app(cx)),
            task_id: target.task_id.clone(),
            terminal_tab_id: target.tab_id.clone(),
            task_title: self.snapshot().map(|snapshot| snapshot.task.title.clone()),
            translator: self.translator,
        };
        cx.new(|cx| PaneChatLayer::new(props, cx))
    }

    /// 터미널 칸 영역. 터미널이 연결되어 한 번 그려진 뒤에만 있다. 폭은 터미널 뷰가 칸을 그린 폭을 그대로 쓴다
    fn terminal_chat_grid(&self, window: &Window, cx: &App) -> Option<TerminalChatGrid> {
        let terminal = self.terminal.as_ref()?.read(cx);
        let width = terminal.grid_width()?;
        let (columns, rows) = terminal.read_screen(|screen| (screen.columns(), screen.rows()))?;
        let cell = measure_cell_size(window);
        Some(TerminalChatGrid {
            columns,
            rows,
            width,
            height: cell.height * rows as f32,
        })
    }

    /// 그릴 때마다 층에 터미널이 보이는지와 칸 영역을 알린다. 기록 화면으로 바꾸면 층은 숨는다
    pub(in super::super) fn sync_pane_chat_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layer) = self.pane_chat.clone() else {
            return;
        };
        if self._pane_chat_events.is_none() {
            self._pane_chat_events =
                Some(
                    cx.subscribe_in(&layer, window, |view, _, event, window, cx| match event {
                        PaneChatLayerEvent::ReturnToTerminal => view.focus_terminal(window, cx),
                        PaneChatLayerEvent::OpenHistory => {
                            view.run_dock_command(DockCommand::ToggleHistory, window, cx)
                        }
                    }),
                );
        }
        // 숨긴 탭의 층은 읽기·주기 조회를 멈춘다. 다시 고르면 그 탭의 채팅 상태로 다시 읽는다
        for (_, hidden) in self.hidden_pane_chats.clone() {
            hidden.update(cx, |layer, cx| {
                layer.set_visibility(false, None, window, cx)
            });
        }
        let grid = self.terminal_chat_grid(window, cx);
        let is_visible = self.layout.main_view == MainView::Terminal && grid.is_some();
        layer.update(cx, |layer, cx| {
            layer.set_visibility(is_visible, grid, window, cx)
        });
    }

    pub(in super::super) fn is_pane_chat_focused(&self, cx: &App) -> bool {
        self.pane_chat
            .as_ref()
            .is_some_and(|layer| layer.read(cx).is_chat_focused())
    }

    pub(in super::super) fn focus_pane_chat(&self, window: &mut Window, cx: &mut App) {
        if let Some(layer) = self.pane_chat.as_ref() {
            layer.update(cx, |layer, cx| layer.focus_chat(window, cx));
        }
    }

    /// `Mod+Shift+J`(공용 단축키 `terminalToggleChat`)
    pub(in super::super) fn toggle_pane_chat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(layer) = self.pane_chat.clone() {
            layer.update(cx, |layer, cx| layer.toggle(None, window, cx));
        }
    }
}
