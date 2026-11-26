//! TS `paneChatService.test.ts`의 "pane chat agent activity"와 "Claude 선택 화면에 채팅에서 답하기".

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use kanvibe_ai::pane_chat::claude_prompt::PaneChatPromptKind;
use kanvibe_ai::pane_chat::process::PaneChatProvider;
use kanvibe_ai::pane_chat::snapshot::{PaneChatAgentActivity, PaneChatSnapshot};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::AiMessageRole;
use serde_json::{Value, json};

use super::*;
use crate::pane_chat::service::PaneChatClock;
use crate::pane_chat::surface::PaneChatSurface;

/// TS `processTable().replace(codex, claude)`. foreground CLI가 Claude인 pane
fn claude_processes() -> String {
    process_table(DEFAULT_PROCESS_START).replace(
        "/usr/bin/python3 python3 /tmp/codex",
        "/tmp/claude /tmp/claude",
    )
}

fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

/// TS `claudeScreen(lines, cursorRow, cursorColumn)`. 커서 줄이 입력 줄이다
fn claude_screen(lines: &[String], cursor_row: usize, cursor_column: usize) -> PaneChatSurface {
    PaneChatSurface {
        viewport_lines: Some(lines.to_vec()),
        cursor_row: Some(cursor_row),
        cursor_column: Some(cursor_column),
        input_line: lines.get(cursor_row).cloned(),
        ..surface()
    }
}

mod activity {
    use super::*;

    const CLAUDE_WORKING_FOOTER: &str =
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt · ← 1 agent";
    const CLAUDE_IDLE_FOOTER: &str = "  ⏵⏵ auto mode on (shift+tab to cycle) · ← 1 agent";

    /// Claude Code 2.1.325를 tmux에서 실제로 돌려 캡처한 화면의 아래쪽. 작업 중일 때만 footer에 중단 안내가 붙는다
    fn claude_screen_lines(footer: &str, above_box: &str) -> Vec<String> {
        let rule = "─".repeat(40);
        rows(&[
            above_box,
            "                                    ● high · /effort",
            &rule,
            "❯ ",
            &rule,
            footer,
        ])
    }

    /// Codex 0.169를 실제로 돌려 캡처한 화면의 아래쪽. 작업 중일 때만 입력란 위에 Working 줄이 생긴다
    fn codex_screen_lines(status: &str) -> Vec<String> {
        rows(&[
            status,
            "                         ⚠ weekly limit: 10% left · /status",
            "› Ask Codex to do anything",
            "  GPT-6.1-Sol high · /repo",
            "  ← for agents · ? for shortcuts",
        ])
    }

    fn activity_of(harness: &Harness) -> PaneChatAgentActivity {
        harness
            .service
            .get_pane_chat_snapshot("task-chat", Some("%3"))
            .unwrap()
            .agent_activity
    }

    #[test]
    fn claude_activity_reads_only_the_footer_interrupt_hint() {
        let cases = [
            (
                PaneChatAgentActivity::Working,
                claude_screen_lines(
                    CLAUDE_WORKING_FOOTER,
                    "· Swooping… (2s · thinking with high effort)",
                ),
            ),
            (
                PaneChatAgentActivity::Idle,
                claude_screen_lines(CLAUDE_IDLE_FOOTER, ""),
            ),
            // 입력란 위 대화 본문에 같은 문구가 있어도 footer에 없으면 작업 중이 아니다
            (
                PaneChatAgentActivity::Idle,
                claude_screen_lines(
                    CLAUDE_IDLE_FOOTER,
                    "Press esc to interrupt a running agent.",
                ),
            ),
        ];
        for (expected, viewport_lines) in cases {
            let harness = Harness::new();
            harness.respond_all_commands(&claude_processes());
            harness.surfaces.set(Some(PaneChatSurface {
                input_line: Some("❯ ".to_string()),
                cursor_column: Some(2),
                cursor_row: Some(3),
                viewport_lines: Some(viewport_lines),
                ..surface()
            }));
            assert_eq!(activity_of(&harness), expected);
        }
    }

