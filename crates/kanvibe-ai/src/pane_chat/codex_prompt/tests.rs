use super::*;

/// Codex CLI 0.169.0이 실제로 그린 화면을 옮긴 것. 선택 화면은 화면 아래에 붙고, 위쪽은 빈 줄 둘로 대화 내용과 떨어진다
const HISTORY: [&str; 6] = [
    "• Ran git tag probe-approval-1",
    "  └ (no output)",
    "",
    "  Worked for 28s • 6:23 PM",
    "",
    "",
];

fn with_history(rows: &[&str]) -> Vec<String> {
    HISTORY
        .iter()
        .chain(rows)
        .map(|row| row.to_string())
        .collect()
}

fn approval() -> Vec<String> {
    with_history(&[
        "  Would you like to run the following command?",
        "",
        "  Environment: local",
        "",
        "  Reason: Allow creation of the requested Git tag?",
        "",
        "  $ git tag probe-approval-1",
        "",
        "",
        "› 1. Yes, proceed (y)",
        "  2. Yes, and don't ask again for commands that start with `git tag probe-approval-1` (p)",
        "  3. No, and tell Codex what to do differently (esc)",
        "",
        "  Press enter to confirm or esc to cancel",
    ])
}

fn rate_limit() -> Vec<String> {
    with_history(&[
        "  Approaching rate limits",
        "  Switch to gpt-6-luna for lower credit usage?",
        "",
        "",
        "  1. Switch to gpt-6-luna                   Fast and affordable model for easier tasks.",
        "› 2. Keep current model",
        "  3. Keep current model (never show again)  Hide future rate limit reminders about switching models",
        "",
        "  enter select · esc back",
    ])
}

fn composer() -> Vec<String> {
    with_history(&[
        "• Working (5s • esc to interrupt)",
        "",
        "› Ask Codex to do anything",
        "",
        "  GPT-6.1 Terra medium · ~/repo",
        "  ← for agents · ? for shortcuts",
    ])
}

fn option(number: u64, label: &str, description: &str) -> PaneChatPromptOption {
    PaneChatPromptOption {
        number,
        label: label.to_string(),
        description: description.to_string(),
        is_checked: false,
        is_text_entry: false,
    }
}

// parity: TS-aiSessions-paneChatCodexPrompt-ef0282
#[test]
fn reads_command_approval_as_permission() {
    assert_eq!(
        read_codex_prompt(&approval()),
        Some(PaneChatPromptScreen {
            kind: PaneChatPromptKind::Permission,
            title: "Would you like to run the following command?".to_string(),
            detail: "Environment: local\n\nReason: Allow creation of the requested Git tag?\n\n$ git tag probe-approval-1".to_string(),
            steps_line: None,
            highlighted_number: 1,
            is_multi_select: false,
            is_text_entry_active: false,
            has_hidden_options: false,
            options: vec![
                option(1, "Yes, proceed (y)", ""),
                option(2, "Yes, and don't ask again for commands that start with `git tag probe-approval-1` (p)", ""),
                option(3, "No, and tell Codex what to do differently (esc)", ""),
            ],
        })
    );
}

fn rate_limit_options() -> Vec<PaneChatPromptOption> {
    vec![
        option(
            1,
            "Switch to gpt-6-luna",
            "Fast and affordable model for easier tasks.",
        ),
        option(2, "Keep current model", ""),
        option(
            3,
            "Keep current model (never show again)",
            "Hide future rate limit reminders about switching models",
        ),
    ]
}

// parity: TS-aiSessions-paneChatCodexPrompt-552ab2
#[test]
fn reads_rate_limit_warning_with_inline_descriptions() {
    let prompt = read_codex_prompt(&rate_limit()).unwrap();
    assert_eq!(prompt.kind, PaneChatPromptKind::Confirmation);
    assert_eq!(
        prompt.title,
        "Approaching rate limits\nSwitch to gpt-6-luna for lower credit usage?"
    );
    assert_eq!(prompt.detail, "");
    assert_eq!(prompt.highlighted_number, 2);
    assert_eq!(prompt.options, rate_limit_options());
}

// parity: TS-aiSessions-paneChatCodexPrompt-ecf143
#[test]
fn joins_wrapped_label_and_hyphen_break() {
    let narrow: Vec<String> = approval()
        .into_iter()
        .flat_map(|line| {
            if line.starts_with("  2. ") {
                vec![
                    "  2. Yes, and don't ask again for commands that start with `git tag qa-approval-".to_string(),
                    "     yes` (p)".to_string(),
                ]
            } else {
                vec![line]
            }
        })
        .collect();
    let options = read_codex_prompt(&narrow).unwrap().options;
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Yes, proceed (y)",
            "Yes, and don't ask again for commands that start with `git tag qa-approval-yes` (p)",
            "No, and tell Codex what to do differently (esc)",
        ]
    );
    assert_eq!(options[1].description, "");
}

// parity: TS-aiSessions-paneChatCodexPrompt-17221c
#[test]
fn joins_row_wrapped_in_description_column() {
    let narrow = with_history(&[
        "  Approaching rate limits",
        "  Switch to gpt-6-luna for lower credit usage?",
        "",
        "",
        "› 1. Switch to gpt-6-luna                   Fast and affordable model for easier",
        "                                            tasks.",
        "  2. Keep current model",
        "  3. Keep current model (never show again)  Hide future rate limit reminders",
        "                                            about switching models",
        "",
        "  enter select · esc back",
    ]);
    let prompt = read_codex_prompt(&narrow).unwrap();
    assert_eq!(
        prompt.title,
        "Approaching rate limits\nSwitch to gpt-6-luna for lower credit usage?"
    );
    assert_eq!(prompt.highlighted_number, 1);
    assert_eq!(prompt.options, rate_limit_options());
}

