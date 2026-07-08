use std::error::Error;

use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};
use gpui_component::{Root, button::Button};

use crate::{
    NativeUiColumnSpec, NativeUiLaunchConfig, NativeUiRenderSpec, build_native_ui_render_spec,
    load_read_only_board,
};

pub fn run_native_ui() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = NativeUiLaunchConfig::from_env()?;
    let bootstrap = load_read_only_board(&config.repo_root, &config.database_path, config.locale)?;
    let spec = build_native_ui_render_spec(&bootstrap);
    let _qa_socket = crate::qa_control::spawn_debug_qa_socket_from_env(spec.clone())?;
    let app = Application::new();

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(860.0)), cx);
        let spec = spec.clone();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| KanVibeRoot { spec: spec.clone() });
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("failed to open KanVibe native window");
        cx.activate(true);
    });

    Ok(())
}

struct KanVibeRoot {
    spec: NativeUiRenderSpec,
}

impl Render for KanVibeRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let columns = self
            .spec
            .columns
            .iter()
            .fold(div().flex().gap_3().w_full(), |row, column| {
                row.child(render_board_column(column))
            });

        div()
            .size_full()
            .bg(rgb(0x0f1117))
            .text_color(rgb(0xf8fafc))
            .p_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(rgb(0xf8fafc))
                                    .child(self.spec.window_title.clone()),
                            )
                            .child(format!(
                                "{} projects · {} visible tasks · {} done",
                                self.spec.project_count,
                                self.spec.total_visible_tasks,
                                self.spec.done_total
                            )),
                    )
                    .child(
                        Button::new("new-task")
                            .primary()
                            .label(self.spec.primary_action_label.clone()),
                    ),
            )
            .child(
                div()
                    .mb_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x202632))
                    .child(format!(
                        "{} · route {} · locale {}",
                        self.spec.all_projects_label,
                        self.spec.route,
                        self.spec.locale.code()
                    )),
            )
            .child(columns)
    }
}

fn render_board_column(column: &NativeUiColumnSpec) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(180.0))
        .rounded_md()
        .border_1()
        .border_color(rgb(0x293241))
        .bg(rgb(0x171b23))
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .mb_3()
                .child(
                    div()
                        .text_color(rgb(color_to_u32(column.color)))
                        .child(column.label.clone()),
                )
                .child(format!("{}", column.task_count)),
        )
        .child(
            div()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .bg(rgb(0x111827))
                .p_3()
                .child(
                    column
                        .first_card_title
                        .clone()
                        .unwrap_or_else(|| "No tasks".to_owned()),
                ),
        )
}

fn color_to_u32(color: kanvibe_theme::Rgb) -> u32 {
    (u32::from(color.red) << 16) | (u32::from(color.green) << 8) | u32::from(color.blue)
}