    #[test]
    fn codex_activity_reads_the_working_status_above_the_composer() {
        for (expected, status) in [
            (
                PaneChatAgentActivity::Working,
                "• Working (0s • esc to interrupt)",
            ),
            (PaneChatAgentActivity::Idle, ""),
        ] {
            let harness = Harness::new();
            let viewport_lines = codex_screen_lines(status);
            harness.surfaces.set(Some(PaneChatSurface {
                input_line: Some(viewport_lines[2].clone()),
                cursor_column: Some(2),
                cursor_row: Some(2),
                viewport_lines: Some(viewport_lines),
                ..surface()
            }));
            assert_eq!(activity_of(&harness), expected);
        }
    }

    #[test]
    fn activity_is_unknown_without_screen_coordinates_or_for_unverified_clis() {
        let harness = Harness::new();
        harness.respond_all_commands(&claude_processes());
        harness.surfaces.set(Some(PaneChatSurface {
            input_line: None,
            cursor_column: None,
            screen: claude_screen_lines(
                CLAUDE_WORKING_FOOTER,
                "· Swooping… (2s · thinking with high effort)",
            )
            .join("\n"),
            ..surface()
        }));
        assert_eq!(activity_of(&harness), PaneChatAgentActivity::Unknown);

        harness.respond_all_commands(
            "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 opencode.exe opencode",
        );
        let mut open_binding = binding("open", "/allowed/open");
        open_binding.revision = Some(1);
        harness.sessions.set_process_binding(Some(open_binding));
        harness.sessions.respond_detail(|call| {
            Ok((call.provider == PaneChatProvider::Opencode)
                .then(|| detail("open", AiSessionProvider::Opencode, Vec::new(), None)))
        });
        let viewport_lines = rows(&[
            "  ┃  Ask anything…",
            "  ┃  Build · esc to interrupt",
            "  ╹▀▀▀",
        ]);
        harness.surfaces.set(Some(PaneChatSurface {
            input_line: Some(viewport_lines[0].clone()),
            cursor_column: Some(5),
            cursor_row: Some(0),
            viewport_lines: Some(viewport_lines),
            ..surface()
        }));
        assert_eq!(activity_of(&harness), PaneChatAgentActivity::Unknown);
    }
}

mod answers {
    use super::*;

    const UP: &str = "\x1b[A";
    const DOWN: &str = "\x1b[B";
    const RIGHT: &str = "\x1b[C";
    const LEFT: &str = "\x1b[D";

    fn model_switch() -> Vec<String> {
        rows(&[
            &"▔".repeat(20),
            "   Switch model?",
            "   ❯ 1. Yes, switch to Opus 5.5",
            "     2. No, go back",
        ])
    }

    fn question_head() -> Vec<String> {
        rows(&[
            &"─".repeat(20),
            "←  ☐ Size  ☐ Notes  ✔ Submit  →",
            "",
            "Which size?",
            "",
        ])
    }

    fn question() -> Vec<String> {
        [
            question_head(),
            rows(&["❯ 1. Small", "  2. Large", "  3. Type something."]),
        ]
        .concat()
    }

    fn toppings_head() -> Vec<String> {
        rows(&[
            &"─".repeat(20),
            "←  ☐ Toppings  ✔ Submit  →",
            "",
            "Which toppings?",
            "",
        ])
    }

    fn input_box() -> Vec<String> {
        rows(&[&"─".repeat(20), "❯ ", &"─".repeat(20)])
    }

    fn input_box_screen() -> PaneChatSurface {
        claude_screen(&input_box(), 1, 2)
    }

    /// 단일 선택 질문의 직접 입력 줄이 `text`를 입력받는 중인 화면
    fn typing_screen(text: &str) -> PaneChatSurface {
        let lines = [
            question_head(),
            rows(&["  1. Small", "  2. Large", &format!("❯ 3. {text}")]),
        ]
        .concat();
        claude_screen(&lines, 7, 5 + text.encode_utf16().count())
    }

    /// 다중 선택 질문. `highlighted`번 줄이 강조돼 있고 3번이 직접 입력 줄이다
    fn toppings_screen(highlighted: usize, text_entry: &str, cheese: &str) -> PaneChatSurface {
        let options = [
            format!("1. {cheese} Cheese"),
            "2. [ ] Olives".to_string(),
            format!("3. {text_entry}"),
        ];
        let mut lines = toppings_head();
        for (index, row) in options.iter().enumerate() {
            let marker = if index + 1 == highlighted { "❯" } else { " " };
            lines.push(format!("{marker} {row}"));
        }
        lines.push("     Submit".to_string());
        claude_screen(
            &lines,
            4 + highlighted,
            if highlighted == 3 { 9 } else { 0 },
        )
    }

