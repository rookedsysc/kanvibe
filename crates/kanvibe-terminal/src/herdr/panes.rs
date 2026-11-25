//! Herdr 탭·pane 조회. 공유 서버에서는 그 task Space에 속하고 복원에 성공한 pane만 다룬다.

use std::sync::LazyLock;

use kanvibe_ai::AiSessionProvider;
use kanvibe_domain::terminal_tabs::TerminalTab;
use regex::Regex;
use serde::Deserialize;

use super::response::{
    HerdrLayoutResult, HerdrPane, HerdrPaneList, HerdrTabLayout, HerdrTabList, is_positive_integer,
};
use super::{HerdrError, HerdrService};
use crate::pane_mirror::MirrorPane;

static AGENT_SESSION_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9._:-]{1,200}$").expect("유효한 정규식"));

/// pane에서 도는 에이전트의 대화. Herdr가 알려 준 값 가운데 형식이 안전한 것만 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrAgentSession {
    pub provider: AiSessionProvider,
    pub session_id: String,
}

/// task가 쓸 수 있는 pane과 그 pane이 속한 탭
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrTaskPaneTab {
    pub pane_id: String,
    pub tab_id: String,
    pub agent_session: Option<HerdrAgentSession>,
}

/// TS `listHerdrMirrorPanes`의 options. 기본값은 모든 탭의 좌표와 실행 명령을 읽는다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrMirrorPaneOptions {
    /// TS `metadata: "minimal"`. 지정한 탭(없으면 첫 탭)의 좌표만 읽고 실행 명령은 비운다
    pub is_minimal: bool,
    pub tab_id: Option<String>,
    pub selected_pane_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HerdrPaneSize {
    pub cols: i64,
    pub rows: i64,
}

/// 지난 출력 줄 수와 화면 높이. 모바일 화면을 다시 받아야 하는지 판단할 때 쓴다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct HerdrPaneExtent {
    #[serde(rename = "max_offset_from_bottom")]
    pub history_rows: i64,
    pub viewport_rows: i64,
}

/// task의 Space에 속하고 복원에 성공한 pane과 탭 목록
struct EligiblePanes {
    panes: Vec<HerdrPane>,
    tabs: Vec<TerminalTab>,
}

#[derive(Deserialize)]
struct SelectedPane {
    pane: Option<HerdrPane>,
}

#[derive(Deserialize)]
struct PaneExtentScroll {
    scroll: HerdrPaneExtent,
}

#[derive(Deserialize)]
struct PaneExtentResult {
    pane: PaneExtentScroll,
}

#[derive(Default, Deserialize)]
struct ForegroundProcess {
    name: Option<String>,
}

#[derive(Deserialize)]
struct ProcessDetails {
    foreground_processes: Option<Vec<ForegroundProcess>>,
}

#[derive(Deserialize)]
struct ProcessDetailsResult {
    process_info: ProcessDetails,
}

fn agent_session_of(pane: &HerdrPane) -> Option<HerdrAgentSession> {
    let provider = match pane.agent.as_deref()? {
        "claude" => AiSessionProvider::Claude,
        "codex" => AiSessionProvider::Codex,
        "gemini" => AiSessionProvider::Gemini,
        "opencode" => AiSessionProvider::Opencode,
        _ => return None,
    };
    let session_id = pane.agent_session_id.as_ref()?.as_str()?;
    AGENT_SESSION_ID_PATTERN
        .is_match(session_id)
        .then(|| HerdrAgentSession {
            provider,
            session_id: session_id.to_string(),
        })
}

/// 좌표를 읽을 탭. 선택한 pane이 있으면 그 pane의 탭, 아니면 지정한 탭이나 쓸 수 있는 pane이 있는 첫 탭이다
fn geometry_tab_id(eligible: &EligiblePanes, options: &HerdrMirrorPaneOptions) -> Option<String> {
    if let Some(selected_pane_id) = &options.selected_pane_id {
        return eligible
            .panes
            .iter()
            .find(|pane| &pane.pane_id == selected_pane_id)
            .and_then(|pane| pane.tab_id.clone());
    }
    if let Some(tab_id) = &options.tab_id {
        return Some(tab_id.clone());
    }
    eligible
        .tabs
        .iter()
        .find(|tab| eligible.panes.iter().any(|pane| pane.is_in_tab(&tab.id)))
        .map(|tab| tab.id.clone())
}

