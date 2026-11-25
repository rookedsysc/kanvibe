use serde_json::json;

use super::*;

fn border() -> String {
    "─".repeat(40)
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| row.to_string()).collect()
}

/// Claude Code 2.1.326이 실제로 그린 화면을 옮긴 것. 커서는 강조된 선택지의 ❯ 위에 놓인다
fn single_question() -> Vec<String> {
    let border = border();
    lines(&[
        "❯ Ask me a question",
        &border,
        " ☐ Color",
        "",
        "Which color do you prefer?",
        "",
        "❯ 1. Red (warm)",
        "     Warm",
        "  2. Green (calm)",
        "     Calm",
        "  3. Blue (cool)",
        "     Cool",
        "  4. Type something.",
        &border,
        "  5. Chat about this",
        "",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ])
}

fn permission() -> Vec<String> {
    let border = border();
    let dashed = "╌".repeat(40);
    lines(&[
        &border,
        " Bash command",
        " Create empty probe file",
        &dashed,
        " touch kanvibe-probe.txt",
        &dashed,
        " Do you want to proceed?",
        " ❯ 1. Yes",
        "   2. Yes, and always allow access to /home/user/kanvibe__worktrees/fix-lo",
        "      t-of-issues/workspace from this project",
        "   3. No",
        "",
        " Esc to cancel · Tab to amend",
    ])
}

fn model_switch() -> Vec<String> {
    let top = "▔".repeat(40);
    lines(&[
        &top,
        "   Switch model?",
        "   Your next response will be slower and use more tokens",
        "   ❯ 1. Yes, switch to Opus 5.5",
        "     2. No, go back",
        "   Enter to confirm · Esc to cancel",
    ])
}

fn multi_select() -> Vec<String> {
    let border = border();
    lines(&[
        &border,
        "←  ☒ Size  ☐ Toppings  ✔ Submit  →",
        "",
        "Which toppings?",
        "",
        "❯ 1. [✔] Cheese",
        "         Cheese",
        "  2. [ ] Olives",
        "         Olives",
        "  3. [ ] Type something",
        "     Submit",
        &border,
        "  4. Chat about this",
    ])
}

fn review() -> Vec<String> {
    let border = border();
    lines(&[
        &border,
        "←  ☒ Size  ☒ Toppings  ✔ Submit  →",
        "",
        "Review your answers",
        "",
        " ● Which size?",
        "   → Medium",
        "",
        "Ready to submit your answers?",
        "",
        "❯ 1. Submit answers",
        "  2. Cancel",
    ])
}

fn option(
    number: u64,
    label: &str,
    description: &str,
    is_checked: bool,
    is_text_entry: bool,
) -> PaneChatPromptOption {
    PaneChatPromptOption {
        number,
        label: label.to_string(),
        description: description.to_string(),
        is_checked,
        is_text_entry,
    }
}

fn replace_rows(screen: &[String], replacements: &[(&str, &str)]) -> Vec<String> {
    screen
        .iter()
        .map(|line| {
            replacements
                .iter()
                .find(|(from, _)| line == from)
                .map_or_else(|| line.clone(), |(_, to)| to.to_string())
        })
        .collect()
}

