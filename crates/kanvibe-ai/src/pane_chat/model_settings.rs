//! pane에서 돌고 있는 CLI의 모델·effort·속도 설정. 기록에서 현재 값을 읽고, 고를 수 있는 선택지를 만들고,
//! 바꾸려는 값을 CLI에 칠 슬래시 명령으로 옮긴다.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::LazyLock;

/// 슬라이더의 한 칸. id는 CLI 명령에 그대로 쓰는 값이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatEffortLevel {
    pub id: String,
    pub label: String,
}

/// 직렬화할 때의 키 순서. TS는 Claude 모델 계열을 `{ ...family, effortLevels }`로 만들어 `supportsFast`가
/// `effortLevels` 앞에 오고, Codex 목록과 현재 모델 항목은 `effortLevels`가 먼저다. 모바일 계약이 바이트 단위로 같도록 그 순서를 지킨다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneChatModelOptionKeyOrder {
    #[default]
    EffortLevelsFirst,
    SupportsFastFirst,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatModelOption {
    /// CLI의 `/model` 명령에 그대로 쓰는 값. Claude는 별칭, Codex는 slug다
    pub id: String,
    pub label: String,
    /// 낮은 단계부터. 비어 있으면 이 모델은 effort를 고를 수 없다
    pub effort_levels: Vec<PaneChatEffortLevel>,
    pub supports_fast: bool,
    #[serde(skip)]
    pub key_order: PaneChatModelOptionKeyOrder,
}

impl Serialize for PaneChatModelOption {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("PaneChatModelOption", 4)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("label", &self.label)?;
        if self.key_order == PaneChatModelOptionKeyOrder::SupportsFastFirst {
            record.serialize_field("supportsFast", &self.supports_fast)?;
            record.serialize_field("effortLevels", &self.effort_levels)?;
        } else {
            record.serialize_field("effortLevels", &self.effort_levels)?;
            record.serialize_field("supportsFast", &self.supports_fast)?;
        }
        record.end()
    }
}

/// pane에서 돌고 있는 CLI의 현재 모델 설정과 고를 수 있는 선택지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatModelSettings {
    /// models 중 하나의 id. 목록에 없는 모델을 쓰고 있으면 그 모델도 목록에 넣어 준다
    pub model: Option<String>,
    pub effort: Option<String>,
    /// 기록에서 알아내지 못했으면 `None`이다
    pub is_fast: Option<bool>,
    pub models: Vec<PaneChatModelOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PaneChatModelSettingChange {
    Model {
        model: String,
    },
    Effort {
        effort: String,
    },
    Fast {
        #[serde(rename = "isFast")]
        is_fast: bool,
    },
    /// effort를 그 모델의 기본값으로 되돌린다
    Reset,
}

/// 모델 설정을 다루는 CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneChatModelProvider {
    Claude,
    Codex,
}

fn as_record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// 비어 있지 않은 문자열
fn as_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// 요청으로 들어온 값이 변경 하나의 모양을 갖췄는지 확인한다. 값이 지금 고를 수 있는 것인지는 명령을 만들 때 따진다
pub fn parse_pane_chat_model_setting_change(value: &Value) -> Option<PaneChatModelSettingChange> {
    let change = value.as_object()?;
    match change.get("kind").and_then(Value::as_str)? {
        "model" => Some(PaneChatModelSettingChange::Model {
            model: change.get("model")?.as_str()?.to_string(),
        }),
        "effort" => Some(PaneChatModelSettingChange::Effort {
            effort: change.get("effort")?.as_str()?.to_string(),
        }),
        "fast" => Some(PaneChatModelSettingChange::Fast {
            is_fast: change.get("isFast")?.as_bool()?,
        }),
        "reset" => Some(PaneChatModelSettingChange::Reset),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatModelState {
    /// CLI가 기록에 남긴 모델 이름. Claude는 `claude-opus-5-5` 같은 전체 ID다
    pub model: Option<String>,
    pub effort: Option<String>,
    pub is_fast: Option<bool>,
}

fn effort_label(id: &str) -> String {
    match id {
        "low" => "Light",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        other => other,
    }
    .to_string()
}

fn to_effort_levels<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<PaneChatEffortLevel> {
    ids.into_iter()
        .map(|id| PaneChatEffortLevel {
            id: id.to_string(),
            label: effort_label(id),
        })
        .collect()
}

/// Claude Code가 `/model`로 받는 별칭과 그 모델군의 선택지.
/// 계정별 실제 목록은 CLI 밖에서 읽을 방법이 없어, 공식 문서의 별칭과 단계 표를 그대로 옮긴다
const CLAUDE_MODEL_FAMILIES: [(&str, &str, bool); 4] = [
    ("fable", "Fable", true),
    ("opus", "Opus", true),
    ("sonnet", "Sonnet", false),
    ("haiku", "Haiku", false),
];
const COMMAND_OUTPUT_TAG: &str = "<local-command-stdout>";
const CLAUDE_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

static SET_MODEL_OUTPUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("Set model to `([^`]+)`").expect("모델 변경 출력 패턴"));
static SET_EFFORT_OUTPUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("Set effort level to ([A-Za-z0-9_]+)").expect("effort 변경 출력 패턴")
});

