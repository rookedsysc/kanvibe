//! pane별 채팅 모드 기록(TS `paneChatModeStore.ts`). 모든 작업의 기록을 앱 설정 `pane_chat_modes` 한 칸에 JSON으로 둔다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kanvibe_db::Database;
use kanvibe_db::DatabaseError;
use kanvibe_db::repositories::app_settings::{find_app_setting, save_app_setting};
use serde_json::{Map, Value};

#[cfg(test)]
mod tests;

const PANE_CHAT_MODES_KEY: &str = "pane_chat_modes";
/// 삭제된 작업의 기록이 끝없이 쌓이지 않도록 최근에 바꾼 작업만 남긴다. 수는 pane 채팅 세션 바인딩의 보관 상한(`MAX_BINDINGS`)과 같다
const MAX_TASKS_WITH_MODES: usize = 256;

/// pane ID별 채팅 모드. true는 채팅 UI, false는 사용자가 직접 CLI로 되돌린 pane이다
pub type PaneChatModes = BTreeMap<String, bool>;

/// 작업 ID를 넣은 순서(가장 최근에 바꾼 작업이 맨 뒤)대로 담는다. 저장 JSON의 키 순서도 이 순서를 따른다
type ModesByTask = Map<String, Value>;

#[derive(Debug, thiserror::Error)]
pub enum PaneChatModeError {
    #[error("pane 채팅 모드 저장 요청 형식이 올바르지 않습니다.")]
    InvalidRequest,
    #[error(transparent)]
    Database(#[from] DatabaseError),
}

/// 앱이 하나를 만들어 데스크톱 화면과 모바일 명령이 함께 쓴다
pub struct PaneChatModeStore {
    database: Arc<Database>,
    /// 읽기-고치기-쓰기를 한 줄로 세우는 잠금(TS `writeQueue`)
    write_lock: Mutex<()>,
}

/// 저장값이 깨졌거나 형식이 다르면 그 부분만 버리고 읽을 수 있는 기록은 살린다
fn parse_modes_by_task(serialized: Option<&str>) -> ModesByTask {
    let mut modes_by_task = ModesByTask::new();
    let Some(serialized) = serialized.filter(|value| !value.is_empty()) else {
        return modes_by_task;
    };
    let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(serialized) else {
        return modes_by_task;
    };
    for (task_id, modes) in parsed {
        let Value::Object(modes) = modes else {
            continue;
        };
        let valid_modes: Map<String, Value> = modes
            .into_iter()
            .filter(|(_, is_enabled)| is_enabled.is_boolean())
            .collect();
        modes_by_task.insert(task_id, Value::Object(valid_modes));
    }
    modes_by_task
}

/// 다시 넣어 가장 최근에 바꾼 작업이 맨 뒤에 오게 하고, 상한을 넘은 오래된 작업부터 버린다
fn record_pane_mode(
    modes_by_task: &mut ModesByTask,
    task_id: &str,
    pane_id: &str,
    is_enabled: bool,
) {
    let mut task_modes = match modes_by_task.shift_remove(task_id) {
        Some(Value::Object(modes)) => modes,
        _ => Map::new(),
    };
    task_modes.insert(pane_id.to_string(), Value::Bool(is_enabled));
    modes_by_task.insert(task_id.to_string(), Value::Object(task_modes));
    while modes_by_task.len() > MAX_TASKS_WITH_MODES {
        let oldest_task_id = modes_by_task
            .keys()
            .next()
            .cloned()
            .expect("상한을 넘었으면 비어 있지 않다");
        modes_by_task.shift_remove(&oldest_task_id);
    }
}

impl PaneChatModeStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            write_lock: Mutex::new(()),
        }
    }

    fn read_all_pane_chat_modes(&self) -> Result<ModesByTask, DatabaseError> {
        let setting = find_app_setting(&self.database.connection(), PANE_CHAT_MODES_KEY)?;
        Ok(parse_modes_by_task(
            setting.as_ref().map(|setting| setting.value.as_str()),
        ))
    }

    pub fn get_pane_chat_modes(&self, task_id: &str) -> Result<PaneChatModes, DatabaseError> {
        let modes_by_task = self.read_all_pane_chat_modes()?;
        let Some(Value::Object(modes)) = modes_by_task.get(task_id) else {
            return Ok(PaneChatModes::new());
        };
        Ok(modes
            .iter()
            .filter_map(|(pane_id, is_enabled)| Some((pane_id.clone(), is_enabled.as_bool()?)))
            .collect())
    }

    pub fn set_pane_chat_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_enabled: bool,
    ) -> Result<(), PaneChatModeError> {
        if task_id.is_empty() || pane_id.is_empty() {
            return Err(PaneChatModeError::InvalidRequest);
        }
        // 모든 작업의 기록이 설정 한 칸을 공유하므로 데스크톱 화면과 모바일 명령이 같은 읽기-쓰기 순서를 지켜야 서로의 변경을 덮지 않는다.
        // 앞선 저장이 실패해 잠금이 오염돼도 뒤따르는 저장은 막지 않는다.
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut modes_by_task = self.read_all_pane_chat_modes()?;
        record_pane_mode(&mut modes_by_task, task_id, pane_id, is_enabled);
        let serialized = Value::Object(modes_by_task).to_string();
        save_app_setting(
            &self.database.connection(),
            PANE_CHAT_MODES_KEY,
            &serialized,
        )?;
        Ok(())
    }
}