    fn toppings(highlighted: usize) -> PaneChatSurface {
        toppings_screen(highlighted, "[ ] Type something", "[ ]")
    }

    /// 키를 보낼 때마다 화면을 바꾸는 가짜 CLI. 준비한 화면을 다 쓰면 선택 화면이 닫히고 입력란이 나온다(TS beforeEach의 write)
    struct ClaudePromptPane {
        harness: Harness,
        next_screens: Arc<Mutex<VecDeque<Option<PaneChatSurface>>>>,
    }

    impl ClaudePromptPane {
        fn new() -> Self {
            let harness = Harness::new();
            harness.respond_all_commands(&claude_processes());
            harness
                .surfaces
                .set(Some(claude_screen(&model_switch(), 2, 3)));
            let next_screens: Arc<Mutex<VecDeque<Option<PaneChatSurface>>>> = Arc::default();
            let surfaces = harness.surfaces.clone();
            let queue = next_screens.clone();
            harness.bridge.respond(move |_, _, _| {
                match queue.lock().unwrap().pop_front() {
                    Some(Some(screen)) => surfaces.set(Some(screen)),
                    // 화면을 바꾸지 않는 키(TS `mockImplementationOnce(async () => {})`)
                    Some(None) => {}
                    None => surfaces.set(Some(input_box_screen())),
                }
                Ok(())
            });
            Self {
                harness,
                next_screens,
            }
        }

        fn show(&self, screen: PaneChatSurface) {
            self.harness.surfaces.set(Some(screen));
        }

        /// TS `screenAfterEachKey(...screens)`
        fn screens_after_each_key(&self, screens: Vec<PaneChatSurface>) {
            self.next_screens
                .lock()
                .unwrap()
                .extend(screens.into_iter().map(Some));
        }

        fn snapshot(&self) -> PaneChatSnapshot {
            self.harness
                .service
                .get_pane_chat_snapshot("task-chat", Some("%3"))
                .unwrap()
        }

        /// 답을 보내고 그 결과를 "sent" 또는 오류 이름으로 받는다
        fn answer(&self, snapshot: &PaneChatSnapshot, requested: Value) -> &'static str {
            let prompt_id = snapshot
                .prompt
                .as_ref()
                .map(|prompt| prompt.id.clone())
                .unwrap_or_default();
            let result = self.harness.service.answer_pane_chat_prompt(
                "task-chat",
                snapshot.target_token.as_deref().unwrap(),
                &prompt_id,
                &requested,
            );
            match result {
                Ok(()) => "sent",
                Err(error) => error_name(&error),
            }
        }