// parity: TS-aiSessions-paneChatClaudePrompt-b4b54f
#[test]
fn reads_question_with_described_options_and_text_entry() {
    assert_eq!(
        read_claude_prompt(&single_question(), 6, 0, None),
        Some(PaneChatPromptScreen {
            kind: PaneChatPromptKind::Question,
            title: "Which color do you prefer?".to_string(),
            detail: "☐ Color".to_string(),
            steps_line: None,
            highlighted_number: 1,
            is_multi_select: false,
            is_text_entry_active: false,
            has_hidden_options: false,
            options: vec![
                option(1, "Red (warm)", "Warm", false, false),
                option(2, "Green (calm)", "Calm", false, false),
                option(3, "Blue (cool)", "Cool", false, false),
                option(4, "Type something.", "", false, true),
                option(5, "Chat about this", "", false, false),
            ],
        })
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-a0d1e0
#[test]
fn reads_permission_command_detail_and_wrapped_rest_as_description() {
    let prompt = read_claude_prompt(&permission(), 7, 1, None).unwrap();
    assert_eq!(prompt.kind, PaneChatPromptKind::Permission);
    assert_eq!(prompt.title, "Do you want to proceed?");
    assert_eq!(
        prompt.detail,
        "Bash command\nCreate empty probe file\n\ntouch kanvibe-probe.txt"
    );
    assert_eq!(prompt.highlighted_number, 1);
    let labels: Vec<&str> = prompt
        .options
        .iter()
        .map(|option| option.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Yes",
            "Yes, and always allow access to /home/user/kanvibe__worktrees/fix-lo",
            "No"
        ]
    );
    assert_eq!(
        prompt.options[1].description,
        "t-of-issues/workspace from this project"
    );
}

const HARD_WRAPPED: &str =
    "   2. Yes, and always allow access to /home/user/kanvibe__worktrees/fix-lo";

// parity: TS-aiSessions-paneChatClaudePrompt-f80e40
#[test]
fn joins_path_broken_inside_word_without_space() {
    let long_path =
        "/home/user/projects/kanvibe__worktrees/fix-lot-of-issues/qa-results/workspace-directory";
    let head = "   2. Yes, and always allow access to ";
    let wrap_column = 70;
    let split = wrap_column - head.len();
    let rows = [
        format!("{head}{}", &long_path[..split]),
        format!("      {} now", &long_path[split..]),
    ];
    let screen: Vec<String> = permission()
        .into_iter()
        .flat_map(|line| {
            if line == HARD_WRAPPED {
                rows.to_vec()
            } else if line.starts_with("      t-of-issues") {
                Vec::new()
            } else {
                vec![line]
            }
        })
        .collect();
    let prompt = read_claude_prompt(&screen, 7, 1, Some(wrap_column + 1)).unwrap();
    assert_eq!(
        prompt.options[1].label,
        format!("Yes, and always allow access to {long_path} now")
    );
    assert_eq!(prompt.options[1].description, "");
}

// parity: TS-aiSessions-paneChatClaudePrompt-f10455
#[test]
fn joins_word_wrapped_row_with_space() {
    let screen: Vec<String> = permission()
        .into_iter()
        .map(|line| {
            if line == HARD_WRAPPED {
                "   2. Yes, and do not ask again for commands that start with git".to_string()
            } else if line.starts_with("      t-of-issues") {
                "      tag in this workspace".to_string()
            } else {
                line
            }
        })
        .collect();
    let prompt = read_claude_prompt(&screen, 7, 1, Some(67)).unwrap();
    assert_eq!(
        prompt.options[1].label,
        "Yes, and do not ask again for commands that start with git tag in this workspace"
    );
    assert_eq!(prompt.options[1].description, "");
}

// parity: TS-aiSessions-paneChatClaudePrompt-9816a5
#[test]
fn short_option_keeps_description_row() {
    let prompt = read_claude_prompt(&single_question(), 6, 0, Some(120)).unwrap();
    assert_eq!(
        (
            prompt.options[0].label.as_str(),
            prompt.options[0].description.as_str()
        ),
        ("Red (warm)", "Warm")
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-5b10e9
#[test]
fn question_label_near_edge_keeps_description() {
    let near_edge = [
        "Which preset?",
        "",
        "❯ 1. Use the shared configuration preset with strict lint rules",
        "     recommended for new repositories",
        "  2. Minimal",
        "     smallest setup",
        "  3. Chat about this",
    ];
    let prompt = read_claude_prompt(&near_edge, 2, 0, Some(70)).unwrap();
    assert_eq!(
        prompt.options[0].label,
        "Use the shared configuration preset with strict lint rules"
    );
    assert_eq!(
        prompt.options[0].description,
        "recommended for new repositories"
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-b73152
#[test]
fn question_joins_wrapped_label_when_boundary_is_clear() {
    let screen = [
        "Which preset?",
        "",
        "❯ 1. Use the shared configuration preset from the",
        "     platform team",
        "     recommended",
        "  2. Minimal",
        "     smallest setup",
        "  3. Chat about this",
    ];
    let prompt = read_claude_prompt(&screen, 2, 0, Some(54)).unwrap();
    assert_eq!(
        prompt.options[0].label,
        "Use the shared configuration preset from the platform team"
    );
    assert_eq!(prompt.options[0].description, "recommended");
}

// parity: TS-aiSessions-paneChatClaudePrompt-267b93
#[test]
fn description_after_wrapped_label_stays_description() {
    let screen = [
        "Pick one?",
        "",
        "❯ 1. A fairly long option label that needs",
        "     two lines",
        "     its description",
        "  2. Short",
    ];
    let prompt = read_claude_prompt(&screen, 2, 0, Some(44)).unwrap();
    assert_eq!(
        prompt.options[0].label,
        "A fairly long option label that needs two lines"
    );
    assert_eq!(prompt.options[0].description, "its description");
}

// parity: TS-aiSessions-paneChatClaudePrompt-35d2d6
#[test]
fn confirmation_below_border_uses_whole_notice_as_title() {
    let prompt = read_claude_prompt(&model_switch(), 3, 3, None).unwrap();
    assert_eq!(prompt.kind, PaneChatPromptKind::Confirmation);
    assert_eq!(
        prompt.title,
        "Switch model?\nYour next response will be slower and use more tokens"
    );
    assert_eq!(prompt.detail, "");
    let options: Vec<(u64, &str)> = prompt
        .options
        .iter()
        .map(|option| (option.number, option.label.as_str()))
        .collect();
    assert_eq!(
        options,
        [(1, "Yes, switch to Opus 5.5"), (2, "No, go back")]
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-a56c52
#[test]
fn multi_select_reads_checks_and_steps_without_submit_row() {
    let prompt = read_claude_prompt(&multi_select(), 5, 0, None).unwrap();
    assert_eq!(
        prompt.steps_line.as_deref(),
        Some("←  ☒ Size  ☐ Toppings  ✔ Submit  →")
    );
    assert_eq!(prompt.detail, "");
    assert!(prompt.is_multi_select);
    assert_eq!(
        prompt.options,
        [
            option(1, "Cheese", "Cheese", true, false),
            option(2, "Olives", "Olives", false, false),
            option(3, "Type something", "", false, true),
            option(4, "Chat about this", "", false, false),
        ]
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-188204
#[test]
fn review_screen_reads_chosen_answers_as_detail() {
    let prompt = read_claude_prompt(&review(), 10, 0, None).unwrap();
    assert_eq!(prompt.kind, PaneChatPromptKind::Question);
    assert_eq!(prompt.title, "Ready to submit your answers?");
    assert_eq!(
        prompt.detail,
        "Review your answers\n\n ● Which size?\n   → Medium"
    );
    assert_eq!(
        prompt.steps_line.as_deref(),
        Some("←  ☒ Size  ☒ Toppings  ✔ Submit  →")
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-f7f50d
#[test]
fn chosen_single_option_drops_mark_and_is_checked() {
    let revisited = replace_rows(
        &single_question(),
        &[("  2. Green (calm)", "  2. Green (calm) ✔")],
    );
    let prompt = read_claude_prompt(&revisited, 6, 0, None).unwrap();
    assert_eq!(prompt.options[1].label, "Green (calm)");
    assert!(prompt.options[1].is_checked);
}

// parity: TS-aiSessions-paneChatClaudePrompt-47a045
#[test]
fn cursor_on_highlighted_text_means_text_entry() {
    let typing = replace_rows(
        &single_question(),
        &[
            ("❯ 1. Red (warm)", "  1. Red (warm)"),
            ("  4. Type something.", "❯ 4. Extra large"),
        ],
    );
    let prompt = read_claude_prompt(&typing, 12, 16, None).unwrap();
    assert_eq!(prompt.highlighted_number, 4);
    assert!(prompt.is_text_entry_active);
    assert!(
        prompt
            .options
            .contains(&option(4, "Extra large", "", false, true))
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-200d49
#[test]
fn would_you_like_to_is_permission() {
    let border = border();
    let screen = [
        border.as_str(),
        " Ready to code?",
        "",
        " Would you like to proceed?",
        "",
        " ❯ 1. Yes",
        "   2. No, keep planning",
    ];
    assert_eq!(
        read_claude_prompt(&screen, 5, 1, None).unwrap().kind,
        PaneChatPromptKind::Permission
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-fca029
#[test]
fn highlighted_multi_select_text_entry_is_active() {
    let typing = replace_rows(
        &multi_select(),
        &[
            ("❯ 1. [✔] Cheese", "  1. [✔] Cheese"),
            ("  3. [ ] Type something", "❯ 3. [ ] Type something"),
        ],
    );
    let prompt = read_claude_prompt(&typing, 9, 9, None).unwrap();
    assert_eq!(prompt.highlighted_number, 3);
    assert!(prompt.is_text_entry_active);
    assert!(prompt.is_multi_select);
}

// parity: TS-aiSessions-paneChatClaudePrompt-ae3b7f
#[test]
fn spaced_chosen_mark_after_typed_text_is_removed() {
    let with_text = |row: &str| replace_rows(&single_question(), &[("  4. Type something.", row)]);
    let while_typing = read_claude_prompt(&with_text("  4. term  ✔"), 6, 0, None)
        .unwrap()
        .options[3]
        .clone();
    let after_leaving = read_claude_prompt(&with_text("  4. term ✔"), 6, 0, None)
        .unwrap()
        .options[3]
        .clone();
    assert_eq!(while_typing, option(4, "term", "", true, false));
    assert_eq!(after_leaving, while_typing);
}

// parity: TS-aiSessions-paneChatClaudePrompt-445316
#[test]
fn chosen_mark_in_the_middle_is_checked() {
    let top = "▔".repeat(40);
    let model_list = [
        top.as_str(),
        "   Select model",
        "     1.  Default (recommended)  Opus",
        "   ❯ 2.  Sonnet 5.5 ✔           Most efficient",
        "     ▸   More models",
    ];
    assert_eq!(
        read_claude_prompt(&model_list, 3, 3, None).unwrap().options,
        [
            option(1, "Default (recommended)  Opus", "", false, false),
            option(2, "Sonnet 5.5           Most efficient", "", true, false),
        ]
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-2c2bf4
#[test]
fn scrolled_options_report_hidden_options() {
    let border = border();
    let scrolled = [
        border.as_str(),
        " ☐ Season",
        "",
        "Pick a season?",
        "",
        "❯ 1. Spring",
        "     mild days",
        "  2. Summer",
        "     hot days",
        "↓ 3. Autumn",
        "     cool days",
        &border,
        "  6. Chat about this",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ];
    let prompt = read_claude_prompt(&scrolled, 5, 0, None).unwrap();
    assert!(prompt.has_hidden_options);
    let numbers: Vec<u64> = prompt.options.iter().map(|option| option.number).collect();
    assert_eq!(numbers, [1, 2, 3, 6]);
    assert_eq!(
        (
            prompt.options[2].label.as_str(),
            prompt.options[2].description.as_str()
        ),
        ("Autumn", "cool days")
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-7ac011
#[test]
fn notice_on_top_border_is_not_mixed_into_title() {
    let mut screen = lines(&[
        "● earlier reply",
        "tmux focus-events off · add 'set -g focus-events on' to ~/.tmux.conf and re… ▔",
    ]);
    screen.extend(model_switch().into_iter().skip(1));
    let prompt = read_claude_prompt(&screen, 4, 3, None).unwrap();
    assert_eq!(
        prompt.title,
        "Switch model?\nYour next response will be slower and use more tokens"
    );
    assert_eq!(prompt.detail, "");
}

// parity: TS-aiSessions-paneChatClaudePrompt-89489d
#[test]
fn refuses_screens_that_are_not_prompts() {
    let border = border();
    let cases: [(Vec<&str>, usize, usize); 7] = [
        (vec![&border, "❯ ", &border, "  ? for shortcuts"], 1, 2),
        (vec![&border, "❯ 1. first", "  2. second", &border], 1, 10),
        (
            vec![
                "Pick one:",
                "  1. first",
                "  2. second",
                &border,
                "❯ ",
                &border,
            ],
            4,
            2,
        ),
        (vec!["Continue?", "❯ 1. Yes"], 1, 0),
        (vec!["Continue?", "❯ 2. Yes", "  3. No"], 1, 0),
        (vec!["Continue?", "❯ 1. Yes", "  3. No"], 1, 0),
        (vec!["❯ 1. Yes", "  2. No"], 0, 0),
    ];
    for (screen, cursor_row, cursor_column) in cases {
        assert_eq!(
            read_claude_prompt(&screen, cursor_row, cursor_column, None),
            None,
            "{screen:?}"
        );
    }
}

fn asked() -> Vec<ClaudeAskedQuestion> {
    vec![ClaudeAskedQuestion {
        question: "Which preset?".to_string(),
        options: vec![
            ClaudeAskedOption {
                label: "Use the shared configuration preset from the platform team with strict lint rules enabled".to_string(),
                description: "recommended for new repositories".to_string(),
            },
            ClaudeAskedOption {
                label: "Minimal".to_string(),
                description: "smallest setup".to_string(),
            },
        ],
    }]
}

fn asked_json() -> serde_json::Value {
    json!([{
        "question": "Which preset?",
        "options": [
            {"label": "Use the shared configuration preset from the platform team with strict lint rules enabled", "description": "recommended for new repositories"},
            {"label": "Minimal", "description": "smallest setup"}
        ]
    }])
}

/// 50칸 pane에서 라벨이 두 줄로 넘어가고, 둘째 줄이 화면 끝에 가까워 설명과 가를 수 없는 화면
fn narrow() -> Vec<String> {
    lines(&[
        "Which preset?",
        "",
        "❯ 1. Use the shared configuration preset from the",
        "     platform team with strict lint rules enabled",
        "     recommended for new repositories",
        "  2. Minimal",
        "     smallest setup",
        "  3. Type something.",
        "  4. Chat about this",
    ])
}

// parity: TS-aiSessions-paneChatClaudePrompt-f003b1
#[test]
fn reads_latest_question_tool_call() {
    let records = [
        json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "name": "AskUserQuestion", "input": {"questions": [{"question": "Old?", "options": [{"label": "A", "description": "a"}]}]}}]}}),
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "answered"}]}}),
        json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "asking"}, {"type": "tool_use", "name": "AskUserQuestion", "input": {"questions": asked_json()}}]}}),
    ];
    assert_eq!(read_claude_asked_questions(&records), asked());
    let bash = [
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}]}}),
    ];
    assert!(read_claude_asked_questions(&bash).is_empty());
    let broken = [
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "AskUserQuestion", "input": {"questions": "broken"}}]}}),
    ];
    assert!(read_claude_asked_questions(&broken).is_empty());
}

