//! 재생 드라이버가 "보이는 글자·역할"로 화면 요소를 찾을 수 있게, 그려진 요소의 위치를 프레임마다 모은다.
//! states.yaml 대상 표기(`"글자"`, `{text, exact}`, `{role, name, exact}`, `{placeholder}`, `{selector}`, `nth`)를
//! Playwright와 같은 뜻으로 해석한다. 입력란은 포커스와 값 바꾸기(Playwright `focus`·`fill`)를 함께 남긴다.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, FocusHandle, Global, IntoElement, ParentElement, Pixels, SharedString, Styled,
    Window, WindowId, canvas, div,
};
use serde::Deserialize;

/// 요소 하나의 접근성 정보. `name`은 aria-label 또는 보이는 글자다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaTargetDescription {
    pub role: &'static str,
    pub name: SharedString,
    pub text: SharedString,
}

/// 입력란 값을 통째로 바꾼다(Playwright `fill`: 값을 바꾸고 input 이벤트를 한 번 낸다)
pub type QaFillHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 대상이 글자·역할 말고 더 가진 것. 입력란의 placeholder, CSS 선택자 대신 쓰는 표식, 포커스, 값 바꾸기
#[derive(Clone, Default)]
pub struct QaTargetBehavior {
    pub placeholder: Option<SharedString>,
    /// states.yaml `selector` 값과 똑같은 글자일 때만 맞는다(예: `[data-kanban-task-card='true'][data-kanban-status='todo']`)
    pub selector: Option<SharedString>,
    pub focus_handle: Option<FocusHandle>,
    pub fill: Option<QaFillHandler>,
}

#[derive(Clone)]
struct PaintedTarget {
    description: QaTargetDescription,
    behavior: QaTargetBehavior,
    bounds: Bounds<Pixels>,
    /// 대상을 자르는 영역(부모 스크롤 영역들이 겹친 보이는 칸)
    clip_bounds: Bounds<Pixels>,
}

/// 찾은 대상
#[derive(Clone)]
pub struct QaTargetHit {
    pub bounds: Bounds<Pixels>,
    /// 대상을 자르는 영역. 대상이 이 밖에 있으면 스크롤해야 보인다
    pub clip_bounds: Bounds<Pixels>,
    pub focus_handle: Option<FocusHandle>,
    pub fill: Option<QaFillHandler>,
}

/// 창마다 마지막으로 그린 프레임의 요소 위치
#[derive(Default)]
pub struct QaTargetRegistry {
    painted_targets: HashMap<WindowId, Vec<PaintedTarget>>,
}

impl Global for QaTargetRegistry {}

/// states.yaml 대상 표기
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TargetQuery {
    ExactText(String),
    Described(DescribedTarget),
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct DescribedTarget {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub exact: Option<bool>,
    #[serde(default)]
    pub nth: Option<usize>,
}

impl QaTargetRegistry {
    /// 루트 뷰가 그리기 전에 부른다. 지난 프레임 위치가 남으면 사라진 요소를 누르게 된다.
    pub fn begin_frame(&mut self, window_id: WindowId) {
        self.painted_targets.entry(window_id).or_default().clear();
    }

    fn record(&mut self, window_id: WindowId, painted: PaintedTarget) {
        self.painted_targets
            .entry(window_id)
            .or_default()
            .push(painted);
    }

    pub fn find(&self, window_id: WindowId, query: &TargetQuery) -> Option<Bounds<Pixels>> {
        self.find_hit(window_id, query).map(|hit| hit.bounds)
    }

    /// 대상과 그 포커스·값 바꾸기
    pub fn find_hit(&self, window_id: WindowId, query: &TargetQuery) -> Option<QaTargetHit> {
        let painted_targets = self.painted_targets.get(&window_id)?;
        let nth = match query {
            TargetQuery::ExactText(_) => 0,
            TargetQuery::Described(described) => described.nth.unwrap_or(0),
        };
        painted_targets
            .iter()
            .filter(|painted| matches_query(&painted.description, &painted.behavior, query))
            .nth(nth)
            .map(|painted| QaTargetHit {
                bounds: painted.bounds,
                clip_bounds: painted.clip_bounds,
                focus_handle: painted.behavior.focus_handle.clone(),
                fill: painted.behavior.fill.clone(),
            })
    }

    /// 마지막 프레임에 그려진 대상 가운데 역할이 `role`인 것의 글(성능 표식이 터미널 줄 변화를 볼 때 쓴다)
    pub fn painted_texts<'a>(
        &'a self,
        window_id: WindowId,
        role: &'a str,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.painted_targets
            .get(&window_id)
            .into_iter()
            .flatten()
            .filter(move |painted| painted.description.role == role)
            .map(|painted| painted.description.text.as_ref())
    }
}

fn text_matches(actual: &str, expected: &str, is_exact: bool) -> bool {
    if is_exact {
        actual == expected
    } else {
        actual.to_lowercase().contains(&expected.to_lowercase())
    }
}

