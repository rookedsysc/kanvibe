//! pane 하나의 대화 기록에 매기는 순번. CLI 기록에는 메시지 ID가 없어서, 구독자가 "어디까지 받았는지"를
//! 말할 수 있도록 데스크탑이 순번을 매긴다. epoch은 이 순번이 유효한 범위다.

use serde::{Deserialize, Serialize};

use crate::types::AggregatedAiMessage;

/// 순번이 붙은 대화 한 줄. 같은 epoch 안에서 seq는 1씩 늘고 다시 쓰이지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatTimelineRow {
    pub seq: u64,
    pub message: AggregatedAiMessage,
}

/// 기록이 다시 쓰이거나 다른 세션으로 바뀌면 새 epoch으로 처음부터 센다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatTimeline {
    pub epoch: String,
    pub session_id: Option<String>,
    /// 오래된 것부터. 조회 창에 들어 있는 줄만 가진다
    pub rows: Vec<PaneChatTimelineRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatTimelineAdvance {
    pub timeline: PaneChatTimeline,
    /// 새로 붙었거나 내용이 자란 줄. 오래된 것부터
    pub changed_rows: Vec<PaneChatTimelineRow>,
    /// 이전 순번을 이어 갈 수 없어 새 epoch을 시작했는지
    pub is_replaced: bool,
}

/// 구독자가 대화를 어디까지 받았는지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatCursor {
    pub epoch: String,
    pub seq: u64,
}

/// CLI가 마지막 답변을 이어 쓴 결과인지. 같은 사람이 쓴 글이 앞부분을 그대로 둔 채 길어진 경우만 인정한다
fn is_grown_message(previous: &AggregatedAiMessage, current: &AggregatedAiMessage) -> bool {
    previous.role == current.role && current.full_text.starts_with(&previous.full_text)
}

/// 이전 줄들이 새 조회 창의 어디에서부터 이어지는지 찾는다.
///
/// 조회 창은 최근 줄만 담으므로 오래된 줄이 창 밖으로 밀려나는 것은 어긋남이 아니다. 그래서 이전 줄을 앞에서부터
/// 하나씩 버려 가며, 남은 줄이 새 창의 앞부분과 같아지는 지점을 찾는다. 맨 끝 줄 하나는 글이 길어진 경우도 같은 줄로 본다
fn find_continuation_start(
    previous_rows: &[PaneChatTimelineRow],
    messages: &[AggregatedAiMessage],
) -> Option<usize> {
    (0..previous_rows.len()).find(|dropped_count| {
        let kept_rows = &previous_rows[*dropped_count..];
        if kept_rows.len() > messages.len() {
            return false;
        }
        let last_kept_index = kept_rows.len() - 1;
        kept_rows.iter().enumerate().all(|(index, row)| {
            row.message == messages[index]
                || (index == last_kept_index && is_grown_message(&row.message, &messages[index]))
        })
    })
}

fn numbered_rows(messages: &[AggregatedAiMessage]) -> Vec<PaneChatTimelineRow> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| PaneChatTimelineRow {
            seq: index as u64 + 1,
            message: message.clone(),
        })
        .collect()
}

fn start_timeline(
    session_id: Option<&str>,
    messages: &[AggregatedAiMessage],
    is_replaced: bool,
) -> PaneChatTimelineAdvance {
    let rows = numbered_rows(messages);
    PaneChatTimelineAdvance {
        timeline: PaneChatTimeline {
            epoch: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.map(str::to_string),
            rows: rows.clone(),
        },
        changed_rows: rows,
        is_replaced,
    }
}

/// 새로 읽은 대화 기록(오래된 것부터)을 이전 상태와 맞춰 순번을 이어 매긴다
pub fn advance_pane_chat_timeline(
    previous: Option<&PaneChatTimeline>,
    session_id: Option<&str>,
    messages: &[AggregatedAiMessage],
) -> PaneChatTimelineAdvance {
    let Some(previous) = previous else {
        return start_timeline(session_id, messages, false);
    };
    if previous.session_id.as_deref() != session_id {
        return start_timeline(session_id, messages, true);
    }
    if previous.rows.is_empty() {
        let rows = numbered_rows(messages);
        return PaneChatTimelineAdvance {
            timeline: PaneChatTimeline {
                rows: rows.clone(),
                ..previous.clone()
            },
            changed_rows: rows,
            is_replaced: false,
        };
    }
    let Some(continuation_start) = find_continuation_start(&previous.rows, messages) else {
        return start_timeline(session_id, messages, true);
    };
    continue_timeline(previous, &previous.rows[continuation_start..], messages)
}

fn continue_timeline(
    previous: &PaneChatTimeline,
    kept_rows: &[PaneChatTimelineRow],
    messages: &[AggregatedAiMessage],
) -> PaneChatTimelineAdvance {
    let last_kept_index = kept_rows.len() - 1;
    let last_kept_row = &kept_rows[last_kept_index];
    let mut changed_rows = Vec::new();
    let mut rows = kept_rows[..last_kept_index].to_vec();
    if last_kept_row.message == messages[last_kept_index] {
        rows.push(last_kept_row.clone());
    } else {
        let grown_row = PaneChatTimelineRow {
            seq: last_kept_row.seq,
            message: messages[last_kept_index].clone(),
        };
        rows.push(grown_row.clone());
        changed_rows.push(grown_row);
    }
    for (index, message) in messages.iter().enumerate().skip(kept_rows.len()) {
        let appended_row = PaneChatTimelineRow {
            seq: last_kept_row.seq + (index - last_kept_index) as u64,
            message: message.clone(),
        };
        rows.push(appended_row.clone());
        changed_rows.push(appended_row);
    }
    PaneChatTimelineAdvance {
        timeline: PaneChatTimeline {
            rows,
            ..previous.clone()
        },
        changed_rows,
        is_replaced: false,
    }
}

/// 구독자가 가진 위치가 이 기록의 끝과 같은지. 다르면 구독자는 전체를 다시 받아야 한다
pub fn is_pane_chat_cursor_current(
    timeline: Option<&PaneChatTimeline>,
    cursor: Option<&PaneChatCursor>,
) -> bool {
    match (timeline, cursor) {
        (Some(timeline), Some(cursor)) => {
            timeline.epoch == cursor.epoch && read_pane_chat_timeline_seq(timeline) == cursor.seq
        }
        _ => false,
    }
}

/// 마지막 줄의 순번. 줄이 없으면 0이다
pub fn read_pane_chat_timeline_seq(timeline: &PaneChatTimeline) -> u64 {
    timeline.rows.last().map_or(0, |row| row.seq)
}

#[cfg(test)]
mod tests;
