//! 터미널 탭 이름·순서, pane 레이아웃, 원격 터미널 이미지 붙여넣기(TS `mobileWorkspaceService`).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutConfig, PaneLayoutType};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::host_file_access::quote_shell_argument;
use kanvibe_terminal::pane_layout_service::{
    SavePaneLayoutInput, delete_pane_layout, get_all_pane_layouts, save_pane_layout,
};
use serde_json::{Map, Value};

use super::MobileFeatures;
use super::validation::{
    read_mobile_boolean, read_mobile_enum, read_mobile_object, read_mobile_text,
    read_mobile_text_str, read_safe_integer,
};
use crate::error::{MobileError, MobileResult};

const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PANE_COMMAND_LENGTH: usize = 4096;
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn layout_pane_count(layout_type: PaneLayoutType) -> usize {
    match layout_type {
        PaneLayoutType::Single => 1,
        PaneLayoutType::Horizontal2 | PaneLayoutType::Vertical2 => 2,
        PaneLayoutType::LeftRightTb | PaneLayoutType::LeftTbRight => 3,
        PaneLayoutType::Quad => 4,
    }
}

/// pane은 배열 위치와 같은 `position`과 4096자 이하의 NUL 없는 명령만 받는다
fn read_pane_commands(
    value: Option<&Value>,
    expected_count: usize,
) -> MobileResult<Vec<PaneCommand>> {
    let Some(panes) = value
        .and_then(Value::as_array)
        .filter(|panes| panes.len() == expected_count)
    else {
        return Err(MobileError::feature(
            400,
            "레이아웃의 pane 수가 맞지 않습니다",
        ));
    };
    panes
        .iter()
        .enumerate()
        .map(|(index, pane)| read_pane_command(pane, index))
        .collect()
}

fn read_pane_command(value: &Value, index: usize) -> MobileResult<PaneCommand> {
    let pane = read_mobile_object(value)?;
    let has_position = pane
        .get("position")
        .and_then(Value::as_f64)
        .is_some_and(|position| position == index as f64);
    let command = pane
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| {
            kanvibe_ai::javascript::utf16_length(command) <= MAX_PANE_COMMAND_LENGTH
                && !command.contains('\0')
        });
    match (has_position, command) {
        (true, Some(command)) => Ok(PaneCommand {
            position: index as i64,
            command: command.to_string(),
        }),
        _ => Err(MobileError::feature(400, "pane 명령이 올바르지 않습니다")),
    }
}

fn read_pane_layout_input(body: &Map<String, Value>) -> MobileResult<SavePaneLayoutInput> {
    let layout_type = read_mobile_enum(PaneLayoutType::parse, body.get("layoutType"), "레이아웃")?;
    let is_global = read_mobile_boolean(body.get("isGlobal"), "글로벌 설정")?;
    let project_id = if is_global {
        None
    } else {
        Some(read_mobile_text(body.get("projectId"), "프로젝트", 128)?)
    };
    let has_project = body.get("projectId").is_some_and(|value| !value.is_null());
    if is_global && has_project {
        return Err(MobileError::feature(
            400,
            "글로벌 설정에는 프로젝트를 지정할 수 없습니다",
        ));
    }
    let panes = read_pane_commands(body.get("panes"), layout_pane_count(layout_type))?;
    Ok(SavePaneLayoutInput {
        layout_type,
        panes,
        project_id,
        is_global,
    })
}

/// base64는 표준 알파벳·4의 배수 길이·크기 상한 안이어야 하고, 풀어낸 바이트가 선언한 형식의 머리와 맞아야 한다
fn read_pasted_image(body: &Map<String, Value>) -> MobileResult<(&'static str, String)> {
    let mime_type = read_mobile_enum(
        |value| {
            ["image/png", "image/jpeg"]
                .into_iter()
                .find(|mime| *mime == value)
        },
        body.get("mimeType"),
        "이미지 형식",
    )?;
    let max_encoded_length = MAX_IMAGE_BYTES.div_ceil(3) * 4;
    let Some(encoded) = body
        .get("base64")
        .and_then(Value::as_str)
        .filter(|encoded| {
            encoded.len() <= max_encoded_length && encoded.len() % 4 == 0 && is_base64_text(encoded)
        })
    else {
        return Err(MobileError::feature(
            400,
            "이미지가 올바르지 않거나 너무 큽니다",
        ));
    };
    let image = STANDARD.decode(encoded).unwrap_or_default();
    let has_signature = if mime_type == "image/png" {
        image.starts_with(&PNG_SIGNATURE)
    } else {
        image.starts_with(&[255, 216, 255])
    };
    let is_canonical = STANDARD.encode(&image) == encoded;
    if image.is_empty() || !is_canonical || image.len() > MAX_IMAGE_BYTES || !has_signature {
        return Err(MobileError::feature(
            400,
            "이미지 내용과 형식이 맞지 않습니다",
        ));
    }
    Ok((mime_type, encoded.to_string()))
}

