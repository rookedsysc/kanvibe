use super::*;
use crate::types::AiMessageRole;

fn message(text: &str) -> AggregatedAiMessage {
    AggregatedAiMessage::plain(AiMessageRole::Assistant, None, text)
}

fn messages(texts: &[&str]) -> Vec<AggregatedAiMessage> {
    texts.iter().map(|text| message(text)).collect()
}

fn start(texts: &[&str]) -> PaneChatTimeline {
    advance_pane_chat_timeline(None, Some("session-a"), &messages(texts)).timeline
}

fn seqs(rows: &[PaneChatTimelineRow]) -> Vec<u64> {
    rows.iter().map(|row| row.seq).collect()
}

fn row(seq: u64, message: AggregatedAiMessage) -> PaneChatTimelineRow {
    PaneChatTimelineRow { seq, message }
}

#[test]
fn first_read_numbers_from_one_without_replacing() {
    let advance = advance_pane_chat_timeline(None, Some("session-a"), &messages(&["a", "b"]));
    assert_eq!(seqs(&advance.changed_rows), [1, 2]);
    assert!(!advance.is_replaced);
    assert_eq!(read_pane_chat_timeline_seq(&advance.timeline), 2);
}

#[test]
fn appended_rows_continue_numbering() {
    let timeline = start(&["a", "b", "c"]);
    let advance = advance_pane_chat_timeline(
        Some(&timeline),
        Some("session-a"),
        &messages(&["a", "b", "c", "d", "e"]),
    );
    assert_eq!(
        advance.changed_rows,
        [row(4, message("d")), row(5, message("e"))]
    );
    assert_eq!(advance.timeline.epoch, timeline.epoch);
    assert!(!advance.is_replaced);
}

#[test]
fn grown_last_row_keeps_its_number() {
    let timeline = start(&["a", "답변을"]);
    let advance = advance_pane_chat_timeline(
        Some(&timeline),
        Some("session-a"),
        &messages(&["a", "답변을 이어 씁니다"]),
    );
    assert_eq!(
        advance.changed_rows,
        [row(2, message("답변을 이어 씁니다"))]
    );
    assert_eq!(advance.timeline.epoch, timeline.epoch);
}

#[test]
fn rows_scrolled_out_of_window_are_not_divergence() {
    let timeline = start(&["a", "b", "c", "d"]);
    let advance = advance_pane_chat_timeline(
        Some(&timeline),
        Some("session-a"),
        &messages(&["c", "d", "e", "f"]),
    );
    assert_eq!(seqs(&advance.changed_rows), [5, 6]);
    assert_eq!(seqs(&advance.timeline.rows), [3, 4, 5, 6]);
    assert!(!advance.is_replaced);
}

#[test]
fn unchanged_record_emits_nothing() {
    let timeline = start(&["a", "b"]);
    let advance =
        advance_pane_chat_timeline(Some(&timeline), Some("session-a"), &messages(&["a", "b"]));
    assert!(advance.changed_rows.is_empty());
    assert_eq!(advance.timeline, timeline);
}

#[test]
fn diverged_records_start_new_epoch() {
    let cases: [(&str, Vec<&str>); 4] = [
        ("session-a", vec!["a", "달라짐", "c"]),
        ("session-a", vec!["a", "b", "전혀 다른 글"]),
        ("session-a", vec![]),
        ("session-b", vec!["a", "b", "c"]),
    ];
    for (session_id, texts) in cases {
        let timeline = start(&["a", "b", "c"]);
        let advance =
            advance_pane_chat_timeline(Some(&timeline), Some(session_id), &messages(&texts));
        assert!(advance.is_replaced);
        assert_ne!(advance.timeline.epoch, timeline.epoch);
        assert_eq!(
            seqs(&advance.timeline.rows),
            (1..=texts.len() as u64).collect::<Vec<_>>()
        );
    }
}

#[test]
fn repeated_sentence_gets_new_number() {
    let timeline = start(&["네"]);
    let advance =
        advance_pane_chat_timeline(Some(&timeline), Some("session-a"), &messages(&["네", "네"]));
    assert_eq!(advance.changed_rows, [row(2, message("네"))]);
}

#[test]
fn first_row_after_empty_record_keeps_epoch() {
    let timeline = start(&[]);
    let first = AggregatedAiMessage::plain(AiMessageRole::User, None, "첫 줄");
    let advance = advance_pane_chat_timeline(
        Some(&timeline),
        Some("session-a"),
        std::slice::from_ref(&first),
    );
    assert_eq!(advance.changed_rows, [row(1, first)]);
    assert_eq!(advance.timeline.epoch, timeline.epoch);
    assert!(!advance.is_replaced);
}

#[test]
fn judges_subscriber_cursor_freshness() {
    let timeline = start(&["a", "b", "c"]);
    let cursor = |epoch: &str, seq: u64| PaneChatCursor {
        epoch: epoch.to_string(),
        seq,
    };
    assert!(is_pane_chat_cursor_current(
        Some(&timeline),
        Some(&cursor(&timeline.epoch, 3))
    ));
    assert!(!is_pane_chat_cursor_current(
        Some(&timeline),
        Some(&cursor(&timeline.epoch, 2))
    ));
    assert!(!is_pane_chat_cursor_current(
        Some(&timeline),
        Some(&cursor("other", 3))
    ));
    assert!(!is_pane_chat_cursor_current(Some(&timeline), None));
}

#[test]
fn unread_pane_has_no_current_cursor() {
    let cursor = PaneChatCursor {
        epoch: "any".to_string(),
        seq: 0,
    };
    assert!(!is_pane_chat_cursor_current(None, Some(&cursor)));
}
