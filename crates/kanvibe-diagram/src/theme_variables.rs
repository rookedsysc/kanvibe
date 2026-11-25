//! 정의의 `themeVariables`(앞머리 `config.themeVariables`와 `init` 지시문).
//!
//! TS 모바일 워커(diagramWorker/main.ts)는 `themeVariables`를 `secure`에 넣어 정의에서 바꿀 수 없고, 데스크톱 화면
//! (AiSessionMessageContent)은 Mermaid 기본 `secure`를 써서 정의의 값이 먹는다. 부르는 쪽이 [`ThemeVariablesPolicy`]로 고른다.
//!
//! Mermaid 11.15 `updateCurrentConfig`는 지시문에 `theme`이 없으면 테마 변수를 다시 계산하지 않는다. initialize가 계산해 둔
//! 값 위에 정의의 값을 깊게 겹칠 뿐이라, 덮은 변수만 바뀌고 그 변수에서 파생된 변수(예: `primaryColor` → `mainBkg`)는 그대로다.
//! 겹치기 전 Mermaid `sanitize`처럼 `<`·`>`·`url(data:`가 든 글자와 `__`로 시작하는 열쇠는 버린다.

use serde_json::{Map, Value};

use crate::directive::DiagramConfig;
use crate::scene::Color;
use crate::sequence::css_color::parse_css_color;

/// 정의의 `themeVariables`를 쓸지. Mermaid `secure` 목록에 `themeVariables`가 들었는지와 같다
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeVariablesPolicy {
    /// 모바일 워커: `secure`가 막는다
    #[default]
    Ignore,
    /// 데스크톱 화면: 정의의 값을 테마 기본값 위에 겹친다
    Apply,
}

/// 정의가 덮은 테마 변수. 덮지 않은 변수는 `None`이고 각 다이어그램이 테마 기본값을 쓴다
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeVariables {
    overrides: Map<String, Value>,
}

impl ThemeVariables {
    pub fn read(config: &DiagramConfig, policy: ThemeVariablesPolicy) -> Self {
        let overrides = match (policy, config.tree.get("themeVariables")) {
            (ThemeVariablesPolicy::Apply, Some(Value::Object(map))) => sanitize(map),
            _ => Map::new(),
        };
        Self { overrides }
    }

    /// 안쪽 묶음(`xyChart`, `radar` 등)
    pub fn section(&self, key: &str) -> Self {
        let overrides = self
            .overrides
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Self { overrides }
    }

    /// CSS에 그대로 들어가는 글자 값. 숫자도 글자로 읽는다
    pub fn text(&self, key: &str) -> Option<String> {
        match self.overrides.get(key)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        }
    }

    /// CSS 색. 브라우저가 읽지 못하는 값은 덮지 않은 것으로 본다
    pub fn color(&self, key: &str) -> Option<Color> {
        parse_theme_color(&self.text(key)?)
    }

    /// 숫자나 숫자 글자(`"0.7"`)
    pub fn number(&self, key: &str) -> Option<f64> {
        self.text(key)?.trim().parse().ok()
    }

    /// 덮은 색이 있으면 `target`을 바꾼다
    pub fn read_color(&self, key: &str, target: &mut Color) {
        if let Some(color) = self.color(key) {
            *target = color;
        }
    }

    pub fn read_number(&self, key: &str, target: &mut f64) {
        if let Some(number) = self.number(key) {
            *target = number;
        }
    }
}

/// `#rgb`·`#rrggbb`·`rgb()`·`hsl()`·CSS 이름 색
pub fn parse_theme_color(text: &str) -> Option<Color> {
    let trimmed = text.trim();
    if trimmed.starts_with('#') {
        return Color::parse_css(trimmed);
    }
    parse_css_color(trimmed)
}

/// Mermaid `sanitize`: 위험한 글자와 `__` 열쇠를 버리고 안쪽 묶음도 같은 규칙으로 거른다
fn sanitize(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .filter(|(key, _)| !key.starts_with("__"))
        .filter_map(|(key, value)| {
            let kept = match value {
                Value::String(text) if is_unsafe_text(text) => return None,
                Value::Object(inner) => Value::Object(sanitize(inner)),
                other => other.clone(),
            };
            Some((key.clone(), kept))
        })
        .collect()
}

fn is_unsafe_text(text: &str) -> bool {
    text.contains('<') || text.contains('>') || text.contains("url(data:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    const DEFINITION: &str = "---\nconfig:\n  themeVariables:\n    mainBkg: '#ff0000'\n    radar:\n      curveOpacity: 0.25\n---\n%%{init: {\"themeVariables\": {\"lineColor\": \"hsl(120, 100%, 25%)\", \"textColor\": \"<b>\", \"__proto\": \"red\"}}}%%\nflowchart LR\n  A --> B";

    #[test]
    fn 데스크톱은_앞머리와_지시문의_값을_겹쳐_읽는다() {
        let variables = ThemeVariables::read(&read_config(DEFINITION), ThemeVariablesPolicy::Apply);
        assert_eq!(variables.color("mainBkg"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(variables.color("lineColor"), Some(Color::rgb(0, 128, 0)));
        assert_eq!(
            variables.section("radar").number("curveOpacity"),
            Some(0.25)
        );
        assert_eq!(variables.text("textColor"), None);
        assert_eq!(variables.text("__proto"), None);
    }

    #[test]
    fn 모바일은_정의의_값을_쓰지_않는다() {
        let variables =
            ThemeVariables::read(&read_config(DEFINITION), ThemeVariablesPolicy::Ignore);
        assert_eq!(variables, ThemeVariables::default());
    }

    fn node_fills(policy: ThemeVariablesPolicy) -> Vec<Color> {
        let fonts = crate::text::tests::bundled_fonts();
        let scene = crate::render_scene(
            DEFINITION,
            crate::theme::Theme::Light,
            crate::LabelMode::SvgText,
            policy,
            &fonts,
        )
        .unwrap();
        let mut fills = Vec::new();
        collect_fills(&scene.items, &mut fills);
        fills
    }

    fn collect_fills(items: &[crate::scene::SceneItem], fills: &mut Vec<Color>) {
        for item in items {
            match item {
                crate::scene::SceneItem::Path(path) => fills.extend(path.fill),
                crate::scene::SceneItem::Clip(clip) => collect_fills(&clip.items, fills),
                _ => {}
            }
        }
    }

    #[test]
    fn 부르는_쪽_설정에_따라_노드_채움색이_갈린다() {
        let red = Color::rgb(255, 0, 0);
        assert!(node_fills(ThemeVariablesPolicy::Apply).contains(&red));
        assert!(!node_fills(ThemeVariablesPolicy::Ignore).contains(&red));
    }
}