/// JS `/^[A-Za-z0-9+/]*={0,2}$/`
fn is_base64_text(encoded: &str) -> bool {
    let body = encoded.trim_end_matches('=');
    encoded.len() - body.len() <= 2
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
}

impl MobileFeatures {
    fn require_mobile_tab(&self, task_id: &str, tab_id: &str) -> MobileResult<Vec<TerminalTab>> {
        self.require_mobile_task(task_id)?;
        let listing = self.tabs.list_terminal_tabs(task_id)?;
        if !listing.is_ok {
            return Err(MobileError::feature(409, "터미널 탭을 조회하지 못했습니다"));
        }
        if !listing.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(MobileError::feature(404, "없는 터미널 탭입니다"));
        }
        Ok(listing.tabs)
    }

    fn refreshed_tabs(&self, task_id: &str) -> MobileResult<Vec<TerminalTab>> {
        let refreshed = self.tabs.list_terminal_tabs(task_id)?;
        if !refreshed.is_ok {
            return Err(MobileError::feature(
                409,
                "최신 탭 목록을 조회하지 못했습니다",
            ));
        }
        Ok(refreshed.tabs)
    }

    pub fn rename_mobile_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        input: &Value,
    ) -> MobileResult<Vec<TerminalTab>> {
        let name = read_mobile_text(read_mobile_object(input)?.get("name"), "탭 이름", 128)?;
        self.require_mobile_tab(task_id, tab_id)?;
        if !self.tabs.rename_terminal_tab(task_id, tab_id, &name)? {
            return Err(MobileError::feature(409, "탭 이름을 바꾸지 못했습니다"));
        }
        self.refreshed_tabs(task_id)
    }

    pub fn move_mobile_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        input: &Value,
    ) -> MobileResult<Vec<TerminalTab>> {
        let index = read_safe_integer(read_mobile_object(input)?.get("index"))
            .filter(|index| *index >= 0)
            .ok_or_else(|| MobileError::feature(400, "탭 위치가 올바르지 않습니다"))?;
        let tabs = self.require_mobile_tab(task_id, tab_id)?;
        if index as usize >= tabs.len() {
            return Err(MobileError::feature(400, "탭 위치가 목록을 벗어났습니다"));
        }
        if !self.tabs.move_terminal_tab(task_id, tab_id, index)? {
            return Err(MobileError::feature(409, "탭을 옮기지 못했습니다"));
        }
        self.refreshed_tabs(task_id)
    }

    pub fn get_mobile_pane_layouts(&self) -> MobileResult<Vec<PaneLayoutConfig>> {
        Ok(get_all_pane_layouts(&self.database)?)
    }

    pub fn save_mobile_pane_layout(&self, input: &Value) -> MobileResult<PaneLayoutConfig> {
        let layout = read_pane_layout_input(read_mobile_object(input)?)?;
        if let Some(project_id) = layout.project_id.as_deref() {
            self.require_mobile_project(project_id)?;
        }
        Ok(save_pane_layout(&self.database, layout)?)
    }

    pub fn delete_mobile_pane_layout(&self, layout_id: &str) -> MobileResult<()> {
        let layout_id = read_mobile_text_str(layout_id, "레이아웃", 128)?;
        if !delete_pane_layout(&self.database, &layout_id)? {
            return Err(MobileError::feature(404, "없는 레이아웃입니다"));
        }
        Ok(())
    }

    /// 원격 터미널 pane에만 이미지를 올리고, 올린 경로를 셸 인자로 감싸 그 pane에 입력한다
    pub fn paste_mobile_remote_image(
        &self,
        task_id: &str,
        pane_id: &str,
        input: &Value,
    ) -> MobileResult<String> {
        let (mime_type, encoded) = read_pasted_image(read_mobile_object(input)?)?;
        let task = self.require_mobile_task(task_id)?;
        if task.task.ssh_host.as_deref().is_none_or(str::is_empty) {
            return Err(MobileError::feature(
                409,
                "원격 터미널에서만 이미지를 붙여넣을 수 있습니다",
            ));
        }
        if !self.panes.has_task_pane(task_id, pane_id)? {
            return Err(MobileError::feature(404, "없는 터미널 pane입니다"));
        }
        let data_url = format!("data:{mime_type};base64,{encoded}");
        let remote_path = self
            .images
            .paste_image_to_remote_terminal(task_id, &data_url)
            .map_err(|_| MobileError::feature(409, "이미지를 전송하지 못했습니다"))?;
        self.panes
            .write_to_pane(task_id, pane_id, &quote_shell_argument(&remote_path))?;
        Ok(remote_path)
    }
}
