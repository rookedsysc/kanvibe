//! 프로젝트 색 편집(TS `ProjectColorEditor`). 색 원을 누르면 오른쪽 아래로 색 고르기 팝오버가 열린다.
//! 판·띠·hex 입력으로 고른 색은 원에 미리 보이고, OK나 미리 정한 색을 누르면 저장하며 팝오버를 닫는다.
//! 저장이 실패하면 미리 바꾼 색을 저장된 색으로 되돌리고 사유를 알린다. 저장하는 동안에는 팝오버를 다시 열지 못한다.

use std::rc::Rc;

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::{
    Anchor, AnyElement, AppContext, BoxShadow, Context, Div, Entity, InteractiveElement,
    IntoElement, ParentElement, Pixels, SharedString, Size, Stateful, StatefulInteractiveElement,
    Styled, Subscription, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::project::Project;

use super::TrackedSave;
use super::color_picker::{
    ColorChangeHandler, Hsv, PickerBounds, hsv_to_rgb, parse_hex, render_color_picker, rgb_to_hsv,
    to_hex,
};
use crate::components::{TailwindText, box_shadows};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_service, failure};
use crate::views::settings::text_field::themed_text_field;
use crate::views::task_detail::TaskDetailView;
use crate::views::task_detail::chrome::css_hex;
use crate::views::task_detail::status_move::{KANBAN_SERVICE_UNAVAILABLE, kanban_service};
use crate::views::task_detail::usage_cards::visible_text;