        fn typed_keys(&self) -> Vec<String> {
            self.harness.bridge.written_bytes()
        }
    }

    #[test]
    fn snapshot_carries_the_on_screen_prompt_with_its_kind() {
        let pane = ClaudePromptPane::new();
        let prompt = pane.snapshot().prompt.unwrap();
        assert_eq!(prompt.screen.kind, PaneChatPromptKind::Confirmation);
        assert_eq!(prompt.screen.title, "Switch model?");
        let options: Vec<(u64, &str)> = prompt
            .screen
            .options
            .iter()
            .map(|option| (option.number, option.label.as_str()))
            .collect();
        assert_eq!(
            options,
            vec![(1, "Yes, switch to Opus 5.5"), (2, "No, go back")]
        );
        assert!(!prompt.id.is_empty());
    }

    #[test]
    fn prompt_kind_comes_from_the_screen_not_from_the_transcript() {
        let pane = ClaudePromptPane::new();
        let rule = "╌".repeat(20);
        let permission = rows(&[
            &"─".repeat(20),
            " Bash command",
            &rule,
            " touch a",
            &rule,
            " Do you want to proceed?",
            " ❯ 1. Yes",
            "   2. No",
        ]);
        pane.show(claude_screen(&permission, 6, 1));
        let prompt = pane.snapshot().prompt.unwrap();
        assert_eq!(prompt.screen.kind, PaneChatPromptKind::Permission);
        assert_eq!(prompt.screen.detail, "Bash command\n\ntouch a");

        pane.harness.sessions.respond_detail(|_| {
            let mut tool = message(AiMessageRole::Tool, "sleep 30");
            tool.tool_name = Some("Bash".to_string());
            Ok(Some(detail(
                "exact-session",
                AiSessionProvider::Claude,
                vec![tool],
                None,
            )))
        });
        pane.show(claude_screen(&model_switch(), 2, 3));
        assert_eq!(
            pane.snapshot().prompt.unwrap().screen.kind,
            PaneChatPromptKind::Confirmation
        );
    }

    #[test]
    fn asked_question_text_refines_labels_but_keeps_the_screen_prompt_id() {
        let pane = ClaudePromptPane::new();
        let label = "Use the shared configuration preset from the platform team with strict lint rules enabled";
        let narrow_question = rows(&[
            "Which preset?",
            "",
            "❯ 1. Use the shared configuration preset from the",
            "     platform team with strict lint rules enabled",
            "     recommended for new repositories",
            "  2. Minimal",
            "     smallest setup",
            "  3. Chat about this",
        ]);
        pane.show(PaneChatSurface {
            viewport_columns: Some(50),
            ..claude_screen(&narrow_question, 2, 0)
        });
        let from_screen_only = pane.snapshot();
        pane.harness.sessions.set_tail(Ok(vec![json!({
            "type": "assistant",
            "message": { "role": "assistant", "content": [{ "type": "tool_use", "name": "AskUserQuestion", "input": { "questions": [{
                "question": "Which preset?",
                "options": [{ "label": label, "description": "recommended for new repositories" }, { "label": "Minimal", "description": "smallest setup" }],
            }] } }] },
        })]));

        let refined = pane.snapshot();

        let screen_option = &from_screen_only.prompt.as_ref().unwrap().screen.options[0];
        assert_eq!(
            screen_option.label,
            "Use the shared configuration preset from the"
        );
        let refined_option = &refined.prompt.as_ref().unwrap().screen.options[0];
        assert_eq!(refined_option.label, label);
        assert_eq!(
            refined_option.description,
            "recommended for new repositories"
        );
        assert_eq!(
            refined.prompt.as_ref().unwrap().id,
            from_screen_only.prompt.as_ref().unwrap().id
        );
        assert_eq!(
            pane.answer(&refined, json!({ "type": "option", "number": 1 })),
            "sent"
        );
        assert_eq!(pane.typed_keys(), vec!["1"]);
    }

    #[test]
    fn composer_screens_and_unverified_cli_layouts_are_not_prompts() {
        let pane = ClaudePromptPane::new();
        pane.show(input_box_screen());
        assert!(pane.snapshot().prompt.is_none());

        pane.harness
            .respond_all_commands(&process_table(DEFAULT_PROCESS_START));
        pane.show(claude_screen(&model_switch(), 2, 3));
        assert!(pane.snapshot().prompt.is_none());
    }

    #[test]
    fn same_prompt_on_screen_sends_the_chosen_number_key_once() {
        let pane = ClaudePromptPane::new();
        let snapshot = pane.snapshot();

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 2 })),
            "sent"
        );

        assert_eq!(
            pane.harness.writes(),
            Harness::expected_writes("task-chat", "%3", &["2"])
        );
    }

    #[test]
    fn moved_highlight_is_still_the_same_prompt() {
        let pane = ClaudePromptPane::new();
        let snapshot = pane.snapshot();
        let switch = model_switch();
        let moved = rows(&[
            &switch[0],
            &switch[1],
            "     1. Yes, switch to Opus 5.5",
            "   ❯ 2. No, go back",
        ]);
        pane.show(claude_screen(&moved, 3, 3));

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 1 })),
            "sent"
        );

        assert_eq!(pane.typed_keys(), vec!["1"]);
    }

    #[test]
    fn changed_or_closed_prompt_receives_no_key() {
        let switch = model_switch();
        let other_prompt = rows(&[
            &switch[0],
            "   Do you want to proceed?",
            "   ❯ 1. Yes",
            "     2. No",
        ]);
        for changed_screen in [claude_screen(&other_prompt, 2, 3), input_box_screen()] {
            let pane = ClaudePromptPane::new();
            let snapshot = pane.snapshot();
            pane.show(changed_screen);

            assert_eq!(
                pane.answer(&snapshot, json!({ "type": "option", "number": 1 })),
                "PaneChatInputError"
            );

            assert!(pane.typed_keys().is_empty());
        }
    }

    #[test]
    fn check_state_changed_in_the_terminal_blocks_the_same_number_key() {
        let pane = ClaudePromptPane::new();
        pane.show(toppings(2));
        let snapshot = pane.snapshot();
        pane.show(toppings_screen(2, "[ ] Type something", "[✔]"));

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 1 })),
            "PaneChatInputError"
        );

        assert!(pane.typed_keys().is_empty());
    }

    #[test]
    fn answer_waits_for_the_screen_to_react_but_only_for_a_bounded_time() {
        let pane = ClaudePromptPane::new();
        let snapshot = pane.snapshot();
        // 키를 받은 CLI가 120ms 뒤에 선택 화면을 닫는다
        let reacts_at: Arc<Mutex<Option<f64>>> = Arc::default();
        let clock = pane.harness.clock.clone();
        let reaction = reacts_at.clone();
        pane.harness.bridge.respond(move |_, _, _| {
            *reaction.lock().unwrap() = Some(clock.now_ms() + 120.0);
            Ok(())
        });
        let clock = pane.harness.clock.clone();
        let reaction = reacts_at.clone();
        pane.harness.surfaces.respond(move |_, _, _| {
            let has_reacted = reaction
                .lock()
                .unwrap()
                .is_some_and(|at| clock.now_ms() >= at);
            Ok(Some(match has_reacted {
                true => input_box_screen(),
                false => claude_screen(&model_switch(), 2, 3),
            }))
        });
        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 2 })),
            "sent"
        );
        let has_reacted = reacts_at
            .lock()
            .unwrap()
            .is_some_and(|at| pane.harness.clock.now_ms() >= at);
        assert!(has_reacted, "반응을 볼 때까지 기다린 뒤 끝나야 한다");

        let unchanged_pane = ClaudePromptPane::new();
        let unchanged = unchanged_pane.snapshot();
        unchanged_pane.next_screens.lock().unwrap().push_back(None);
        assert_eq!(
            unchanged_pane.answer(&unchanged, json!({ "type": "option", "number": 2 })),
            "sent"
        );
        // 반응이 없으면 여섯 번 다시 읽고(사이마다 50ms) 끝난다
        assert_eq!(unchanged_pane.harness.clock.sleeps(), vec![50; 6]);
    }

    #[test]
    fn active_text_entry_is_left_with_up_before_sending_a_number() {
        let pane = ClaudePromptPane::new();
        pane.show(typing_screen("term  ✔"));
        let snapshot = pane.snapshot();
        let left = [
            question_head(),
            rows(&["  1. Small", "❯ 2. Large", "  3. term ✔"]),
        ]
        .concat();
        pane.screens_after_each_key(vec![claude_screen(&left, 6, 0)]);

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 1 })),
            "sent"
        );

        assert_eq!(pane.typed_keys(), vec![UP, "1"]);
    }

    #[test]
    fn active_text_entry_is_left_with_up_before_moving_between_questions() {
        let pane = ClaudePromptPane::new();
        pane.show(typing_screen("Type something."));
        let snapshot = pane.snapshot();
        let left = [
            question_head(),
            rows(&["  1. Small", "❯ 2. Large", "  3. Type something."]),
        ]
        .concat();
        pane.screens_after_each_key(vec![claude_screen(&left, 6, 0)]);

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "step", "direction": "next" })),
            "sent"
        );

        assert_eq!(pane.typed_keys(), vec![UP, RIGHT]);
    }

    #[test]
    fn single_select_text_entry_uses_its_number_then_pastes_and_submits_after_echo() {
        let pane = ClaudePromptPane::new();
        pane.show(claude_screen(&question(), 5, 0));
        let snapshot = pane.snapshot();
        pane.screens_after_each_key(vec![
            typing_screen("Type something."),
            typing_screen("Extra large"),
        ]);

        let requested = json!({ "type": "text", "number": 3, "text": " Extra large " });
        assert_eq!(pane.answer(&snapshot, requested), "sent");

        assert_eq!(
            pane.typed_keys(),
            vec!["3".to_string(), bracketed("Extra large"), "\r".to_string()]
        );
    }

    #[test]
    fn multi_select_text_entry_moves_with_arrows_and_leaves_without_enter() {
        let pane = ClaudePromptPane::new();
        pane.show(toppings(1));
        let snapshot = pane.snapshot();
        pane.screens_after_each_key(vec![
            toppings(2),
            toppings(3),
            toppings_screen(3, "[✔] Garlic", "[ ]"),
            toppings_screen(2, "[✔] Garlic", "[ ]"),
        ]);

        assert_eq!(
            pane.answer(
                &snapshot,
                json!({ "type": "text", "number": 3, "text": "Garlic" })
            ),
            "sent"
        );

        assert_eq!(
            pane.typed_keys(),
            vec![
                DOWN.to_string(),
                DOWN.to_string(),
                bracketed("Garlic"),
                UP.to_string()
            ]
        );
    }

    #[test]
    fn text_entry_already_showing_the_same_text_is_submitted_with_enter_only() {
        let pane = ClaudePromptPane::new();
        pane.show(typing_screen("Extra large"));
        let snapshot = pane.snapshot();

        let requested = json!({ "type": "text", "number": 3, "text": "Extra large" });
        assert_eq!(pane.answer(&snapshot, requested), "sent");

        assert_eq!(pane.typed_keys(), vec!["\r"]);
    }

    #[test]
    fn text_entry_holding_other_text_receives_no_key() {
        let pane = ClaudePromptPane::new();
        pane.show(typing_screen("from terminal"));
        let snapshot = pane.snapshot();

        let requested = json!({ "type": "text", "number": 3, "text": "from chat" });
        assert_eq!(pane.answer(&snapshot, requested), "PaneChatInputError");

        assert!(pane.typed_keys().is_empty());
    }

    #[test]
    fn pasted_text_not_echoed_exactly_is_not_submitted() {
        let pane = ClaudePromptPane::new();
        pane.show(typing_screen("Type something."));
        let snapshot = pane.snapshot();
        pane.screens_after_each_key(vec![typing_screen("Extra largeX")]);

        let requested = json!({ "type": "text", "number": 3, "text": "Extra large" });
        assert_eq!(pane.answer(&snapshot, requested), "PaneChatInputError");

        assert_eq!(pane.typed_keys(), vec![bracketed("Extra large")]);
    }

    #[test]
    fn step_and_cancel_answers_send_their_single_key() {
        let cases = [
            (json!({ "type": "step", "direction": "next" }), RIGHT),
            (json!({ "type": "step", "direction": "previous" }), LEFT),
            (json!({ "type": "cancel" }), "\x1b"),
        ];
        for (requested, key) in cases {
            let pane = ClaudePromptPane::new();
            pane.show(claude_screen(&question(), 5, 0));
            let snapshot = pane.snapshot();

            assert_eq!(pane.answer(&snapshot, requested), "sent");

            assert_eq!(pane.typed_keys(), vec![key]);
        }
    }

    #[test]
    fn invalid_answers_are_rejected_without_any_key() {
        let cases = [
            json!({ "type": "step", "direction": "next" }),
            json!({ "type": "option", "number": 3 }),
            json!({ "type": "option", "number": 10 }),
            json!({ "type": "text", "number": 1, "text": "note" }),
            json!({ "type": "text", "number": 1, "text": "a\nb" }),
            json!({ "type": "enter" }),
        ];
        for requested in cases {
            let pane = ClaudePromptPane::new();
            let snapshot = pane.snapshot();

            assert_eq!(
                pane.answer(&snapshot, requested.clone()),
                "PaneChatInputError",
                "{requested}"
            );

            assert!(pane.typed_keys().is_empty(), "{requested}");
        }
    }

    #[test]
    fn blank_text_entry_cannot_be_chosen_by_number() {
        let pane = ClaudePromptPane::new();
        pane.show(claude_screen(&question(), 5, 0));
        let snapshot = pane.snapshot();

        assert_eq!(
            pane.answer(&snapshot, json!({ "type": "option", "number": 3 })),
            "PaneChatInputError"
        );

        assert!(pane.typed_keys().is_empty());
    }
}
