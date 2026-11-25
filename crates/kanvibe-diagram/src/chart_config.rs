//! 차트형 다이어그램(xychart·radar·venn)이 `config.<종류>`에서 값을 읽는 방식.
//!
//! Mermaid `cleanAndMerge`처럼 형이 맞는 값만 기본값을 덮는다. 숫자 자리에 글자가 오는 식으로 형이 어긋나면
//! 무시한다. `themeVariables`는 여기서 읽지 않고 [`crate::theme_variables`]가 부르는 쪽 설정에 따라 읽는다.

use serde_json::{Map, Value};

use crate::directive::DiagramConfig;

/// 설정 나무의 한 묶음. 묶음이 없으면 모든 값을 기본값으로 둔다
#[derive(Clone, Copy)]
pub struct ConfigSection<'a> {
    map: Option<&'a Map<String, Value>>,
}

impl<'a> ConfigSection<'a> {
    /// `config.<key>` 묶음(`xyChart`, `radar`, `venn` 등)
    pub fn of(config: &'a DiagramConfig, key: &str) -> Self {
        Self {
            map: config.section(key),
        }
    }

    /// 안쪽 묶음(`xAxis` 등)
    pub fn child(&self, key: &str) -> Self {
        Self {
            map: self
                .map
                .and_then(|map| map.get(key))
                .and_then(Value::as_object),
        }
    }

    fn value(&self, key: &str) -> Option<&'a Value> {
        self.map.and_then(|map| map.get(key))
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.value(key).and_then(Value::as_f64)
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        self.value(key).and_then(Value::as_bool)
    }

    pub fn text(&self, key: &str) -> Option<&'a str> {
        self.value(key).and_then(Value::as_str)
    }

    /// 숫자가 있으면 `target`을 덮는다
    pub fn read_number(&self, key: &str, target: &mut f64) {
        if let Some(value) = self.number(key) {
            *target = value;
        }
    }

    pub fn read_flag(&self, key: &str, target: &mut bool) {
        if let Some(value) = self.flag(key) {
            *target = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 형이_맞는_값만_읽는다() {
        let config = read_config(
            "%%{init: {\"xyChart\": {\"width\": 400, \"height\": \"x\", \"xAxis\": {\"showLabel\": false}}}}%%\nxychart",
        );
        let section = ConfigSection::of(&config, "xyChart");
        assert_eq!(section.number("width"), Some(400.0));
        assert_eq!(section.number("height"), None);
        assert_eq!(section.child("xAxis").flag("showLabel"), Some(false));
    }
}