fn matches_query(
    description: &QaTargetDescription,
    behavior: &QaTargetBehavior,
    query: &TargetQuery,
) -> bool {
    match query {
        TargetQuery::ExactText(text) => description.text.as_ref() == text,
        TargetQuery::Described(described) => {
            let placeholder_matches = described.placeholder.as_deref().is_none_or(|expected| {
                behavior.placeholder.as_deref().is_some_and(|actual| {
                    text_matches(actual, expected, described.exact.unwrap_or(false))
                })
            });
            let selector_matches = described
                .selector
                .as_deref()
                .is_none_or(|expected| behavior.selector.as_deref() == Some(expected));
            let role_matches = described
                .role
                .as_deref()
                .is_none_or(|role| role == description.role);
            let name_matches = described.name.as_deref().is_none_or(|name| {
                text_matches(&description.name, name, described.exact.unwrap_or(false))
            });
            let text_matches_query = described.text.as_deref().is_none_or(|text| {
                text_matches(&description.text, text, described.exact.unwrap_or(true))
            });
            role_matches
                && name_matches
                && text_matches_query
                && placeholder_matches
                && selector_matches
        }
    }
}

/// 감싼 요소(부모가 `relative()`여야 한다)의 위치를 이번 프레임 대상으로 남기는 투명 층
pub fn qa_target_layer(description: QaTargetDescription) -> impl IntoElement {
    qa_target_layer_with(description, QaTargetBehavior::default())
}

/// `qa_target_layer`에 placeholder·선택자·포커스·값 바꾸기를 더한 층
pub fn qa_target_layer_with(
    description: QaTargetDescription,
    behavior: QaTargetBehavior,
) -> impl IntoElement {
    div().absolute().inset_0().child(
        canvas(
            move |bounds, window: &mut Window, cx: &mut App| {
                record_painted_target(description.clone(), behavior.clone(), bounds, window, cx);
            },
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

/// 버튼(`role=button`). `label`은 접근성 이름, `text`는 보이는 글자다
pub fn qa_button_layer(label: &str, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role: "button",
        name: label.to_string().into(),
        text: text.to_string().into(),
    })
}

/// 제목·안내처럼 버튼이 아닌 글자
pub fn qa_text_layer(role: &'static str, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role,
        name: text.to_string().into(),
        text: text.to_string().into(),
    })
}

/// 한 줄 입력란(`role=textbox`, 접근성 이름은 placeholder). 포커스를 주고 값을 바꿀 수 있다
pub fn qa_text_input_layer(
    placeholder: impl Into<SharedString>,
    focus_handle: FocusHandle,
    fill: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let placeholder = placeholder.into();
    qa_target_layer_with(
        QaTargetDescription {
            role: "textbox",
            name: placeholder.clone(),
            text: SharedString::default(),
        },
        QaTargetBehavior {
            placeholder: Some(placeholder),
            selector: None,
            focus_handle: Some(focus_handle),
            fill: Some(Rc::new(fill)),
        },
    )
}

fn record_painted_target(
    description: QaTargetDescription,
    behavior: QaTargetBehavior,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if !cx.has_global::<QaTargetRegistry>() {
        return;
    }
    let window_id = window.window_handle().window_id();
    let clip_bounds = window.content_mask().bounds;
    cx.global_mut::<QaTargetRegistry>().record(
        window_id,
        PaintedTarget {
            description,
            behavior,
            bounds,
            clip_bounds,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn description(role: &'static str, name: &str, text: &str) -> QaTargetDescription {
        QaTargetDescription {
            role,
            name: name.to_string().into(),
            text: text.to_string().into(),
        }
    }

    #[test]
    fn plain_string_targets_match_exact_visible_text() {
        let button = description("button", "뒤로가기", "뒤로가기");
        let behavior = QaTargetBehavior::default();
        assert!(matches_query(
            &button,
            &behavior,
            &TargetQuery::ExactText("뒤로가기".into())
        ));
        assert!(!matches_query(
            &button,
            &behavior,
            &TargetQuery::ExactText("뒤로".into())
        ));
    }

    #[test]
    fn role_and_name_targets_match_names_loosely_unless_exact() {
        let settings = description("button", "설정", "");
        let query: TargetQuery = serde_json::from_str(r#"{"role":"button","name":"설"}"#).unwrap();
        let exact_query: TargetQuery =
            serde_json::from_str(r#"{"role":"button","name":"설","exact":true}"#).unwrap();

        let behavior = QaTargetBehavior::default();
        assert!(matches_query(&settings, &behavior, &query));
        assert!(!matches_query(&settings, &behavior, &exact_query));
    }

    #[test]
    fn placeholder_and_selector_targets_match_like_playwright() {
        let field = description("textbox", "계정 이름 (예: work)", "");
        let input = QaTargetBehavior {
            placeholder: Some("계정 이름 (예: work)".into()),
            ..QaTargetBehavior::default()
        };
        let by_placeholder: TargetQuery =
            serde_json::from_str(r#"{"placeholder":"계정 이름","nth":1,"value":"work"}"#).unwrap();
        assert!(matches_query(&field, &input, &by_placeholder));
        assert!(!matches_query(
            &field,
            &QaTargetBehavior::default(),
            &by_placeholder
        ));

        let card = description("link", "카드", "카드");
        let todo_card = QaTargetBehavior {
            selector: Some("[data-kanban-task-card='true'][data-kanban-status='todo']".into()),
            ..QaTargetBehavior::default()
        };
        let by_selector: TargetQuery = serde_json::from_str(
            r#"{"selector":"[data-kanban-task-card='true'][data-kanban-status='todo']","description":"Todo 열의 첫 카드"}"#,
        )
        .unwrap();
        assert!(matches_query(&card, &todo_card, &by_selector));
        assert!(!matches_query(
            &card,
            &QaTargetBehavior::default(),
            &by_selector
        ));
    }
}
