//! 정의의 `themeVariables`·`theme`(앞머리 `config`와 `init` 지시문)를 Mermaid 11.15 `updateCurrentConfig`처럼 푼다.
//!
//! TS 모바일 워커(diagramWorker/main.ts)는 `theme`·`themeVariables`·`themeCSS`를 `secure`에 넣어 정의에서 바꿀 수 없고, 데스크톱
//! 화면(AiSessionMessageContent)은 Mermaid 기본 `secure`를 써서 정의의 값이 먹는다. 부르는 쪽이 [`ThemeVariablesPolicy`]로 고른다.
//!
//! 결과는 Mermaid가 다이어그램에 넘기는 `config.themeVariables` 전체다. `initialize`는 앱 테마(라이트 `default`, 다크 `dark`)의
//! 변수를 계산해 두고, 정의는 두 갈래로 그 위에 얹힌다.
//! - `theme`이 없거나 Mermaid가 모르는 이름이면 다시 계산하지 않는다. 계산해 둔 값 위에 정의의 값을 겹칠 뿐이라, 덮은 변수만
//!   바뀌고 그 변수에서 파생된 변수(예: `primaryColor` → `mainBkg`)는 그대로다. 안쪽 묶음(`radar` 등)은 열쇠마다 겹친다.
//! - `theme`이 `base`·`forest`·`neutral`·`default`·`dark`이면 그 테마를 정의의 값으로 다시 계산해 파생 변수도 따라 바뀐다.
//!   `neo`·`redux` 계열은 다이어그램 styles가 테마 이름으로 그림을 바꾸는데 그 갈래를 옮기지 않아, 앱 테마 위에 겹치는 쪽으로 둔다.
//!
//! 겹치기 전 Mermaid `sanitize`처럼 `<`·`>`·`url(data:`가 든 글자와 `__`로 시작하는 열쇠는 버린다.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::DiagramError;
use crate::directive::DiagramConfig;
use crate::mermaid_theme::{CONFIG_KEYS, MERMAID_THEME_NAMES, MermaidTheme, theme_variables};
use crate::scene::Color;
use crate::sequence::css_color::parse_css_color;
use crate::theme::Theme;
use crate::theme_css::ThemeCss;

/// 정의의 `theme`·`themeVariables`를 쓸지. Mermaid `secure` 목록에 두 열쇠가 들었는지와 같다
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeVariablesPolicy {
    /// 모바일 워커: `secure`가 막는다
    #[default]
    Ignore,
    /// 데스크톱 화면: 정의의 값을 쓴다
    Apply,
}

/// Mermaid가 다이어그램에 넘기는 테마 변수. 비어 있으면(테스트용 기본값) 각 다이어그램이 앱 테마 기본값을 쓴다
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeVariables {
    values: Map<String, Value>,
    /// 정의의 `themeCSS`. 모바일은 `secure`가 막아 비어 있다
    theme_css: ThemeCss,
}

impl ThemeVariables {
    /// 앱 테마와 정의 설정에서 테마 변수를 푼다. 다시 계산하다 khroma가 읽지 못하는 색을 만나면 Mermaid처럼 그리기에 실패한다
    pub fn read(
        config: &DiagramConfig,
        policy: ThemeVariablesPolicy,
        app_theme: Theme,
    ) -> Result<Self, DiagramError> {
        let site = site_variables(app_theme);
        if policy == ThemeVariablesPolicy::Ignore {
            return Ok(Self {
                values: site.clone(),
                theme_css: ThemeCss::default(),
            });
        }
        let directive = sanitized_directive(config);
        let theme_css = directive
            .get("themeCSS")
            .and_then(Value::as_str)
            .map(ThemeCss::parse)
            .unwrap_or_default();
        let overrides = match directive.get("themeVariables") {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        };
        let directive_theme = directive
            .get("theme")
            .and_then(Value::as_str)
            .filter(|name| MERMAID_THEME_NAMES.contains(name));
        if let Some(theme) = directive_theme.and_then(MermaidTheme::from_name) {
            let values = theme_variables(theme, Some(&overrides))
                .map_err(|error| DiagramError::Syntax(error.to_string()))?;
            return Ok(Self { values, theme_css });
        }
        let mut values = site.clone();
        merge_overrides(&mut values, overrides);
        Ok(Self { values, theme_css })
    }

    /// 안쪽 묶음(`xyChart`, `radar` 등)
    pub fn section(&self, key: &str) -> Self {
        let values = self
            .values
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Self {
            values,
            theme_css: ThemeCss::default(),
        }
    }

    pub fn theme_css(&self) -> &ThemeCss {
        &self.theme_css
    }