/// Claude Code 기록의 끝부분에서 가장 최근 응답이 쓴 모델과 effort를 읽는다
pub fn read_claude_model_state(records: &[Value]) -> PaneChatModelState {
    let mut state = PaneChatModelState::default();
    for entry in records {
        let Some(record) = entry.as_object() else {
            continue;
        };
        let message = as_record(record.get("message"));
        let record_type = record.get("type").and_then(Value::as_str);
        // 설정 명령의 결과는 다음 응답이 오기 전에도 기록에 남으므로, 바꾼 직후의 값을 여기서 먼저 안다
        let command_output = (record_type == Some("user"))
            .then(|| as_string(message.and_then(|message| message.get("content"))))
            .flatten()
            .filter(|output| output.contains(COMMAND_OUTPUT_TAG));
        if let Some(output) = command_output {
            apply_claude_command_output(&mut state, output);
            continue;
        }
        let model = as_string(message.and_then(|message| message.get("model")));
        // CLI가 스스로 만든 안내 응답은 모델을 쓰지 않는다
        let Some(model) =
            model.filter(|model| record_type == Some("assistant") && *model != "<synthetic>")
        else {
            continue;
        };
        state.model = Some(
            as_string(record.get("requestedModel"))
                .unwrap_or(model)
                .to_string(),
        );
        if let Some(effort) =
            as_string(record.get("effort")).or_else(|| as_string(record.get("perTurnEffort")))
        {
            state.effort = Some(effort.to_string());
        }
        let usage = as_record(message.and_then(|message| message.get("usage")));
        if let Some(speed) = as_string(usage.and_then(|usage| usage.get("speed"))) {
            state.is_fast = Some(speed != "standard");
        }
    }
    state
}

fn apply_claude_command_output(state: &mut PaneChatModelState, output: &str) {
    if let Some(found) = SET_MODEL_OUTPUT.captures(output) {
        state.model = Some(found[1].to_string());
    }
    if let Some(found) = SET_EFFORT_OUTPUT.captures(output) {
        state.effort = Some(found[1].to_string());
    }
}

/// Codex 기록의 끝부분에서 가장 최근에 적용된 모델·effort·속도 등급을 읽는다
pub fn read_codex_model_state(records: &[Value]) -> PaneChatModelState {
    let mut state = PaneChatModelState::default();
    for entry in records {
        let Some(record) = entry.as_object() else {
            continue;
        };
        let payload = as_record(record.get("payload"));
        let field = |key: &str| as_string(payload.and_then(|payload| payload.get(key)));
        let record_type = record.get("type").and_then(Value::as_str);
        if record_type == Some("turn_context") {
            replace_if_present(&mut state.model, field("model"));
            replace_if_present(&mut state.effort, field("effort"));
            continue;
        }
        // 턴 사이에 바꾼 설정은 이 이벤트로만 남는다. 속도 등급도 여기에만 실린다
        let is_settings_event = record_type == Some("event_msg")
            && payload
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str)
                == Some("thread_settings_applied");
        let Some(settings) = is_settings_event
            .then(|| as_record(payload.and_then(|payload| payload.get("thread_settings"))))
            .flatten()
        else {
            continue;
        };
        replace_if_present(&mut state.model, as_string(settings.get("model")));
        replace_if_present(
            &mut state.effort,
            as_string(settings.get("reasoning_effort")),
        );
        state.is_fast =
            Some(settings.get("service_tier").and_then(Value::as_str) == Some("priority"));
    }
    state
}

fn replace_if_present(target: &mut Option<String>, value: Option<&str>) {
    if let Some(value) = value {
        *target = Some(value.to_string());
    }
}

/// JS `Number(value ?? 0)`. 숫자로 바꿀 수 없으면 NaN이다
fn js_number_or_zero(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = crate::javascript::js_trim(text);
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Array(items)) if items.is_empty() => 0.0,
        Some(_) => f64::NAN,
    }
}

