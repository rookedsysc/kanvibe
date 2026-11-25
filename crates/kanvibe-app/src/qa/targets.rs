//! 재생 드라이버가 "보이는 글자·역할"로 화면 요소를 찾을 수 있게, 그려진 요소의 위치를 프레임마다 모은다.
//! states.yaml 대상 표기(`"글자"`, `{text, exact}`, `{role, name, exact}`, `nth`)를 Playwright와 같은 뜻으로 해석한다.

use std::collections::HashMap;

use gpui_kit::{
    App, Bounds, Global, IntoElement, ParentElement, Pixels, SharedString, Styled, Window,
    WindowId, canvas, div,
};
use serde::Deserialize;

/// 요소 하나의 접근성 정보. `name`은 aria-label 또는 보이는 글자다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaTargetDescription {
    pub role: &'static str,
    pub name: SharedString,
    pub text: SharedString,
}

#[derive(Debug, Clone, PartialEq)]
struct PaintedTarget {
    description: QaTargetDescription,
    bounds: Bounds<Pixels>,
}

/// 창마다 마지막으로 그린 프레임의 요소 위치
#[derive(Debug, Default)]
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

    fn record(
        &mut self,
        window_id: WindowId,
        description: QaTargetDescription,
        bounds: Bounds<Pixels>,
    ) {
        self.painted_targets
            .entry(window_id)
            .or_default()
            .push(PaintedTarget {
                description,
                bounds,
            });
    }

    pub fn find(&self, window_id: WindowId, query: &TargetQuery) -> Option<Bounds<Pixels>> {
        let painted_targets = self.painted_targets.get(&window_id)?;
        let nth = match query {
            TargetQuery::ExactText(_) => 0,
            TargetQuery::Described(described) => described.nth.unwrap_or(0),
        };
        painted_targets
            .iter()
            .filter(|painted| matches_query(&painted.description, query))
            .nth(nth)
            .map(|painted| painted.bounds)
    }
}

fn text_matches(actual: &str, expected: &str, is_exact: bool) -> bool {
    if is_exact {
        actual == expected
    } else {
        actual.to_lowercase().contains(&expected.to_lowercase())
    }
}

fn matches_query(description: &QaTargetDescription, query: &TargetQuery) -> bool {
    match query {
        TargetQuery::ExactText(text) => description.text.as_ref() == text,
        TargetQuery::Described(described) => {
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
            role_matches && name_matches && text_matches_query
        }
    }
}

/// 감싼 요소(부모가 `relative()`여야 한다)의 위치를 이번 프레임 대상으로 남기는 투명 층
pub fn qa_target_layer(description: QaTargetDescription) -> impl IntoElement {
    div().absolute().inset_0().child(
        canvas(
            move |bounds, window: &mut Window, cx: &mut App| {
                record_painted_target(description.clone(), bounds, window, cx);
            },
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

fn record_painted_target(
    description: QaTargetDescription,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if !cx.has_global::<QaTargetRegistry>() {
        return;
    }
    let window_id = window.window_handle().window_id();
    cx.global_mut::<QaTargetRegistry>()
        .record(window_id, description, bounds);
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
        assert!(matches_query(
            &button,
            &TargetQuery::ExactText("뒤로가기".into())
        ));
        assert!(!matches_query(
            &button,
            &TargetQuery::ExactText("뒤로".into())
        ));
    }

    #[test]
    fn role_and_name_targets_match_names_loosely_unless_exact() {
        let settings = description("button", "설정", "");
        let query: TargetQuery = serde_json::from_str(r#"{"role":"button","name":"설"}"#).unwrap();
        let exact_query: TargetQuery =
            serde_json::from_str(r#"{"role":"button","name":"설","exact":true}"#).unwrap();

        assert!(matches_query(&settings, &query));
        assert!(!matches_query(&settings, &exact_query));
    }
}