// parity: TS-aiSessions-paneChatCodexPrompt-6fff3f
#[test]
fn reads_with_trailing_blank_rows() {
    let mut screen = rate_limit();
    screen.extend([String::new(), String::new()]);
    assert!(
        read_codex_prompt(&screen)
            .unwrap()
            .title
            .contains("Approaching rate limits")
    );
}

// parity: TS-aiSessions-paneChatCodexPrompt-1adddb
#[test]
fn does_not_mix_conversation_into_title_or_detail() {
    let prompt = read_codex_prompt(&approval()).unwrap();
    assert!(!format!("{}\n{}", prompt.title, prompt.detail).contains("Worked for"));
}

// parity: TS-aiSessions-paneChatCodexPrompt-2a7af3
#[test]
fn refuses_screens_that_are_not_prompts() {
    let without_esc: Vec<String> = rate_limit()
        .into_iter()
        .map(|line| {
            if line == "  enter select · esc back" {
                "  enter select".to_string()
            } else {
                line
            }
        })
        .collect();
    let to_rows = |rows: &[&str]| rows.iter().map(|row| row.to_string()).collect::<Vec<_>>();
    let cases = [
        composer(),
        without_esc,
        to_rows(&[
            "",
            "",
            "  Continue?",
            "",
            "",
            "› 1. Yes",
            "",
            "  enter select · esc back",
        ]),
        to_rows(&[
            "",
            "",
            "  Continue?",
            "",
            "",
            "› 2. Yes",
            "  3. No",
            "",
            "  enter select · esc back",
        ]),
        to_rows(&[
            "",
            "",
            "  Continue?",
            "",
            "",
            "  1. Yes",
            "  2. No",
            "",
            "  enter select · esc back",
        ]),
        to_rows(&["› 1. Yes", "  2. No", "", "  enter select · esc back"]),
        to_rows(&[
            "",
            "",
            "  Continue?",
            "",
            "",
            "› 1. Yes",
            "  2. No",
            "  enter select · esc back",
        ]),
    ];
    for screen in cases {
        assert_eq!(read_codex_prompt(&screen), None, "{screen:?}");
    }
}

const CONTENT: [&str; 4] = [
    "• You ran echo qa-content",
    "  └ qa-content",
    "",
    "  Worked for <1s • 10:14 PM",
];

fn conversation_above(rows_above_dialog: &[&str]) -> Vec<String> {
    let dialog = approval()[HISTORY.len()..].to_vec();
    let mut screen: Vec<String> = rows_above_dialog
        .iter()
        .map(|row| row.to_string())
        .collect();
    screen.extend([String::new(), String::new()]);
    screen.extend(dialog);
    read_codex_prompt_with_conversation(&screen)
        .unwrap()
        .conversation_rows
        .into_iter()
        .filter(|row| !js_trim(row).is_empty())
        .collect()
}

fn content_rows() -> Vec<String> {
    CONTENT
        .iter()
        .filter(|row| !js_trim(row).is_empty())
        .map(|row| row.to_string())
        .collect()
}

// parity: TS-aiSessions-paneChatCodexPrompt-c9618a
#[test]
fn without_header_all_rows_above_are_conversation() {
    assert_eq!(conversation_above(&CONTENT), content_rows());
}

// parity: TS-aiSessions-paneChatCodexPrompt-ba57f4
#[test]
fn header_is_removed_from_conversation() {
    let narrow_header = [
        "",
        "  >_ OpenAI Codex (v0.169.0)",
        "     ~/projects/…/run/workspace",
        "",
    ];
    let wide_header = [
        "",
        "  >_ OpenAI Codex (v0.169.0)",
        "     ~/projects/qa/run/workspace",
        "",
    ];
    let narrow = conversation_above(&[&narrow_header[..], &CONTENT[..]].concat());
    let wide = conversation_above(&[&wide_header[..], &CONTENT[..]].concat());
    assert_eq!(narrow, wide);
    assert_eq!(narrow, content_rows());
}

// parity: TS-aiSessions-paneChatCodexPrompt-566496
#[test]
fn boxed_header_is_removed_to_its_end() {
    let boxed_header = [
        "╭──────────────────────────────────────╮",
        "│ >_ OpenAI Codex (v0.169.0)           │",
        "│                                      │",
        "│ model:     gpt-6-luna medium         │",
        "│ directory: ~/projects/qa/workspace   │",
        "╰──────────────────────────────────────╯",
        "",
    ];
    assert_eq!(
        conversation_above(&[&boxed_header[..], &CONTENT[..]].concat()),
        content_rows()
    );
}

// parity: TS-aiSessions-paneChatCodexPrompt-004301
#[test]
fn reads_only_below_last_header() {
    let new_header = [
        "",
        "  >_ OpenAI Codex (v0.169.0)",
        "     ~/projects/qa/run/workspace",
        "",
    ];
    assert!(conversation_above(&[&CONTENT[..], &new_header[..]].concat()).is_empty());
}
