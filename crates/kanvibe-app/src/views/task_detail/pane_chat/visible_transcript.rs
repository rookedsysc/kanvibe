//! 지침 필터까지 거친 대화와 줄 열쇠를 입력이 바뀔 때만 다시 만든다(TS `usePaneChatTranscript`의 `useMemo`).
//! 같은 창의 터미널 출력 프레임마다 pane chat도 다시 그려지므로, render마다 기록 전체를 복제하고 해시하지 않게 한다.

use std::rc::Rc;

use kanvibe_ai::types::AggregatedAiMessage;

use super::super::chat_history::transcript_rows;

/// 보일 대화를 정하는 입력. 하나라도 바뀌면 대화를 다시 만든다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisibleTranscriptInputs {
    pub snapshot_revision: u64,
    pub pages_revision: u64,
    pub should_show_instructions: bool,
}

/// 대화 줄 하나. 열쇠는 이전 페이지가 붙어도 같은 메시지면 같다
pub(super) struct VisibleTranscriptRow {
    pub key: String,
    pub message: Rc<AggregatedAiMessage>,
}

/// 보일 대화. 최신순이다
pub(super) struct VisibleTranscript {
    pub newest_first: Vec<VisibleTranscriptRow>,
}

impl VisibleTranscript {
    pub(super) fn new(
        newest_first_messages: Vec<AggregatedAiMessage>,
        session_key: Option<&str>,
    ) -> Self {
        let mut newest_first_keys: Vec<String> =
            transcript_rows(&newest_first_messages, session_key)
                .into_iter()
                .map(|row| row.key)
                .collect();
        newest_first_keys.reverse();
        let newest_first = newest_first_messages
            .into_iter()
            .zip(newest_first_keys)
            .map(|(message, key)| VisibleTranscriptRow {
                key,
                message: Rc::new(message),
            })
            .collect();
        Self { newest_first }
    }
}

#[cfg(test)]
mod tests {
    use kanvibe_ai::types::AiMessageRole;

    use super::*;

    #[test]
    fn row_keys_match_the_chronological_transcript_rows() {
        let newest_first = vec![
            AggregatedAiMessage::plain(AiMessageRole::Assistant, None, "새 답"),
            AggregatedAiMessage::plain(AiMessageRole::User, None, "질문"),
        ];
        let expected: Vec<(String, String)> = transcript_rows(&newest_first, Some("session"))
            .into_iter()
            .map(|row| (row.key, row.message.text.clone()))
            .collect();

        let built = VisibleTranscript::new(newest_first, Some("session"));
        let chronological: Vec<(String, String)> = built
            .newest_first
            .iter()
            .rev()
            .map(|row| (row.key.clone(), row.message.text.clone()))
            .collect();
        assert_eq!(chronological, expected);
    }
}