    /// CSS에 그대로 들어가는 글자 값. 숫자도 글자로 읽는다
    pub fn text(&self, key: &str) -> Option<String> {
        match self.values.get(key)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        }
    }

    /// CSS 색. 브라우저가 읽지 못하는 값은 없는 것으로 본다
    pub fn color(&self, key: &str) -> Option<Color> {
        parse_theme_color(&self.text(key)?)
    }

    /// 숫자나 숫자 글자(`"0.7"`, `"16px"`의 앞 숫자는 읽지 않는다)
    pub fn number(&self, key: &str) -> Option<f64> {
        self.text(key)?.trim().parse().ok()
    }

    /// CSS 길이(`"7px"`, `"2"`, 숫자)의 앞 숫자. Mermaid `parseFontSize`처럼 단위는 버린다
    pub fn css_length(&self, key: &str) -> Option<f64> {
        let text = self.text(key)?;
        let trimmed = text.trim();
        let end = trimmed
            .char_indices()
            .find(|(index, character)| {
                !(character.is_ascii_digit()
                    || *character == '.'
                    || (*index == 0 && *character == '-'))
            })
            .map_or(trimmed.len(), |(index, _)| index);
        trimmed[..end].parse().ok()
    }

    /// JS 참·거짓(`darkMode` 등)
    pub fn flag(&self, key: &str) -> bool {
        match self.values.get(key) {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::String(text)) => !text.is_empty(),
            Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
            Some(Value::Object(_) | Value::Array(_)) => true,
            _ => false,
        }
    }

    /// 색이 있으면 `target`을 바꾼다
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

/// `initialize({ theme })`가 계산해 두는 앱 테마 변수
fn site_variables(app_theme: Theme) -> &'static Map<String, Value> {
    static LIGHT: OnceLock<Map<String, Value>> = OnceLock::new();
    static DARK: OnceLock<Map<String, Value>> = OnceLock::new();
    let (cell, theme) = match app_theme {
        Theme::Light => (&LIGHT, MermaidTheme::Default),
        Theme::Dark => (&DARK, MermaidTheme::Dark),
    };
    cell.get_or_init(|| theme_variables(theme, None).unwrap_or_default())
}

/// `assignWithDepth(siteThemeVariables, directiveThemeVariables)`: 안쪽 묶음은 열쇠마다 겹치고, 객체와 글자가 엇갈리면 두지 않는다
fn merge_overrides(values: &mut Map<String, Value>, overrides: Map<String, Value>) {
    for (key, value) in overrides {
        match (values.get_mut(&key), value) {
            (Some(Value::Object(existing)), Value::Object(inner)) => existing.extend(inner),
            (None, value) => {
                values.insert(key, value);
            }
            (Some(existing), value) if !existing.is_object() && !value.is_object() => {
                *existing = value;
            }
            _ => {}
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

/// 정의 설정에서 테마에 쓰는 열쇠(`theme`·`themeVariables`·`themeCSS`)만 골라 Mermaid가 `addDirective`에서 거치는 두 단계
/// (`sanitizeDirective` 다음 `updateCurrentConfig`의 `sanitize`)를 같은 차례로 지난 값
pub(crate) fn sanitized_directive(config: &DiagramConfig) -> Map<String, Value> {
    let mut directive: Map<String, Value> = ["theme", "themeVariables", "themeCSS"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), config.tree.get(key)?.clone())))
        .collect();
    sanitize_directive(&mut directive);
    if let Some(Value::Object(variables)) = directive.get_mut("themeVariables") {
        blank_unsafe_theme_values(variables);
    }
    sanitize(&directive)
}

/// `sanitizeDirective`: 기본 설정에 없는 열쇠, `proto`·`constr`가 든 열쇠, null을 지우고 안쪽 묶음도 같은 규칙으로 거른다.
/// CSS가 들어가는 열쇠(`themeCSS`·`fontFamily`)는 중괄호 짝이 맞지 않으면 오류 주석으로 바꾼다
fn sanitize_directive(map: &mut Map<String, Value>) {
    map.retain(|key, value| {
        !key.starts_with("__")
            && !key.contains("proto")
            && !key.contains("constr")
            && CONFIG_KEYS.binary_search(&key.as_str()).is_ok()
            && !value.is_null()
    });
    for (key, value) in map.iter_mut() {
        match value {
            // nodeColors는 색 모양만 남기는 다른 규칙인데 테마와 상관없어 그대로 둔다
            Value::Object(inner) if key != "nodeColors" => sanitize_directive(inner),
            Value::String(text)
                if ["themeCSS", "fontFamily", "altFontFamily"]
                    .iter()
                    .any(|css_key| key.contains(css_key)) =>
            {
                *text = sanitize_css(text);
            }
            _ => {}
        }
    }
}

/// `sanitizeCss`: 닫는 중괄호가 여는 것보다 먼저 많아지거나 개수가 다르면 오류 주석만 남긴다
fn sanitize_css(text: &str) -> String {
    const UNBALANCED: &str = "{ /* ERROR: Unbalanced CSS */ }";
    let (mut opened, mut closed) = (0usize, 0usize);
    for character in text.chars() {
        if opened < closed {
            return UNBALANCED.to_string();
        }
        match character {
            '{' => opened += 1,
            '}' => closed += 1,
            _ => {}
        }
    }
    if opened == closed {
        text.to_string()
    } else {
        UNBALANCED.to_string()
    }
}