// parity: TS-aiSessions-paneChatClaudePrompt-cf10bc
#[test]
fn splits_label_and_description_by_original_question() {
    let screen = read_claude_prompt(&narrow(), 2, 0, Some(50)).unwrap();
    assert_eq!(
        screen.options[0].label,
        "Use the shared configuration preset from the"
    );
    let options = apply_asked_questions(screen, &asked()).options;
    let asked = asked();
    assert_eq!(
        (options[0].number, options[0].label.as_str()),
        (1, asked[0].options[0].label.as_str())
    );
    assert_eq!(options[0].description, "recommended for new repositories");
    assert_eq!(
        (options[1].label.as_str(), options[1].description.as_str()),
        ("Minimal", "smallest setup")
    );
    assert_eq!(
        (options[2].label.as_str(), options[2].is_text_entry),
        ("Type something.", true)
    );
    assert_eq!(
        (options[3].number, options[3].label.as_str()),
        (4, "Chat about this")
    );
}

// parity: TS-aiSessions-paneChatClaudePrompt-b90ebe
#[test]
fn mismatched_question_keeps_screen_reading() {
    let screen = read_claude_prompt(&narrow(), 2, 0, Some(50)).unwrap();
    let other_question = vec![ClaudeAskedQuestion {
        question: "Another question?".to_string(),
        options: asked()[0].options.clone(),
    }];
    assert_eq!(
        apply_asked_questions(screen.clone(), &other_question),
        screen
    );
    let different_option = vec![ClaudeAskedQuestion {
        question: "Which preset?".to_string(),
        options: vec![
            ClaudeAskedOption {
                label: "Different".to_string(),
                description: "text".to_string(),
            },
            asked()[0].options[1].clone(),
        ],
    }];
    assert_eq!(
        apply_asked_questions(screen.clone(), &different_option).options[0],
        screen.options[0]
    );
    let permission_screen = read_claude_prompt(&permission(), 7, 1, None).unwrap();
    assert_eq!(
        apply_asked_questions(permission_screen.clone(), &asked()),
        permission_screen
    );
}
