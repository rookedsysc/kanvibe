//! 한 worktree에서 지금 돌고 있는 AI 세션과 그 세션이 띄운 서브태스크를 모은다.
//!
//! 판정은 두 근거를 겹쳐 쓴다. tmux pane에 에이전트 프로세스가 붙어 있으면 입력을 기다리며 놀고 있어도 실행중이고,
//! pane이 보이지 않아도 세션 기록이 방금 갱신됐으면 실행중이다. 앞의 근거는 tmux 밖을 못 보고 뒤의 근거는 대기 상태를
//! 못 보기 때문에 둘 중 하나라도 성립하면 실행중으로 본다.

use crate::claude_sessions::read_claude_live_sessions;
use crate::codex_sessions::read_codex_live_sessions;
use crate::error::AiResult;
use crate::gemini_sessions::read_gemini_live_sessions;
use crate::javascript::parse_date_ms;
use crate::opencode_sessions::read_opencode_live_sessions;
use crate::providers::AiSessionProvider;
use crate::running_agent_panes::filter_panes_by_worktree;
use crate::session_listing::map_with_concurrency;
use crate::types::{
    AiSessionReaderContext, LiveAiSession, LiveAiSessionState, LiveAiSessionWindows,
    LiveProviderSnapshot, LiveSessionTerminalWindow, RunningAgentPane,
};

/// 이 시간 안에 기록이 갱신됐으면 세션이 일하고 있다고 본다
const RUNNING_WINDOW_MS: u64 = 90_000;

/// 이 시간 안에 움직인 세션까지만 유휴 상태로 보여 준다. 더 오래된 세션은 목록에서 뺀다
const RECENT_WINDOW_MS: u64 = 6 * 60 * 60 * 1000;

pub const LIVE_SESSION_WINDOWS: LiveAiSessionWindows = LiveAiSessionWindows {
    running_window_ms: RUNNING_WINDOW_MS,
    recent_window_ms: RECENT_WINDOW_MS,
};

/// 실시간 세션을 읽는 provider. 순서가 결과 순서다
const LIVE_PROVIDERS: [AiSessionProvider; 4] = [
    AiSessionProvider::Claude,
    AiSessionProvider::Codex,
    AiSessionProvider::Opencode,
    AiSessionProvider::Gemini,
];

/// 실행중 세션 목록. `running_panes`는 호출자가 [`crate::running_agent_panes::RunningAgentPaneSource`]로 미리 모은 pane이다
pub fn read_live_ai_sessions(
    context: &AiSessionReaderContext,
    running_panes: &[RunningAgentPane],
    now_ms: f64,
) -> Vec<LiveAiSession> {
    let panes = filter_panes_by_worktree(running_panes, context.worktree_path.as_deref());
    let snapshots_by_provider =
        map_with_concurrency(&LIVE_PROVIDERS, LIVE_PROVIDERS.len(), |provider, _| {
            read_snapshots_safely(*provider, context, now_ms)
        });
    let running_since = now_ms - RUNNING_WINDOW_MS as f64;

    LIVE_PROVIDERS
        .iter()
        .zip(snapshots_by_provider)
        .flat_map(|(provider, snapshots)| {
            let provider_panes: Vec<&RunningAgentPane> = panes
                .iter()
                .filter(|pane| pane.provider == *provider)
                .collect();
            to_provider_live_sessions(*provider, &snapshots, &provider_panes, running_since)
        })
        .collect()
}

/// 한 provider의 세션 기록과 pane을 최근 순으로 자리끼리 맞춘다. pane에는 세션 id가 없어 정확히 이을 근거가 없기 때문이다.
/// 한쪽이 더 많으면 남는 자리는 비워 둔다. 기록을 못 읽은 pane도, pane 밖에서 도는 세션도 목록에서 사라지면 안 된다
fn to_provider_live_sessions(
    provider: AiSessionProvider,
    snapshots: &[LiveProviderSnapshot],
    provider_panes: &[&RunningAgentPane],
    running_since: f64,
) -> Vec<LiveAiSession> {
    let session_count = snapshots.len().max(provider_panes.len());
    (0..session_count)
        .map(|index| {
            to_live_session(
                provider,
                snapshots.get(index),
                provider_panes.get(index).copied(),
                running_since,
            )
        })
        .collect()
}

fn to_live_session(
    provider: AiSessionProvider,
    snapshot: Option<&LiveProviderSnapshot>,
    pane: Option<&RunningAgentPane>,
    running_since: f64,
) -> LiveAiSession {
    let last_active_at = snapshot.and_then(|snapshot| snapshot.last_active_at.clone());
    let has_recent_activity = last_active_at
        .as_deref()
        .and_then(parse_date_ms)
        .is_some_and(|active_at| active_at >= running_since);
    LiveAiSession {
        provider,
        session_id: snapshot.and_then(|snapshot| snapshot.session_id.clone()),
        current_task: snapshot.and_then(|snapshot| snapshot.current_task.clone()),
        state: if pane.is_some() || has_recent_activity {
            LiveAiSessionState::Running
        } else {
            LiveAiSessionState::Idle
        },
        last_active_at,
        running_subtasks: snapshot
            .map(|snapshot| snapshot.running_subtasks.clone())
            .unwrap_or_default(),
        terminal_window: pane.map(|pane| LiveSessionTerminalWindow {
            session_name: pane.session_name.clone(),
            window_id: pane.window_id.clone(),
            window_name: pane.window_name.clone(),
        }),
    }
}

/// 리더 하나가 실패해도 나머지 provider는 보여 준다. 폴링으로 계속 부르는 조회라 원격 연결이 끊긴 순간을 오류로 띄우지 않는다
fn read_snapshots_safely(
    provider: AiSessionProvider,
    context: &AiSessionReaderContext,
    now_ms: f64,
) -> Vec<LiveProviderSnapshot> {
    let snapshots: AiResult<Vec<LiveProviderSnapshot>> = match provider {
        AiSessionProvider::Claude => {
            read_claude_live_sessions(context, LIVE_SESSION_WINDOWS, now_ms)
        }
        AiSessionProvider::Codex => read_codex_live_sessions(context, LIVE_SESSION_WINDOWS, now_ms),
        AiSessionProvider::Opencode => {
            read_opencode_live_sessions(context, LIVE_SESSION_WINDOWS, now_ms)
        }
        AiSessionProvider::Gemini => {
            read_gemini_live_sessions(context, LIVE_SESSION_WINDOWS, now_ms)
        }
        _ => Ok(Vec::new()),
    };
    snapshots.unwrap_or_default()
}

#[cfg(test)]
mod tests;
