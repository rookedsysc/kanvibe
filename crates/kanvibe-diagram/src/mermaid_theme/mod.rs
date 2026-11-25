//! Mermaid 11.15 테마 변수 계산(`themes[name].getThemeVariables(overrides)`).
//!
//! 테마 클래스의 constructor·updateColors는 [`themes`]에 문장 단위로 옮겼고(생성 코드), 여기에는 그 코드가 쓰는 JS 값
//! 의미([`JsValue`]: `||`의 참·거짓, `??`, 글자 더하기)와 `calculate`가 있다. `calculate`는 덮을 값을 먼저 넣고
//! updateColors로 파생 변수를 채운 뒤 덮을 값을 한 번 더 넣는다. 그래서 덮은 값에서 파생되는 변수도 그 값을 따른다.

// Mermaid 번들에서 생성한 코드라 rustfmt가 다시 줄을 맞추지 않게 둔다(생성기는 커밋 a73f3498까지 parity/tools/diagrams/generate_mermaid_themes.cjs)
#[rustfmt::skip]
mod config_keys;
mod js_number;
pub mod khroma;
#[rustfmt::skip]
mod themes;

use indexmap::IndexMap;
use serde_json::{Map, Value};

pub(crate) use config_keys::CONFIG_KEYS;
use js_number::number_to_js_string;
pub use khroma::KhromaError;

/// 네이티브 렌더러가 계산하는 Mermaid 테마. `neo`·`redux` 계열은 다이어그램 styles가 테마 이름으로 갈라 그려 옮기지 않았다
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MermaidTheme {
    Base,
    Dark,
    Default,
    Forest,
    Neutral,
}

impl MermaidTheme {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "base" => Self::Base,
            "dark" => Self::Dark,
            "default" => Self::Default,
            "forest" => Self::Forest,
            "neutral" => Self::Neutral,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Dark => "dark",
            Self::Default => "default",
            Self::Forest => "forest",
            Self::Neutral => "neutral",
        }
    }
}

/// Mermaid `themes_default`에 있는 이름. `theme` 지시문은 이 이름일 때만 테마 변수를 다시 계산한다
pub const MERMAID_THEME_NAMES: [&str; 11] = [
    "base",
    "dark",
    "default",
    "forest",
    "neutral",
    "neo",
    "neo-dark",
    "redux",
    "redux-dark",
    "redux-color",
    "redux-dark-color",
];

/// 테마 코드가 다루는 JS 값
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum JsValue {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(IndexMap<String, JsValue>),
}

impl From<&str> for JsValue {
    fn from(text: &str) -> Self {
        Self::String(text.to_string())
    }
}

impl JsValue {
    pub(crate) fn object(entries: Vec<(&str, JsValue)>) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub(crate) fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(flag) => *flag,
            Self::Number(number) => *number != 0.0 && !number.is_nan(),
            Self::String(text) => !text.is_empty(),
            Self::Object(_) => true,
        }
    }

    pub(crate) fn is_nullish(&self) -> bool {
        matches!(self, Self::Undefined | Self::Null)
    }

    /// `value?.key`
    pub(crate) fn member(&self, key: &str) -> Self {
        match self {
            Self::Object(entries) => entries.get(key).cloned().unwrap_or_default(),
            _ => Self::Undefined,
        }
    }

    /// `String(value)`
    pub(crate) fn to_property_key(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_string(),
            Self::Null => "null".to_string(),
            Self::Bool(flag) => flag.to_string(),
            Self::Number(number) => number_to_js_string(*number),
            Self::String(text) => text.clone(),
            Self::Object(_) => "[object Object]".to_string(),
        }
    }

    /// `Number(value)`
    pub(crate) fn to_number(&self) -> f64 {
        match self {
            Self::Undefined | Self::Object(_) => f64::NAN,
            Self::Null => 0.0,
            Self::Bool(flag) => f64::from(u8::from(*flag)),
            Self::Number(number) => *number,
            Self::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse().unwrap_or(f64::NAN)
                }
            }
        }
    }

    /// `left + right`: 한쪽이 글자면 이어 붙인다
    pub(crate) fn add(&self, other: &Self) -> Self {
        let is_text = |value: &Self| matches!(value, Self::String(_) | Self::Object(_));
        if is_text(self) || is_text(other) {
            return Self::String(self.to_property_key() + &other.to_property_key());
        }
        Self::Number(self.to_number() + other.to_number())
    }

    pub(crate) fn multiply(&self, other: &Self) -> Self {
        Self::Number(self.to_number() * other.to_number())
    }

    pub(crate) fn negate(&self) -> Self {
        Self::Number(-self.to_number())
    }

    /// `left === right`
    pub(crate) fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left == right,
            (Self::Object(_), Self::Object(_)) => false,
            _ => self == other,
        }
    }

    fn from_json(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(flag) => Self::Bool(*flag),
            Value::Number(number) => Self::Number(number.as_f64().unwrap_or(f64::NAN)),
            Value::String(text) => Self::String(text.clone()),
            Value::Array(items) => Self::Object(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| (index.to_string(), Self::from_json(item)))
                    .collect(),
            ),
            Value::Object(entries) => Self::Object(
                entries
                    .iter()
                    .map(|(key, item)| (key.clone(), Self::from_json(item)))
                    .collect(),
            ),
        }
    }

    /// JSON으로 옮길 수 없는 값(undefined, NaN)은 null이 된다
    fn to_json(&self) -> Value {
        match self {
            Self::Undefined | Self::Null => Value::Null,
            Self::Bool(flag) => Value::Bool(*flag),
            Self::Number(number) => {
                serde_json::Number::from_f64(*number).map_or(Value::Null, Value::Number)
            }
            Self::String(text) => Value::String(text.clone()),
            Self::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, item)| (key.clone(), item.to_json()))
                    .collect(),
            ),
        }
    }
}