/// TS `PRESET_COLORS`
pub const PRESET_COLORS: [&str; 8] = [
    "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD", "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];
/// `w-[220px]`, 안쪽 여백 `p-3`
const POPOVER_WIDTH: f32 = 220.0;
const POPOVER_PADDING: f32 = 12.0;
/// `top-8`
const POPOVER_TOP: f32 = 32.0;

#[derive(Default)]
pub struct ProjectColorEdit {
    pub is_open: bool,
    /// 미리 바꿔 보인 색. 없으면 저장된 색을 보인다
    pub preview: Option<String>,
    pub save: TrackedSave,
    hex_input: Option<Entity<InputState>>,
    bounds: PickerBounds,
    _hex_events: Option<Subscription>,
}

/// 저장된 색. 없으면 이름으로 정한 색(TS `computeProjectColor`)
pub fn saved_project_color(project: &Project) -> String {
    project.color.clone().unwrap_or_else(|| {
        kanvibe_process::project_color::compute_project_color(&project.name).to_string()
    })
}

impl ProjectColorEdit {
    fn displayed_color(&self, project: &Project) -> String {
        self.preview
            .clone()
            .unwrap_or_else(|| saved_project_color(project))
    }

    fn picker_hsv(&self, project: &Project) -> Hsv {
        let rgb = parse_hex(&self.displayed_color(project)).unwrap_or([0, 0, 0]);
        rgb_to_hsv(rgb)
    }
}

impl TaskDetailView {
    pub(in crate::views::task_detail) fn render_project_color_editor(
        &self,
        theme: &ThemeTokens,
        project: &Project,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let edit = &self.editors.project_color;
        let color = edit.displayed_color(project);
        let mut button = color_button(theme, &color);
        button = match edit.save.is_pending {
            true => button.opacity(0.5),
            false => button
                .cursor_pointer()
                .on_click(cx.listener(|view, _, window, cx| view.open_project_color(window, cx))),
        };
        let error = edit.save.error.as_ref().map(|reason| {
            visible_text(
                self.common
                    .translate_with("saveFailed", &[("reason", reason)]),
                text_scale::PX_11,
            )
            .w(px(POPOVER_WIDTH))
            .text_color(theme.status.error)
        });
        let overlay = edit
            .is_open
            .then(|| self.render_color_popover(theme, project, viewport, cx));
        div()
            .relative()
            .child(button)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .children(error.map(|error| floating_below(error.into_any_element(), 0)))
                    .children(overlay),
            )
            .into_any_element()
    }

    /// 팝오버를 열 때 저장된 색으로 맞춘다
    fn open_project_color(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self
            .snapshot()
            .and_then(|snapshot| snapshot.project.clone())
        else {
            return;
        };
        let saved = saved_project_color(&project);
        let digits = saved.trim_start_matches('#').to_lowercase();
        let input =
            cx.new(|cx| InputState::new(window, cx).default_value(SharedString::from(digits)));
        let events = cx.subscribe_in(&input, window, |view, input, event, _, cx| {
            if let InputEvent::Change = event {
                let text = input.read(cx).value().to_string();
                if let Some(rgb) = parse_hex(&text) {
                    view.editors.project_color.preview = Some(to_hex(rgb));
                    cx.notify();
                }
            }
        });
        let edit = &mut self.editors.project_color;
        edit.preview = Some(saved);
        edit.is_open = true;
        edit.hex_input = Some(input);
        edit._hex_events = Some(events);
        cx.notify();
    }

    fn close_project_color(&mut self, cx: &mut Context<Self>) {
        let edit = &mut self.editors.project_color;
        edit.is_open = false;
        edit.hex_input = None;
        edit._hex_events = None;
        cx.notify();
    }

    /// 판이나 띠로 고른 색을 미리 보이고 hex 입력도 같은 값으로 바꾼다
    fn pick_project_color(&mut self, hsv: Hsv, window: &mut Window, cx: &mut Context<Self>) {
        let hex = to_hex(hsv_to_rgb(hsv));
        if let Some(input) = self.editors.project_color.hex_input.clone() {
            let digits = SharedString::from(hex.trim_start_matches('#').to_string());
            input.update(cx, |input, cx| input.set_value(digits, window, cx));
        }
        self.editors.project_color.preview = Some(hex);
        cx.notify();
    }

    fn apply_project_color(&mut self, hex: String, cx: &mut Context<Self>) {
        self.close_project_color(cx);
        let Some(project) = self
            .snapshot()
            .and_then(|snapshot| snapshot.project.clone())
        else {
            return;
        };
        if !self.editors.project_color.save.begin() {
            return;
        }
        self.editors.project_color.preview = Some(hex.clone());
        let service = kanban_service(cx);
        let project_id = project.id.clone();
        let arguments = serde_json::json!([project_id, hex]).to_string();
        let saved_hex = hex.clone();
        let request = call_service(cx, "kanban.updateProjectColor", arguments, move || {
            let service = service.ok_or_else(|| failure(KANBAN_SERVICE_UNAVAILABLE))?;
            service
                .update_project_color(&project_id, &saved_hex)
                .map_err(failure)
        });
        self.editors._color_save = Some(cx.spawn(async move |this, cx| {
            let result: crate::services::ServiceResult<()> = request.await;
            let _ = this.update(cx, |view, cx| {
                view.finish_project_color_save(hex, result, cx)
            });
        }));
        cx.notify();
    }

    fn finish_project_color_save(
        &mut self,
        hex: String,
        result: crate::services::ServiceResult<()>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                self.editors.project_color.save.finish(None);
                if let super::DetailLoad::Loaded(snapshot) = &mut self.load
                    && let Some(project) = std::rc::Rc::make_mut(snapshot).project.as_mut()
                {
                    project.color = Some(hex);
                }
                crate::app_events::trigger_refresh(crate::app_events::RefreshScope::TaskDetail, cx);
            }
            Err(failure) => {
                // 저장되지 않은 색이 남아 보이지 않게 저장된 색으로 되돌린다
                self.editors.project_color.preview = None;
                self.editors
                    .project_color
                    .save
                    .finish(Some(failure.display_message()));
            }
        }
        cx.notify();
    }

    fn render_color_popover(
        &self,
        theme: &ThemeTokens,
        project: &Project,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let edit = &self.editors.project_color;
        let entity = cx.entity().downgrade();
        let on_pick: ColorChangeHandler = Rc::new(move |hsv, window, cx| {
            let _ = entity.update(cx, |view, cx| view.pick_project_color(hsv, window, cx));
        });
        let picker_width = POPOVER_WIDTH - POPOVER_PADDING * 2.0 - 2.0;
        let popover = div()
            .id("project-color-popover")
            .occlude()
            .w(px(POPOVER_WIDTH))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .p(px(POPOVER_PADDING))
            .child(render_color_picker(
                edit.picker_hsv(project),
                picker_width,
                &edit.bounds,
                on_pick,
            ))
            .child(self.render_hex_row(theme, project, cx))
            .child(self.render_presets(theme, project, cx));
        let backdrop = deferred(
            anchored().position(point(px(0.0), px(0.0))).child(
                div()
                    .id("project-color-backdrop")
                    .occlude()
                    .w(viewport.width)
                    .h(viewport.height)
                    .on_click(cx.listener(|view, _, _, cx| view.close_project_color(cx))),
            ),
        )
        .with_priority(1);
        div()
            .child(backdrop)
            .child(floating_below(popover.into_any_element(), 2))
            .into_any_element()
    }

    /// `#` 글자, hex 입력, OK 버튼
    fn render_hex_row(
        &self,
        theme: &ThemeTokens,
        project: &Project,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let edit = &self.editors.project_color;
        let input = edit.hex_input.clone().map(|input| {
            div()
                .min_w_0()
                .flex_1()
                .rounded(radius::BASE)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.page)
                .px(spacing(1.5))
                .py(spacing(1.0))
                .text_scale(text_scale::XS)
                .mono(400)
                .child(themed_text_field(&input, theme, theme.text.primary, cx))
        });
        let color = edit.displayed_color(project);
        div()
            .mt(spacing(2.0))
            .flex()
            .items_center()
            .gap(spacing(1.5))
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(aligned_text("#", text_scale::XS, TextFamily::Sans)),
            )
            .children(input)
            .child(ok_button(theme).on_click(
                cx.listener(move |view, _, _, cx| view.apply_project_color(color.clone(), cx)),
            ))
            .into_any_element()
    }

    /// 미리 정한 여덟 색. 지금 색과 같으면 `ring-2 ring-offset-1 ring-text-primary`
    fn render_presets(
        &self,
        theme: &ThemeTokens,
        project: &Project,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self
            .editors
            .project_color
            .displayed_color(project)
            .to_lowercase();
        let ring = theme.text.primary;
        let surface = theme.background.surface;
        let swatches = PRESET_COLORS.map(|preset| {
            let is_selected = current == preset.to_lowercase();
            let label = format!("Color {preset}");
            let swatch = div()
                .id(SharedString::from(label.clone()))
                .relative()
                .size(spacing(5.0))
                .rounded_full()
                .bg(css_hex(preset))
                .cursor_pointer()
                .child(qa_target_layer(QaTargetDescription {
                    role: "button",
                    name: label.into(),
                    text: SharedString::default(),
                }))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.apply_project_color(preset.to_string(), cx)
                }));
            match is_selected {
                true => swatch.shadow(vec![
                    ring_layer(surface.into(), 1.0),
                    ring_layer(ring.into(), 3.0),
                ]),
                false => swatch,
            }
        });
        div()
            .mt(spacing(2.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(1.5))
            .children(swatches)
            .into_any_element()
    }
}

