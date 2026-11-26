use super::*;

fn border() -> String {
    "─".repeat(40)
}

/// Claude Code 2.1.326의 입력란을 옮긴 것. 커서는 입력란의 마지막 줄에 있다
fn read(composer_rows: &[&str]) -> Option<String> {
    let mut lines = vec!["● earlier reply".to_string(), border()];
    lines.extend(composer_rows.iter().map(|row| row.to_string()));
    lines.push(border());
    lines.push("  ? for shortcuts".to_string());
    read_claude_composer_text(&lines, 1 + composer_rows.len())
}

#[test]
fn reads_single_row_composer() {
    assert_eq!(
        read(&["❯ Reply with exactly: OK"]).as_deref(),
        Some("Reply with exactly: OK")
    );
}

#[test]
fn joins_wrapped_composer_rows() {
    assert_eq!(
        read(&["❯ first line", "  second line is here", "  third"]).as_deref(),
        Some("first line\nsecond line is here\nthird")
    );
}

#[test]
fn keeps_blank_rows_inside_composer() {
    assert_eq!(
        read(&["❯\u{a0}first paragraph", "", "  second paragraph"]).as_deref(),
        Some("first paragraph\n\nsecond paragraph")
    );
    assert_eq!(
        read(&["❯\u{a0}ends with a newline", ""]).as_deref(),
        Some("ends with a newline")
    );
}

#[test]
fn restores_shell_mode_marker() {
    assert_eq!(read(&["! ls -la"]).as_deref(), Some("!ls -la"));
}

#[test]
fn refuses_rows_that_are_not_the_composer() {
    let border = border();
    let cases: [(Vec<&str>, usize); 3] = [
        (
            vec![
                "● reply",
                &border,
                " Do you want to proceed?",
                " ❯ 1. Yes",
                "   2. No",
            ],
            3,
        ),
        (
            vec![
                "❯ an earlier prompt shown in the conversation",
                "  continued",
                &border,
            ],
            1,
        ),
        (vec![&border, "❯ text", &border, "  ? for shortcuts"], 3),
    ];
    for (lines, cursor_row) in cases {
        assert_eq!(
            read_claude_composer_text(&lines, cursor_row),
            None,
            "{lines:?}"
        );
    }
}

#[test]
fn ignores_whitespace_and_wrapping_differences() {
    assert!(is_claude_composer_showing_paste(
        "word000 word001\nword002",
        "word000 word001 word002",
        0
    ));
}

#[test]
fn different_text_is_not_the_paste() {
    assert!(!is_claude_composer_showing_paste(
        "Reply with exactly: RACE",
        "Reply with exactly: OK",
        0
    ));
    assert!(!is_claude_composer_showing_paste(
        "",
        "Reply with exactly: OK",
        0
    ));
}

#[test]
fn folded_multi_line_marker_needs_matching_line_count() {
    let five_lines = "a\nb\nc\nd\ne";
    assert!(is_claude_composer_showing_paste(
        "[Pasted text #2 +4 lines]",
        five_lines,
        0
    ));
    assert!(!is_claude_composer_showing_paste(
        "[Pasted text #2 +9 lines]",
        five_lines,
        0
    ));
    assert!(!is_claude_composer_showing_paste(
        "[Pasted text #2 +1 lines]",
        "a\nb",
        0
    ));
}

#[test]
fn folded_marker_without_lines_needs_long_single_line() {
    assert!(is_claude_composer_showing_paste(
        "[Pasted text #5]",
        &"x".repeat(1_100),
        0
    ));
    assert!(!is_claude_composer_showing_paste(
        "[Pasted text #5]",
        "short text",
        0
    ));
}

#[test]
fn image_markers_must_match_image_count() {
    assert!(is_claude_composer_showing_paste(
        "[Image #7]describe this",
        "describe this",
        1
    ));
    assert!(is_claude_composer_showing_paste(
        "[Image #1][Image #2]",
        "",
        2
    ));
    assert!(!is_claude_composer_showing_paste(
        "describe this",
        "describe this",
        1
    ));
    assert!(!is_claude_composer_showing_paste(
        "[Image #7]describe this",
        "describe this",
        0
    ));
}

#[test]
fn shell_mode_text_matches() {
    assert!(is_claude_composer_showing_paste("!ls -la", "!ls -la", 0));
}
