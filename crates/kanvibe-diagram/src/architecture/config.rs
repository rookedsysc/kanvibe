//! `config.architecture`(defaultConfig.architecture). 앞머리나 `init` 지시문으로 바꾼 숫자만 기본값을 덮는다.
//!
//! `randomize: true`는 fcose가 `Math.random`으로 스펙트럼 배치를 시작해 TS 결과도 매번 달라지므로 따르지 않고
//! 늘 `false`로 둔다. `nodeSeparation`은 그 스펙트럼 배치에서만 쓰여 읽지 않는다.

use serde_json::Value;

use crate::directive::read_config;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ArchitectureConfig {
    pub icon_size: f64,
    /// 그룹 안쪽 여백이자 viewBox 여백
    pub padding: f64,
    /// cytoscape 노드 라벨 글자 크기(배치 경계 상자)와 그룹 라벨 세로 자리
    pub font_size: f64,
    pub ideal_edge_length_multiplier: f64,
    pub edge_elasticity: f64,
    /// fcose `numIter`
    pub iterations: f64,
}

impl Default for ArchitectureConfig {
    fn default() -> Self {
        Self {
            icon_size: 80.0,
            padding: 40.0,
            font_size: 16.0,
            ideal_edge_length_multiplier: 1.5,
            edge_elasticity: 0.45,
            iterations: 2500.0,
        }
    }
}

impl ArchitectureConfig {
    /// 숫자가 아닌 값은 무시한다(문자열 숫자는 JS에서 덧셈이 글자 잇기가 되어 그림이 깨진다)
    pub fn from_definition(definition: &str) -> Self {
        let mut config = Self::default();
        let tree = read_config(definition);
        let Some(section) = tree.section("architecture") else {
            return config;
        };
        let numbers: [(&str, &mut f64); 6] = [
            ("iconSize", &mut config.icon_size),
            ("padding", &mut config.padding),
            ("fontSize", &mut config.font_size),
            (
                "idealEdgeLengthMultiplier",
                &mut config.ideal_edge_length_multiplier,
            ),
            ("edgeElasticity", &mut config.edge_elasticity),
            ("numIter", &mut config.iterations),
        ];
        for (key, target) in numbers {
            if let Some(value) = section.get(key).and_then(Value::as_f64) {
                *target = value;
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 앞머리와_지시문의_architecture_숫자를_읽는다() {
        let config = ArchitectureConfig::from_definition(
            "---\nconfig:\n  architecture:\n    iconSize: 60\n    padding: \"30\"\n---\n%%{init: {\"architecture\": {\"fontSize\": 20}}}%%\narchitecture-beta",
        );
        assert_eq!(config.icon_size, 60.0);
        assert_eq!(config.padding, 40.0);
        assert_eq!(config.font_size, 20.0);
    }
}