/// `sanitizeDirective` 끝: `themeVariables` 바로 아래 글자 값이 `[\d "#%(),.;A-Za-z]+`가 아니면 빈 글자로 바꾼다
fn blank_unsafe_theme_values(variables: &mut Map<String, Value>) {
    let is_allowed =
        |character: char| character.is_ascii_alphanumeric() || " \"#%(),.;".contains(character);
    for value in variables.values_mut() {
        if let Value::String(text) = value
            && !(!text.is_empty() && text.chars().all(is_allowed))
        {
            text.clear();
        }
    }
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

    fn read(definition: &str, policy: ThemeVariablesPolicy) -> ThemeVariables {
        ThemeVariables::read(&read_config(definition), policy, Theme::Light).unwrap()
    }

    #[test]
    fn 데스크톱은_앞머리와_지시문의_값을_겹쳐_읽는다() {
        let variables = read(DEFINITION, ThemeVariablesPolicy::Apply);
        assert_eq!(variables.color("mainBkg"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(variables.color("lineColor"), Some(Color::rgb(0, 128, 0)));
        let radar = variables.section("radar");
        assert_eq!(radar.number("curveOpacity"), Some(0.25));
        assert_eq!(radar.number("axisStrokeWidth"), Some(2.0));
        // sanitizeDirective가 허용 글자 밖의 값을 빈 글자로 바꾼다
        assert_eq!(variables.text("textColor").as_deref(), Some(""));
        assert_eq!(variables.text("__proto"), None);
    }

    #[test]
    fn 모바일은_정의의_값을_쓰지_않는다() {
        let variables = read(DEFINITION, ThemeVariablesPolicy::Ignore);
        assert_eq!(variables.text("mainBkg").as_deref(), Some("#ECECFF"));
        let forest = read(
            "%%{init: {'theme': 'forest'}}%%\nflowchart LR\n  A",
            ThemeVariablesPolicy::Ignore,
        );
        assert_eq!(forest, variables);
    }

    #[test]
    fn themecss는_데스크톱만_읽고_짝이_맞지_않으면_버린다() {
        let definition =
            "%%{init: {'themeCSS': '.node rect { fill: #ff0000; }'}}%%\nflowchart LR\n  A";
        let desktop = read(definition, ThemeVariablesPolicy::Apply);
        let fill = desktop.theme_css().color(&[".node rect"], "fill");
        assert_eq!(fill, Some(Color::rgb(255, 0, 0)));
        assert!(
            read(definition, ThemeVariablesPolicy::Ignore)
                .theme_css()
                .is_empty()
        );
        let unbalanced =
            "%%{init: {'themeCSS': '.node rect { fill: #ff0000; '}}%%\nflowchart LR\n  A";
        let broken = read(unbalanced, ThemeVariablesPolicy::Apply);
        assert_eq!(broken.theme_css().value(&[".node rect"], "fill"), None);
    }

    #[test]
    fn 테마가_없으면_파생_변수를_다시_계산하지_않는다() {
        let definition =
            "%%{init: {'themeVariables': {'primaryColor': '#ff0000'}}}%%\nflowchart LR\n  A";
        let variables = read(definition, ThemeVariablesPolicy::Apply);
        assert_eq!(variables.text("primaryColor").as_deref(), Some("#ff0000"));
        assert_eq!(variables.text("mainBkg").as_deref(), Some("#ECECFF"));
    }

    #[test]
    fn 테마를_고르면_그_테마로_다시_계산한다() {
        let definition = "---\nconfig:\n  theme: base\n  themeVariables:\n    primaryColor: '#ff0000'\n---\nflowchart LR\n  A";
        let variables = read(definition, ThemeVariablesPolicy::Apply);
        assert_eq!(variables.text("mainBkg").as_deref(), Some("#ff0000"));
        let forest = read(
            "%%{init: {'theme': 'forest'}}%%\nflowchart LR\n  A",
            ThemeVariablesPolicy::Apply,
        );
        assert_eq!(forest.text("mainBkg").as_deref(), Some("#cde498"));
        let neo = read(
            "%%{init: {'theme': 'neo'}}%%\nflowchart LR\n  A",
            ThemeVariablesPolicy::Apply,
        );
        assert_eq!(neo.text("mainBkg").as_deref(), Some("#ECECFF"));
    }

    #[test]
    fn 다시_계산할_때만_읽지_못하는_색이_실패한다() {
        let overrides = "'themeVariables': {'primaryColor': 'steelblue'}";
        let with_theme =
            format!("%%{{init: {{'theme': 'base', {overrides}}}}}%%\nflowchart LR\n  A");
        let config = read_config(&with_theme);
        assert!(ThemeVariables::read(&config, ThemeVariablesPolicy::Apply, Theme::Light).is_err());
        let without_theme = format!("%%{{init: {{{overrides}}}}}%%\nflowchart LR\n  A");
        assert!(
            ThemeVariables::read(
                &read_config(&without_theme),
                ThemeVariablesPolicy::Apply,
                Theme::Light
            )
            .is_ok()
        );
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