impl HerdrService {
    /// TS `readHerdrTabs`. 탭 목록은 주기적으로 조회하므로 herdr 서버가 꺼져 있을 때의 실패를 매번 오류 로그로 남기지 않는다
    pub fn read_herdr_tabs(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<TerminalTab>, HerdrError> {
        let result = self.run_command(session_name, &["tab", "list"], ssh_host, false)?;
        let HerdrTabList { tabs } = super::response::decode(result)?;
        let terminal_tabs = tabs
            .into_iter()
            .enumerate()
            .map(|(index, tab)| TerminalTab {
                id: tab.tab_id,
                native_index: index as i64,
                name: tab.label,
                is_active: tab.focused,
            })
            .collect();
        Ok(terminal_tabs)
    }

    /// task의 Space에 속하고 복원에 성공한 pane과 탭 목록을 읽는다. 배치와 실행 명령은 읽지 않는다
    fn read_eligible_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<EligiblePanes, HerdrError> {
        let (panes, tabs) = std::thread::scope(|scope| {
            let tabs = scope.spawn(|| self.read_herdr_tabs(session_name, ssh_host));
            let panes = self.query::<HerdrPaneList>(session_name, &["pane", "list"], ssh_host);
            (panes, tabs.join().expect("탭 목록 조회 스레드"))
        });
        let HerdrPaneList { panes } = panes?;
        let tabs = tabs?;
        let workspace_prefix = self
            .get_herdr_workspace_id(session_name, ssh_host)?
            .map(|workspace_id| format!("{workspace_id}:"));
        let eligible_panes = panes
            .into_iter()
            .filter(|pane| !pane.is_unrestorable())
            .filter(|pane| {
                workspace_prefix
                    .as_ref()
                    .is_none_or(|prefix| pane.pane_id.starts_with(prefix))
            })
            .filter(|pane| tabs.iter().any(|tab| pane.is_in_tab(&tab.id)))
            .collect();
        Ok(EligiblePanes {
            panes: eligible_panes,
            tabs,
        })
    }

    /// TS `listHerdrTaskPaneTabIds`. task가 쓸 수 있는 pane과 그 pane이 속한 탭만 읽는다.
    /// pane이 있는지, 어느 탭에 있는지만 필요한 호출자가 [`Self::list_herdr_mirror_panes`]를 쓰면 탭마다 배치를, pane마다 실행 명령을
    /// 따로 읽어 원격 명령이 탭과 pane 수만큼 늘어난다.
    pub fn list_herdr_task_pane_tab_ids(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrTaskPaneTab>, HerdrError> {
        let eligible = self.read_eligible_panes(session_name, ssh_host)?;
        let pane_tabs = eligible
            .panes
            .iter()
            .map(|pane| HerdrTaskPaneTab {
                pane_id: pane.pane_id.clone(),
                tab_id: pane.tab_id.clone().unwrap_or_default(),
                agent_session: agent_session_of(pane),
            })
            .collect();
        Ok(pane_tabs)
    }

    /// TS `listHerdrMirrorPanes`. 모바일이 비출 pane을 탭 순서대로 모은다.
    /// 최소 목록은 지정한 탭(없으면 첫 탭)의 좌표만 읽고 실행 명령은 비운다. 다른 탭의 너비 0은 아직 좌표를 읽지 않았다는 뜻이다.
    pub fn list_herdr_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
        options: &HerdrMirrorPaneOptions,
    ) -> Result<Vec<MirrorPane>, HerdrError> {
        let eligible = self.read_eligible_panes(session_name, ssh_host)?;
        let geometry_tab_id = geometry_tab_id(&eligible, options);
        let is_target_requested = options.selected_pane_id.is_some() || options.tab_id.is_some();
        let has_geometry_tab_pane = eligible
            .panes
            .iter()
            .any(|pane| pane.tab_id.is_some() && pane.tab_id == geometry_tab_id);
        if is_target_requested && !has_geometry_tab_pane {
            return Ok(Vec::new());
        }
        let panes_by_tab: Vec<Result<Vec<MirrorPane>, HerdrError>> = std::thread::scope(|scope| {
            let readers: Vec<_> = eligible
                .tabs
                .iter()
                .map(|tab| {
                    let geometry_tab_id = geometry_tab_id.as_deref();
                    let eligible = &eligible;
                    scope.spawn(move || {
                        self.read_tab_mirror_panes(
                            session_name,
                            ssh_host,
                            tab,
                            eligible,
                            options,
                            geometry_tab_id,
                        )
                    })
                })
                .collect();
            readers
                .into_iter()
                .map(|reader| reader.join().expect("탭 pane 조회 스레드"))
                .collect()
        });
        let mut mirror_panes = Vec::new();
        for tab_panes in panes_by_tab {
            mirror_panes.extend(tab_panes?);
        }
        Ok(mirror_panes)
    }

    fn read_tab_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
        tab: &TerminalTab,
        eligible: &EligiblePanes,
        options: &HerdrMirrorPaneOptions,
        geometry_tab_id: Option<&str>,
    ) -> Result<Vec<MirrorPane>, HerdrError> {
        let tab_panes: Vec<&HerdrPane> = eligible
            .panes
            .iter()
            .filter(|pane| pane.is_in_tab(&tab.id))
            .collect();
        let Some(first_pane) = tab_panes.first() else {
            return Ok(Vec::new());
        };
        let should_skip_layout = options.is_minimal && Some(tab.id.as_str()) != geometry_tab_id;
        let layout = match should_skip_layout {
            true => None,
            false => {
                let arguments = ["pane", "layout", "--pane", first_pane.pane_id.as_str()];
                Some(
                    self.query::<HerdrLayoutResult>(session_name, &arguments, ssh_host)?
                        .layout,
                )
            }
        };
        std::thread::scope(|scope| {
            let readers: Vec<_> = tab_panes
                .iter()
                .map(|pane| {
                    let layout = layout.as_ref();
                    scope.spawn(move || {
                        self.build_mirror_pane(session_name, ssh_host, tab, pane, layout, options)
                    })
                })
                .collect();
            readers
                .into_iter()
                .map(|reader| reader.join().expect("pane 조회 스레드"))
                .collect()
        })
    }