/// 테마 클래스 인스턴스(`this`)
#[derive(Debug, Default)]
pub(crate) struct ThemeObject {
    properties: IndexMap<String, JsValue>,
}

impl ThemeObject {
    pub(crate) fn get(&self, key: &str) -> JsValue {
        self.properties.get(key).cloned().unwrap_or_default()
    }

    pub(crate) fn set(&mut self, key: &str, value: JsValue) {
        self.properties.insert(key.to_string(), value);
    }

    fn clear_calculated(&mut self) {
        for value in self.properties.values_mut() {
            if *value == JsValue::from("calculated") {
                *value = JsValue::Undefined;
            }
        }
    }
}

/// 정의가 `THEME_COLOR_LIMIT`를 크게 덮어도 그리기가 멈추지 않도록 테마 반복 횟수에 둔 상한
const MAX_THEME_LOOP: u32 = 256;

/// `for (let i = 0; i < limit; i++)`의 반복 횟수
pub(crate) fn loop_limit(limit: &JsValue) -> u32 {
    let number = limit.to_number();
    if number.is_nan() || number <= 0.0 {
        return 0;
    }
    number.ceil().min(f64::from(MAX_THEME_LOOP)) as u32
}

/// `themes[name].getThemeVariables(overrides)`. 덮을 값이 없으면 `None`(Mermaid `initialize`가 넘기는 undefined)
pub fn theme_variables(
    theme: MermaidTheme,
    overrides: Option<&Map<String, Value>>,
) -> Result<Map<String, Value>, KhromaError> {
    let mut object = ThemeObject::default();
    let (constructor, update_colors): (ThemeStep, ThemeStep) = match theme {
        MermaidTheme::Base => (themes::base_constructor, themes::base_update_colors),
        MermaidTheme::Dark => (themes::dark_constructor, themes::dark_update_colors),
        MermaidTheme::Default => (
            themes::default_theme_constructor,
            themes::default_theme_update_colors,
        ),
        MermaidTheme::Forest => (themes::forest_constructor, themes::forest_update_colors),
        MermaidTheme::Neutral => (themes::neutral_constructor, themes::neutral_update_colors),
    };
    constructor(&mut object)?;
    // theme-default.js의 calculate만 constructor가 "calculated"로 둔 변수를 먼저 undefined로 비운다
    if theme == MermaidTheme::Default {
        object.clear_calculated();
    }
    let overrides: Vec<(String, JsValue)> = overrides
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.clone(), JsValue::from_json(value)))
        .collect();
    apply_overrides(&mut object, &overrides);
    update_colors(&mut object)?;
    apply_overrides(&mut object, &overrides);
    Ok(object
        .properties
        .iter()
        .filter(|(_, value)| **value != JsValue::Undefined)
        .map(|(key, value)| (key.clone(), value.to_json()))
        .collect())
}

type ThemeStep = fn(&mut ThemeObject) -> Result<(), KhromaError>;

fn apply_overrides(object: &mut ThemeObject, overrides: &[(String, JsValue)]) {
    for (key, value) in overrides {
        object.set(key, value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mermaid 번들로 만든 골든 값(생성기는 커밋 a73f3498까지 parity/tools/diagrams/dump_mermaid_theme_variables.mjs)
    const GOLDEN: &str = include_str!("../../../../parity/diagrams/mermaid-theme-variables.json");

    /// 숫자는 정수·소수 표기와 상관없이 같은 값이면 같다
    fn normalize(value: &Value) -> Value {
        match value {
            Value::Number(number) => Value::String(number_to_js_string(number.as_f64().unwrap())),
            Value::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, item)| (key.clone(), normalize(item)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn expected_variables(
        golden: &Map<String, Value>,
        theme: &str,
        case: &Value,
    ) -> Map<String, Value> {
        let mut variables = golden[&format!("{theme}/none")]["variables"]
            .as_object()
            .unwrap()
            .clone();
        for removed in case["removed"].as_array().unwrap() {
            variables.remove(removed.as_str().unwrap());
        }
        for (key, value) in case["changed"].as_object().unwrap() {
            variables.insert(key.clone(), value.clone());
        }
        variables
    }

    #[test]
    fn 다섯_테마의_변수가_mermaid와_같다() {
        let golden: Map<String, Value> = serde_json::from_str(GOLDEN).unwrap();
        let mut mismatches = Vec::new();
        for (key, case) in &golden {
            let (theme, _) = key.split_once('/').unwrap();
            let theme_kind = MermaidTheme::from_name(theme).unwrap();
            let actual = theme_variables(theme_kind, case["overrides"].as_object());
            if let Some(error) = case.get("error") {
                match actual {
                    Err(actual_error) => assert!(
                        actual_error.to_string().contains(error.as_str().unwrap()),
                        "{key}: {actual_error}"
                    ),
                    Ok(_) => mismatches.push(format!("{key}: 실패해야 한다")),
                }
                continue;
            }
            let expected = match case.get("variables") {
                Some(variables) => variables.as_object().unwrap().clone(),
                None => expected_variables(&golden, theme, case),
            };
            let actual = actual.unwrap_or_else(|error| panic!("{key}: {error}"));
            for (name, value) in &expected {
                let actual_value = actual.get(name).map(normalize).unwrap_or(Value::Null);
                if actual_value != normalize(value) {
                    mismatches.push(format!("{key} {name}: {value} != {actual_value}"));
                }
            }
            for name in actual.keys().filter(|name| !expected.contains_key(*name)) {
                mismatches.push(format!("{key} {name}: 남는 변수"));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }
}