/// `w-6 h-6 rounded-full border-2` 색 원. 보이는 글자가 없어 재생 대상 글자에 지금 칠한 색을 싣는다
/// (TS 테스트의 `style.backgroundColor`)
fn color_button(theme: &ThemeTokens, color: &str) -> Stateful<Div> {
    div()
        .id("project-color-button")
        .relative()
        .size(spacing(6.0))
        .rounded_full()
        .border_2()
        .border_color(theme.border.default)
        .bg(css_hex(color))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: "Pick color".into(),
            text: color.to_lowercase().into(),
        }))
}

/// `text-xs px-2 py-1 rounded bg-brand-primary text-text-inverse`
fn ok_button(theme: &ThemeTokens) -> Stateful<Div> {
    div()
        .id("project-color-ok")
        .relative()
        .flex_none()
        .rounded(radius::BASE)
        .bg(theme.brand.primary)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.inverse)
        .cursor_pointer()
        .child(aligned_text("OK", text_scale::XS, TextFamily::Sans))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: "OK".into(),
            text: "OK".into(),
        }))
}

fn ring_layer(color: gpui_kit::Hsla, spread: f32) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(spread),
        inset: false,
    }
}

/// 색 원 오른쪽 끝에 맞춰 `top-8` 아래에 띄운다(`absolute right-0 top-8`)
fn floating_below(content: AnyElement, priority: usize) -> AnyElement {
    deferred(
        anchored()
            .anchor(Anchor::TopRight)
            .offset(point(px(0.0), px(POPOVER_TOP)))
            .child(content),
    )
    .with_priority(priority)
    .into_any_element()
}