    fn build_mirror_pane(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
        tab: &TerminalTab,
        pane: &HerdrPane,
        layout: Option<&HerdrTabLayout>,
        options: &HerdrMirrorPaneOptions,
    ) -> Result<MirrorPane, HerdrError> {
        let rect = layout.and_then(|layout| layout.pane_rect(&pane.pane_id));
        let width = match layout {
            Some(layout) if layout.is_zoomed_pane(&pane.pane_id) => layout.area.width,
            Some(layout) => rect.map_or(layout.area.width, |rect| rect.width),
            None => 0.0,
        };
        let viewport_rows = pane.viewport_rows();
        let is_geometry_valid = rect.is_some()
            && is_positive_integer(Some(width))
            && is_positive_integer(viewport_rows);
        if options.is_minimal && layout.is_some() && !is_geometry_valid {
            return Err(HerdrError::Message(
                "Herdr pane의 배치를 확인할 수 없습니다.".to_string(),
            ));
        }
        let command = match options.is_minimal {
            true => String::new(),
            false => self.read_herdr_pane_command(session_name, &pane.pane_id, ssh_host),
        };
        Ok(MirrorPane {
            id: pane.pane_id.clone(),
            tab_id: tab.id.clone(),
            tab_name: tab.name.clone(),
            command,
            left: rect.map_or(0, |rect| rect.x as i64),
            top: rect.map_or(0, |rect| rect.y as i64),
            width: width as i64,
            height: viewport_rows.unwrap_or_default() as i64,
        })
    }

    /// TS `readHerdrTaskPaneSize`. 선택한 pane과 그 탭 배치만 읽는다. 쓸 수 없는 pane이면 `None`이다
    pub fn read_herdr_task_pane_size(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<HerdrPaneSize>, HerdrError> {
        let SelectedPane { pane } =
            self.query(session_name, &["pane", "get", pane_id], ssh_host)?;
        let Some(pane) = pane.filter(|pane| !pane.is_unrestorable() && pane.pane_id == pane_id)
        else {
            return Ok(None);
        };
        let arguments = ["pane", "layout", "--pane", pane_id];
        let HerdrLayoutResult { layout } = self.query(session_name, &arguments, ssh_host)?;
        let Some(rect) = layout.pane_rect(pane_id) else {
            return Ok(None);
        };
        let cols = match layout.is_zoomed_pane(pane_id) {
            true => layout.area.width,
            false => rect.width,
        };
        let rows = pane.viewport_rows();
        if !is_positive_integer(Some(cols)) || !is_positive_integer(rows) {
            return Ok(None);
        }
        Ok(Some(HerdrPaneSize {
            cols: cols as i64,
            rows: rows.unwrap_or_default() as i64,
        }))
    }

    /// 실행 명령은 탭 이름 표시용이라 읽지 못해도 pane 목록을 막지 않는다
    fn read_herdr_pane_command(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> String {
        self.read_herdr_pane_foreground_programs(session_name, pane_id, ssh_host)
            .pop()
            .unwrap_or_default()
    }

    /// TS `readHerdrPaneForegroundPrograms`. pane 앞쪽에서 도는 프로그램 이름들. 읽지 못하면 아무것도 돌지 않는 것으로 본다
    pub fn read_herdr_pane_foreground_programs(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Vec<String> {
        let arguments = ["pane", "process-info", "--pane", pane_id];
        let Ok(ProcessDetailsResult { process_info }) =
            self.query(session_name, &arguments, ssh_host)
        else {
            return Vec::new();
        };
        process_info
            .foreground_processes
            .unwrap_or_default()
            .into_iter()
            .filter_map(|process| process.name.filter(|name| !name.is_empty()))
            .collect()
    }

    /// TS `readHerdrPaneExtent`
    pub fn read_herdr_pane_extent(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<HerdrPaneExtent, HerdrError> {
        let PaneExtentResult { pane } =
            self.query(session_name, &["pane", "get", pane_id], ssh_host)?;
        Ok(pane.scroll)
    }
}