/// Codex가 계정 기준으로 받아 둔 모델 캐시(`~/.codex/models_cache.json`)에서 고를 수 있는 모델을 읽는다
pub fn parse_codex_model_catalog(cache: &Value) -> Vec<PaneChatModelOption> {
    let Some(models) = cache
        .get("models")
        .and_then(Value::as_array)
        .filter(|_| cache.is_object())
    else {
        return Vec::new();
    };
    let mut listed: Vec<&Map<String, Value>> = models
        .iter()
        .filter_map(Value::as_object)
        .filter(|model| {
            model.get("visibility").and_then(Value::as_str) == Some("list")
                && as_string(model.get("slug")).is_some()
        })
        .collect();
    listed.sort_by(|left, right| {
        js_number_or_zero(left.get("priority"))
            .partial_cmp(&js_number_or_zero(right.get("priority")))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    listed.into_iter().map(codex_model_option).collect()
}

fn codex_model_option(model: &Map<String, Value>) -> PaneChatModelOption {
    let slug = as_string(model.get("slug")).unwrap_or_default();
    let efforts = model
        .get("supported_reasoning_levels")
        .and_then(Value::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(|level| {
                    as_string(level.as_object().and_then(|level| level.get("effort")))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let supports_fast = model
        .get("service_tiers")
        .and_then(Value::as_array)
        .is_some_and(|tiers| {
            tiers.iter().any(|tier| {
                tier.as_object()
                    .and_then(|tier| tier.get("id"))
                    .and_then(Value::as_str)
                    == Some("priority")
            })
        });
    PaneChatModelOption {
        id: slug.to_string(),
        label: as_string(model.get("display_name"))
            .unwrap_or(slug)
            .to_string(),
        effort_levels: to_effort_levels(efforts),
        supports_fast,
        key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
    }
}

fn build_claude_model_settings(state: PaneChatModelState) -> PaneChatModelSettings {
    let models: Vec<PaneChatModelOption> = CLAUDE_MODEL_FAMILIES
        .iter()
        .map(|(id, label, supports_fast)| PaneChatModelOption {
            id: id.to_string(),
            label: label.to_string(),
            effort_levels: to_effort_levels(CLAUDE_EFFORT_LEVELS),
            supports_fast: *supports_fast,
            key_order: PaneChatModelOptionKeyOrder::SupportsFastFirst,
        })
        .collect();
    let family = state
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .and_then(|model| {
            let lowered = model.to_lowercase();
            CLAUDE_MODEL_FAMILIES
                .iter()
                .find(|(id, _, _)| lowered.contains(id))
        });
    let model = family.map(|(id, _, _)| id.to_string()).or(state.model);
    PaneChatModelSettings {
        models: with_current_model(models, model.as_deref()),
        model,
        effort: state.effort,
        is_fast: state.is_fast,
    }
}

/// 목록에 없는 모델을 쓰고 있어도 화면이 현재 모델을 보여 줄 수 있게 한다. 고를 수 있는 옵션은 알 수 없으므로 비워 둔다
fn with_current_model(
    mut models: Vec<PaneChatModelOption>,
    current: Option<&str>,
) -> Vec<PaneChatModelOption> {
    let Some(current) = current.filter(|current| !current.is_empty()) else {
        return models;
    };
    if models.iter().any(|model| model.id == current) {
        return models;
    }
    models.insert(
        0,
        PaneChatModelOption {
            id: current.to_string(),
            label: current.to_string(),
            effort_levels: Vec::new(),
            supports_fast: false,
            key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
        },
    );
    models
}

pub fn build_pane_chat_model_settings(
    provider: PaneChatModelProvider,
    state: PaneChatModelState,
    codex_catalog: Vec<PaneChatModelOption>,
) -> PaneChatModelSettings {
    if provider == PaneChatModelProvider::Claude {
        return build_claude_model_settings(state);
    }
    PaneChatModelSettings {
        models: with_current_model(codex_catalog, state.model.as_deref()),
        model: state.model,
        effort: state.effort,
        is_fast: state.is_fast,
    }
}

/// 설정 변경 하나를 CLI에 칠 슬래시 명령으로 바꾼다. 지금 고를 수 없는 값이면 `None`이다.
///
/// 명령에 들어가는 값은 반드시 선택지 목록에 있는 것만 쓴다. 요청에 실려 온 문자열을 그대로 터미널에 치지 않기 위해서다
pub fn build_pane_chat_model_command(
    provider: PaneChatModelProvider,
    settings: &PaneChatModelSettings,
    change: &PaneChatModelSettingChange,
) -> Option<String> {
    let current_model = settings
        .models
        .iter()
        .find(|model| Some(model.id.as_str()) == settings.model.as_deref());
    let is_claude = provider == PaneChatModelProvider::Claude;
    match change {
        PaneChatModelSettingChange::Model { model } => settings
            .models
            .iter()
            .find(|candidate| candidate.id == *model)
            .map(|model| format!("/model {}", model.id)),
        PaneChatModelSettingChange::Effort { effort } => {
            let level = current_model?
                .effort_levels
                .iter()
                .find(|candidate| candidate.id == *effort)?;
            Some(if is_claude {
                format!("/effort {}", level.id)
            } else {
                format!("/model --reasoning={}", level.id)
            })
        }
        PaneChatModelSettingChange::Fast { is_fast } => {
            if !current_model?.supports_fast {
                return None;
            }
            Some(match (is_claude, is_fast) {
                (true, true) => "/fast on".to_string(),
                (true, false) => "/fast off".to_string(),
                (false, true) => "/model --speed=priority".to_string(),
                (false, false) => "/model --speed=standard".to_string(),
            })
        }
        PaneChatModelSettingChange::Reset => {
            let model = current_model.filter(|model| !model.effort_levels.is_empty())?;
            // Codex는 모델만 다시 고르면 그 모델의 기본 effort로 돌아간다
            Some(if is_claude {
                "/effort auto".to_string()
            } else {
                format!("/model {}", model.id)
            })
        }
    }
}

#[cfg(test)]
mod tests;
