//! 채팅이 덮을 pane들의 화면 좌표(TS `readPaneChatLayout`). 붙어 있는 클라이언트가 실제로 보여 주는 탭의 pane만 고른다.

use kanvibe_ai::pane_chat::snapshot::{PaneChatBounds, PaneChatLayout, PaneChatLayoutPane};
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::quote_for_posix_shell;
use serde::Deserialize;
use serde_json::Value;

use super::{PaneChatSurfaceError, PaneChatSurfaceReader, js_number};
use crate::pane_mirror::{
    MirrorPane, build_tmux_list_mirror_panes_command, build_zellij_read_viewport_command,
    parse_tmux_mirror_pane_list, parse_zellij_mirror_pane_list,
};
use crate::terminal_registry::HerdrSurfaceBounds;

fn unavailable() -> PaneChatLayout {
    PaneChatLayout {
        active_pane_id: None,
        panes: Vec::new(),
    }
}

/// 모든 값이 정수이고 시작점이 음수가 아니며 크기가 양수인 좌표만 돌려준다(TS `hasValidPaneBounds`)
fn valid_bounds(left: f64, top: f64, width: f64, height: f64) -> Option<PaneChatBounds> {
    let values = [left, top, width, height];
    let is_integer = values
        .iter()
        .all(|value| value.is_finite() && value.fract() == 0.0);
    let is_valid = is_integer && left >= 0.0 && top >= 0.0 && width > 0.0 && height > 0.0;
    is_valid.then_some(PaneChatBounds {
        left: left as i64,
        top: top as i64,
        width: width as i64,
        height: height as i64,
    })
}

fn json_number(entry: &Value, key: &str) -> f64 {
    entry.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

pub(super) fn read_pane_chat_layout(
    reader: &PaneChatSurfaceReader,
    task_id: &str,
    terminal_tab_id: Option<&str>,
) -> Result<PaneChatLayout, PaneChatSurfaceError> {
    let Some(surface) = reader.read_surface(task_id, terminal_tab_id, false, false)? else {
        return Ok(unavailable());
    };
    let task = &surface.task;
    let pane_id = surface.pane_id.clone();
    let panes = match task.session_type {
        Some(SessionType::Terminal) => {
            return Ok(PaneChatLayout {
                active_pane_id: Some(pane_id.clone()),
                panes: vec![PaneChatLayoutPane {
                    id: pane_id.clone(),
                    tab_id: pane_id,
                    bounds: None,
                }],
            });
        }
        Some(SessionType::Tmux) => read_tmux_panes(reader, &surface)?,
        Some(SessionType::Zellij) => read_zellij_panes(reader, &surface)?,
        Some(SessionType::Herdr) => read_herdr_panes(reader, &surface)?,
        None => None,
    };
    let panes: Vec<PaneChatLayoutPane> = panes
        .unwrap_or_default()
        .into_iter()
        .filter(|pane| pane.bounds.is_some())
        .collect();
    if !panes.iter().any(|pane| pane.id == pane_id) {
        return Ok(unavailable());
    }
    Ok(PaneChatLayout {
        active_pane_id: Some(pane_id),
        panes,
    })
}

/// 탭 안 pane 목록. `None`이면 덮을 곳이 없다
type LayoutPanes = Option<Vec<PaneChatLayoutPane>>;

fn read_tmux_panes(
    reader: &PaneChatSurfaceReader,
    surface: &super::PaneChatSurface,
) -> Result<LayoutPanes, PaneChatSurfaceError> {
    let task = &surface.task;
    let output = reader.run(
        task,
        &build_tmux_list_mirror_panes_command(task.session_name.as_deref().unwrap_or_default()),
    )?;
    let format = "#{window_id}\t#{window_zoomed_flag}\t#{window_width}\t#{window_height}\t#{status-position}\t#{status}";
    let viewport = reader.run(
        task,
        &format!(
            "tmux display-message -p -t {} {}",
            quote_for_posix_shell(&surface.pane_id),
            quote_for_posix_shell(format)
        ),
    )?;
    let Some(window) = TmuxWindowViewport::parse(&viewport) else {
        return Ok(None);
    };
    let window_panes: Vec<MirrorPane> = parse_tmux_mirror_pane_list(&output)
        .into_iter()
        .filter(|pane| window.tab_id.as_deref() == Some(pane.tab_id.as_str()))
        .collect();
    if !window_panes.iter().any(|pane| pane.id == surface.pane_id) {
        return Ok(None);
    }
    Ok(Some(window.project(window_panes, &surface.pane_id)))
}

/// tmux pane이 있는 창의 확대 여부·크기와 상태 줄 위치
struct TmuxWindowViewport {
    tab_id: Option<String>,
    is_zoomed: bool,
    width: f64,
    height: f64,
    /// 상태 줄이 위에 있으면 그 줄 수만큼 pane이 아래로 밀린다
    top_offset: f64,
}

impl TmuxWindowViewport {
    /// 상태 줄 수를 정수로 읽지 못하면 `None`이다
    fn parse(viewport: &str) -> Option<Self> {
        let fields: Vec<&str> = kanvibe_ai::javascript::js_trim_end(viewport)
            .split('\t')
            .collect();
        let field = |index: usize| fields.get(index).copied();
        let status_rows = match field(5) {
            Some("on") => 1.0,
            Some("off") => 0.0,
            Some(status) => js_number(status),
            None => f64::NAN,
        };
        if !(status_rows.is_finite() && status_rows.fract() == 0.0) || status_rows < 0.0 {
            return None;
        }
        Some(Self {
            tab_id: field(0).map(str::to_string),
            is_zoomed: field(1) == Some("1"),
            width: field(2).map_or(f64::NAN, js_number),
            height: field(3).map_or(f64::NAN, js_number),
            top_offset: if field(4) == Some("top") {
                status_rows
            } else {
                0.0
            },
        })
    }

    /// 확대된 창은 선택한 pane 하나가 창 전체를 덮는다
    fn project(&self, window_panes: Vec<MirrorPane>, pane_id: &str) -> Vec<PaneChatLayoutPane> {
        let tab_id = self.tab_id.clone().unwrap_or_default();
        window_panes
            .into_iter()
            .filter(|pane| !self.is_zoomed || pane.id == pane_id)
            .map(|pane| PaneChatLayoutPane {
                bounds: if self.is_zoomed {
                    valid_bounds(0.0, self.top_offset, self.width, self.height)
                } else {
                    valid_bounds(
                        pane.left as f64,
                        pane.top as f64 + self.top_offset,
                        pane.width as f64,
                        pane.height as f64,
                    )
                },
                id: pane.id,
                tab_id: tab_id.clone(),
            })
            .collect()
    }
}

/// 활성 탭에서 지금 보이는 층의 pane만 남긴다. 떠 있는 pane 아래의 채팅이 팝업을 가리지 않도록 최상위 레이어만 노출한다
fn visible_zellij_entries(entries: &[Value], tab: &Value) -> Vec<Value> {
    let is_true = |entry: &Value, key: &str| entry.get(key) == Some(&Value::Bool(true));
    let is_floating_visible = is_true(tab, "are_floating_panes_visible");
    let is_fullscreen_active = is_true(tab, "is_fullscreen_active");
    entries
        .iter()
        .filter(|entry| entry.is_object() && entry.get("tab_id") == tab.get("tab_id"))
        .filter(|entry| !is_true(entry, "is_suppressed"))
        .filter(|entry| is_true(entry, "is_floating") == is_floating_visible)
        .filter(|entry| !is_fullscreen_active || is_true(entry, "is_fullscreen"))
        .cloned()
        .collect()
}

/// pane 목록과 탭 목록을 함께 읽는다. 배열이 아닌 응답은 TS의 구조 분해처럼 실패로 본다
fn read_zellij_viewport(
    reader: &PaneChatSurfaceReader,
    surface: &super::PaneChatSurface,
) -> Result<Value, PaneChatSurfaceError> {
    let task = &surface.task;
    let command =
        build_zellij_read_viewport_command(task.session_name.as_deref().unwrap_or_default());
    let response: Value = serde_json::from_str(&reader.run(task, &command)?)?;
    if !response.is_array() {
        return Err(PaneChatSurfaceError(
            "Zellij 화면 배치를 읽지 못했습니다.".to_string(),
        ));
    }
    Ok(response)
}

fn read_zellij_panes(
    reader: &PaneChatSurfaceReader,
    surface: &super::PaneChatSurface,
) -> Result<LayoutPanes, PaneChatSurfaceError> {
    let response = read_zellij_viewport(reader, surface)?;
    let (Some(Value::Array(entries)), Some(Value::Array(tabs))) =
        (response.get(0), response.get(1))
    else {
        return Ok(None);
    };
    let Some(tab) = tabs
        .iter()
        .find(|entry| entry.get("active") == Some(&Value::Bool(true)))
    else {
        return Ok(None);
    };
    let visible = visible_zellij_entries(entries, tab);
    let frames = parse_zellij_mirror_pane_list(&Value::Array(visible.clone()).to_string());
    let panes = frames
        .into_iter()
        .map(|pane| PaneChatLayoutPane {
            bounds: zellij_pane_bounds(&pane, &visible),
            id: pane.id,
            tab_id: pane.tab_id,
        })
        .collect();
    Ok(Some(panes))
}

/// 테두리를 뺀 글자 영역이 유효하면 그 영역을, 아니면 테두리를 포함한 pane 영역을 쓴다
fn zellij_pane_bounds(pane: &MirrorPane, visible: &[Value]) -> Option<PaneChatBounds> {
    let entry = visible.iter().find(|candidate| {
        super::zellij_pane_id(candidate) == pane.id && super::is_zellij_terminal(candidate)
    });
    let content = entry.and_then(|entry| {
        valid_bounds(
            json_number(entry, "pane_content_x"),
            json_number(entry, "pane_content_y"),
            json_number(entry, "pane_content_columns"),
            json_number(entry, "pane_content_rows"),
        )
    });
    content.or_else(|| {
        valid_bounds(
            pane.left as f64,
            pane.top as f64,
            pane.width as f64,
            pane.height as f64,
        )
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct HerdrRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct HerdrLayoutPane {
    pane_id: String,
    rect: HerdrRect,
}

#[derive(Debug, Clone, Deserialize)]
struct HerdrLayout {
    area: HerdrRect,
    zoomed: bool,
    panes: Vec<HerdrLayoutPane>,
}

fn read_herdr_layout(
    reader: &PaneChatSurfaceReader,
    surface: &super::PaneChatSurface,
) -> Result<HerdrLayout, PaneChatSurfaceError> {
    let task = &surface.task;
    let response = reader.herdr.run_command(
        task.session_name.as_deref().unwrap_or_default(),
        &["pane", "layout", "--pane", &surface.pane_id],
        task.ssh_host(),
    )?;
    Ok(serde_json::from_value(
        response.get("layout").cloned().unwrap_or(Value::Null),
    )?)
}

fn read_herdr_panes(
    reader: &PaneChatSurfaceReader,
    surface: &super::PaneChatSurface,
) -> Result<LayoutPanes, PaneChatSurfaceError> {
    let task = &surface.task;
    let Some(surface_bounds) = reader.terminals.herdr_task_surface_bounds(&task.id) else {
        return Ok(None);
    };
    let layout = read_herdr_layout(reader, surface)?;
    // 클라이언트가 마지막으로 알린 크기와 다른 배치는 다른 화면 크기의 좌표라 투영하지 않는다
    if layout.area.width != f64::from(surface_bounds.width)
        || layout.area.height != f64::from(surface_bounds.height)
    {
        return Ok(None);
    }
    let owned = reader.herdr.list_task_pane_tab_ids(
        task.session_name.as_deref().unwrap_or_default(),
        task.ssh_host(),
    )?;
    let tab_id = owned
        .iter()
        .find(|pane| pane.pane_id == surface.pane_id)
        .map(|pane| pane.tab_id.clone())
        .filter(|tab_id| !tab_id.is_empty());
    let Some(tab_id) = tab_id else {
        return Ok(None);
    };
    let tab_panes: Vec<&HerdrLayoutPane> = layout
        .panes
        .iter()
        .filter(|pane| {
            owned
                .iter()
                .any(|entry| entry.pane_id == pane.pane_id && entry.tab_id == tab_id)
        })
        .filter(|pane| !layout.zoomed || pane.pane_id == surface.pane_id)
        .collect();
    let panes = tab_panes
        .into_iter()
        .map(|pane| PaneChatLayoutPane {
            id: pane.pane_id.clone(),
            tab_id: tab_id.clone(),
            bounds: project_herdr_rect(&layout, pane, surface_bounds),
        })
        .collect();
    Ok(Some(panes))
}

/// Herdr workspace 좌표를 클라이언트 화면의 탭 영역 좌표로 옮긴다. 확대된 pane은 영역 전체를 덮는다
fn project_herdr_rect(
    layout: &HerdrLayout,
    pane: &HerdrLayoutPane,
    surface_bounds: HerdrSurfaceBounds,
) -> Option<PaneChatBounds> {
    let rect = if layout.zoomed {
        layout.area
    } else {
        pane.rect
    };
    valid_bounds(
        f64::from(surface_bounds.left) + rect.x,
        f64::from(surface_bounds.top) + rect.y,
        rect.width,
        rect.height,
    )
}
