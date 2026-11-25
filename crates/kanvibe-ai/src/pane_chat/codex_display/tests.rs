use super::*;

const DISPLAYED_THREAD: &str = "0190aaaa-0000-7000-8000-00000000000a";
const OTHER_THREAD: &str = "0190bbbb-0000-7000-8000-00000000000b";
const HINT_ROW: &str = "  ← for agents · ? for shortcuts";

/// headless 터미널 줄을 흉내 낸 테스트용 줄. 글자 하나가 한 칸이고, 쓰지 않은 칸은 빈 문자열이다
struct TestCellRow {
    cells: Vec<(String, bool)>,
}

impl TestCellRow {
    fn new() -> Self {
        Self { cells: Vec::new() }
    }

    fn write(mut self, text: &str, is_dim: bool) -> Self {
        self.cells.extend(
            text.chars()
                .map(|character| (character.to_string(), is_dim)),
        );
        self
    }

    fn pad_to(mut self, columns: usize) -> Self {
        self.cells.resize(columns, (String::new(), false));
        self
    }
}

impl TerminalCellRow for TestCellRow {
    fn column_count(&self) -> usize {
        self.cells.len()
    }

    fn cell_chars(&self, column: usize) -> Option<String> {
        self.cells.get(column).map(|(chars, _)| chars.clone())
    }

    fn is_cell_dim(&self, column: usize) -> bool {
        self.cells.get(column).is_some_and(|(_, is_dim)| *is_dim)
    }

    fn trimmed_text(&self) -> String {
        let text: String = self
            .cells
            .iter()
            .map(|(chars, _)| {
                if chars.is_empty() {
                    " "
                } else {
                    chars.as_str()
                }
            })
            .collect();
        text.trim_end().to_string()
    }
}

fn display(composer_text: &str, thread_id: Option<&str>) -> Option<CodexDisplay> {
    Some(CodexDisplay {
        composer_text: composer_text.to_string(),
        thread_id: thread_id.map(str::to_string),
    })
}

fn footer(thread: &str) -> String {
    format!("  {thread}")
}

// parity: TS-aiSessions-paneChatCodexDisplay-54fdf9
#[test]
fn recognizes_dim_native_hint_with_padding() {
    for hint in ["Ask Codex to do anything", "Summarize recent commits"] {
        let row = TestCellRow::new()
            .write("› ", false)
            .write(hint, true)
            .write(&" ".repeat(40), false)
            .pad_to(80);
        assert!(is_dim_hint_empty_composer(Some(&row), 2));
        let viewport = [
            format!("› {hint}"),
            String::new(),
            footer(DISPLAYED_THREAD),
            "  ← for agents · ? for shortcuts".to_string(),
        ];
        assert_eq!(
            read_codex_display(&viewport, 0, true),
            display("", Some(DISPLAYED_THREAD))
        );
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-c558b7
#[test]
fn refuses_user_written_hint_with_same_characters() {
    let row = TestCellRow::new()
        .write("› Ask Codex to do anything", false)
        .pad_to(80);
    assert!(!is_dim_hint_empty_composer(Some(&row), 2));
}

// parity: TS-aiSessions-paneChatCodexDisplay-c7796d
#[test]
fn accepts_verified_narrow_main_footer_without_relaxing_position() {
    for hint in [
        "  ← for agents",
        "  ← for agents        ⚠ 2 warnings · f2 to view",
    ] {
        let mut rows = vec![
            "› Ask Codex to do anything".to_string(),
            String::new(),
            footer(DISPLAYED_THREAD),
            hint.to_string(),
        ];
        assert_eq!(
            read_codex_display(&rows, 0, true)
                .unwrap()
                .thread_id
                .as_deref(),
            Some(DISPLAYED_THREAD)
        );
        assert!(is_codex_main_composer_ready(&rows, 0));
        let mut extended = rows.clone();
        extended.push("  more".to_string());
        assert_eq!(
            read_codex_display(&extended, 0, true).unwrap().thread_id,
            None
        );
        rows[3] = "  Side from main thread · ⌃/ to switch · ⌃c to close".to_string();
        assert_eq!(read_codex_display(&rows, 0, true).unwrap().thread_id, None);
        assert!(!is_codex_main_composer_ready(&rows, 0));
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-21bd53
#[test]
fn keeps_warning_badge_separate_from_thread_id() {
    let mut rows = vec![
        "› Ask Codex to do anything".to_string(),
        String::new(),
        footer(DISPLAYED_THREAD),
        format!("{HINT_ROW}                       ⚠ 1 warning · f2 to view"),
    ];
    assert_eq!(
        read_codex_display(&rows, 0, true),
        display("", Some(DISPLAYED_THREAD))
    );
    assert!(is_codex_main_composer_ready(&rows, 0));
    rows[3] = "  tab to queue message".to_string();
    assert_eq!(
        read_codex_display(&rows, 0, true)
            .unwrap()
            .thread_id
            .as_deref(),
        Some(DISPLAYED_THREAD)
    );
    assert!(!is_codex_main_composer_ready(&rows, 0));
}

// parity: TS-aiSessions-paneChatCodexDisplay-717ed9
#[test]
fn reads_thread_id_below_empty_composer() {
    let rows = [
        "  >_ OpenAI Codex (v0.169.0)",
        "",
        "› Ask Codex to do anything",
        "",
        &footer(DISPLAYED_THREAD),
        HINT_ROW,
    ];
    assert_eq!(
        read_codex_display(&rows, 2, false),
        display("Ask Codex to do anything", Some(DISPLAYED_THREAD))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-faf6cc
#[test]
fn accepts_main_thread_label_after_id() {
    let rows = [
        "› Ask Codex to do anything".to_string(),
        String::new(),
        format!("  {DISPLAYED_THREAD} · Main [default]"),
        format!("{HINT_ROW} · ⌃/ for side"),
    ];
    assert_eq!(
        read_codex_display(&rows, 0, false)
            .unwrap()
            .thread_id
            .as_deref(),
        Some(DISPLAYED_THREAD)
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-95f87a
#[test]
fn reads_real_footer_not_uuid_inside_draft() {
    let rows = [
        "› draft".to_string(),
        String::new(),
        footer(OTHER_THREAD),
        String::new(),
        format!("  {DISPLAYED_THREAD} · Main [default]"),
    ];
    assert_eq!(
        read_codex_display(&rows, 0, false),
        display(&format!("draft\n\n{OTHER_THREAD}"), Some(DISPLAYED_THREAD))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-4aaa8a
#[test]
fn reads_multi_row_draft_with_blank_line() {
    let rows = [
        "› first line",
        "",
        "  third line",
        "",
        &footer(DISPLAYED_THREAD),
    ];
    assert_eq!(
        read_codex_display(&rows, 2, false),
        display("first line\n\nthird line", Some(DISPLAYED_THREAD))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-11c54e
#[test]
fn reports_no_thread_id_for_other_footers() {
    let footers = [
        "  gpt-sol high · ~/repo".to_string(),
        format!("  {DISPLAYED_THREAD} · gpt-sol high · ~/repo"),
        format!("  {}…", &DISPLAYED_THREAD[..30]),
        format!("  thread {DISPLAYED_THREAD}"),
        format!("  {DISPLAYED_THREAD}0"),
    ];
    for footer_row in footers {
        let rows = ["› Ask Codex to do anything", "", &footer_row, HINT_ROW];
        assert_eq!(
            read_codex_display(&rows, 0, false),
            display("Ask Codex to do anything", None),
            "{footer_row}"
        );
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-5ecf90
#[test]
fn ignores_uuid_in_transcript_above_composer() {
    let rows = [
        &footer(OTHER_THREAD),
        "",
        "› Ask Codex to do anything",
        "",
        "  gpt-sol high · ~/repo",
        HINT_ROW,
    ];
    assert_eq!(read_codex_display(&rows, 2, false).unwrap().thread_id, None);
}

// parity: TS-aiSessions-paneChatCodexDisplay-740168
#[test]
fn reads_typed_command_under_popup() {
    let rows = [
        "› /mod",
        "",
        "  /model       choose what model and reasoning effort to use",
    ];
    assert_eq!(read_codex_display(&rows, 0, false), display("/mod", None));
}

// parity: TS-aiSessions-paneChatCodexDisplay-9e8e7b
#[test]
fn reads_composer_below_popup_with_same_marker() {
    let rows = [
        "› /model  choose what model and reasoning effort to use",
        "",
        "› /mo",
        "",
        &footer(DISPLAYED_THREAD),
    ];
    assert_eq!(
        read_codex_display(&rows, 2, false),
        display("/mo", Some(DISPLAYED_THREAD))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-58180f
#[test]
fn reads_draft_taller_than_twelve_rows() {
    let draft_rows: Vec<String> = (1..=14).map(|index| format!("line {index}")).collect();
    let mut rows = vec![format!("› {}", draft_rows[0])];
    rows.extend(draft_rows[1..].iter().map(|row| format!("  {row}")));
    rows.push(String::new());
    rows.push(footer(DISPLAYED_THREAD));
    assert_eq!(
        read_codex_display(&rows, 13, false),
        display(&draft_rows.join("\n"), Some(DISPLAYED_THREAD))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-e7200e
#[test]
fn returns_nothing_when_first_row_scrolled_out() {
    assert_eq!(
        read_codex_display(
            &["  line 11", "  line 12", "  line 13", "  line 14"],
            3,
            false
        ),
        None
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-47b181
#[test]
fn returns_nothing_when_cursor_is_not_on_composer() {
    let rows = [
        "• Working (3s • esc to interrupt)",
        "",
        &footer(DISPLAYED_THREAD),
    ];
    assert_eq!(read_codex_display(&rows, 0, false), None);
    assert_eq!(read_codex_display::<&str>(&[], 0, false), None);
}

const DRAFT_UUID: &str = "11111111-1111-7111-8111-111111111111";

// parity: TS-aiSessions-paneChatCodexDisplay-48ae3b
#[test]
fn draft_uuid_is_not_footer_without_shortcut_hint() {
    let rows = [
        "› Ask Codex to do anything",
        "",
        &footer(DRAFT_UUID),
        "  this is still user input",
    ];
    assert_eq!(
        read_codex_display(&rows, 0, false),
        display("Ask Codex to do anything", None)
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-f61b6f
#[test]
fn refuses_invented_label_after_id() {
    let labelled = format!("  {DRAFT_UUID} · invented-label");
    assert_eq!(
        read_codex_display(&["› Ask Codex to do anything", "", &labelled], 0, false)
            .unwrap()
            .thread_id,
        None
    );
    assert_eq!(
        read_codex_display(&["› draft", "", &labelled], 0, false)
            .unwrap()
            .thread_id,
        None
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-106714
#[test]
fn empty_composer_thread_needs_exact_layout() {
    let thread = footer(DISPLAYED_THREAD);
    let cases: [Vec<&str>; 5] = [
        vec![&thread],
        vec![
            &thread,
            "  Side from main thread · ⌃/ to switch · ⌃c to close",
        ],
        vec![&thread, "  ⌃c quit"],
        vec![&thread, HINT_ROW, "  more"],
        vec!["", &thread, HINT_ROW],
    ];
    for rows_below in cases {
        let mut rows = vec!["› Ask Codex to do anything", ""];
        rows.extend(rows_below);
        assert_eq!(
            read_codex_display(&rows, 0, false),
            display("Ask Codex to do anything", None),
            "{rows:?}"
        );
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-c407f4
#[test]
fn blank_or_hint_first_row_does_not_read_footer() {
    let thread = footer(DISPLAYED_THREAD);
    assert_eq!(
        read_codex_display(&["›", "", &thread], 0, false)
            .unwrap()
            .thread_id,
        None
    );
    assert_eq!(
        read_codex_display(
            &["› Ask Codex to do anything", "  second row", "", &thread],
            1,
            false
        )
        .unwrap()
        .thread_id,
        None
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-e149fd
#[test]
fn draft_layout_id_comes_with_differing_composer_text() {
    // 화면만으로는 초안의 마지막 줄과 footer를 구분할 수 없어, 이 배치의 ID는 이미 연결된 대화와 같은지 견주는 데만 쓴다
    assert_eq!(
        read_codex_display(&["› draft", "", &footer(DRAFT_UUID)], 0, false),
        display("draft", Some(DRAFT_UUID))
    );
}

// parity: TS-aiSessions-paneChatCodexDisplay-79771f
#[test]
fn allows_only_verified_version() {
    assert!(is_codex_thread_footer_supported("codex-cli 0.169.0"));
    assert!(is_codex_thread_footer_supported("codex-cli 0.169.0\n"));
}

// parity: TS-aiSessions-paneChatCodexDisplay-4843a4
#[test]
fn refuses_other_versions() {
    for output in [
        "codex-cli 0.168.0",
        "codex-cli 0.170.0",
        "codex-cli 0.169.0-alpha.1",
        "codex-cli 0.169.01",
        "Python 3.9.6",
        "",
    ] {
        assert!(!is_codex_thread_footer_supported(output), "{output}");
    }
}

fn same(composer: &str, expected: &str) -> bool {
    is_same_composer_text(composer, expected, None)
}

// parity: TS-aiSessions-paneChatCodexDisplay-c3e15f
#[test]
fn keeps_whitespace_inside_row_literal() {
    assert!(!same("print(\"ab\")", "print(\"a b\")"));
    assert!(!same("a  b", "a b"));
    assert!(!same("  indented", "indented"));
}

// parity: TS-aiSessions-paneChatCodexDisplay-285b3a
#[test]
fn never_reads_row_break_as_fold_without_width() {
    assert!(!same("ab\ncd", "ab cd"));
    assert!(!same("ab\ncd", "abcd"));
    assert!(!same(
        &format!("{}\ny", "x".repeat(33)),
        &format!("{} y", "x".repeat(33))
    ));
    let full = "wrap ".repeat(29).trim().to_string();
    let tail = "wrap ".repeat(16).trim().to_string();
    assert!(!same(&format!("{full}\n{tail}"), "wrap ".repeat(45).trim()));
}

// parity: TS-aiSessions-paneChatCodexDisplay-370c5a
#[test]
fn matches_exact_multi_line_text() {
    assert!(same("ab\ncd", "ab\ncd"));
    assert!(same(
        "가나다 라마바\n\n사아자 ✓",
        "가나다 라마바\n\n사아자 ✓"
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-c2ccf0
#[test]
fn ignores_only_trailing_whitespace() {
    assert!(same("Use $qa-skill", "Use $qa-skill "));
    assert!(same("first\n\nthird", "first \n\nthird"));
}

// parity: TS-aiSessions-paneChatCodexDisplay-3fb4c9
#[test]
fn requires_every_row_and_line() {
    assert!(!same("first\nsecond", "first"));
    assert!(!same("first", "first\nsecond"));
    assert!(!same("first\n\nthird", "first\nthird"));
}

// parity: TS-aiSessions-paneChatCodexDisplay-205b5b
#[test]
fn distinguishes_different_and_partial_echo() {
    assert!(!same("Ask Codex to do anything", "Must stay unsubmitted"));
    assert!(!same("/statusline thr", "/statusline thread-id"));
    assert!(same("/statusline thr", "/statusline thr"));
}

const VIEWPORT_COLUMNS: Option<usize> = Some(150);

fn full_row() -> String {
    "wrap ".repeat(29).trim().to_string()
}

fn same_at(composer: &str, expected: &str, columns: Option<usize>) -> bool {
    is_same_composer_text(composer, expected, columns)
}

// parity: TS-aiSessions-paneChatCodexDisplay-417233
#[test]
fn accepts_observed_korean_wrap() {
    assert!(same_at(
        "REAL_KOREAN_1794666776347 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글\n입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입\n력 확인 한글 입력 확인",
        "REAL_KOREAN_1794666776347 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인",
        Some(80)
    ));
    assert!(!same_at("한글\n입력", "한글 입력", Some(80)));
}

// parity: TS-aiSessions-paneChatCodexDisplay-df1c51
#[test]
fn row_break_is_not_fold_when_next_word_fits() {
    let x = "x".repeat(33);
    assert!(!same_at(
        &format!("{x}\ny"),
        &format!("{x} y"),
        VIEWPORT_COLUMNS
    ));
    assert!(!same_at("ab\ncd", "ab cd", VIEWPORT_COLUMNS));
    let full = full_row();
    assert!(!same_at(
        &format!("{full}\nfit"),
        &format!("{full} fit"),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-898400
#[test]
fn joins_line_folded_at_space() {
    let full = full_row();
    let tail = "wrap ".repeat(16).trim().to_string();
    assert!(same_at(
        &format!("{full}\n{tail}"),
        "wrap ".repeat(45).trim(),
        VIEWPORT_COLUMNS
    ));
    assert!(same_at(
        &format!("first line\n\n{full}\nwrap tail"),
        &format!("first line\n\n{full}  wrap tail"),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-8ac9e1
#[test]
fn joins_path_folded_after_hyphen() {
    let row_ending_at_hyphen = format!("@/{}/desktop-pane-", "d".repeat(128));
    let shown = format!("read this\n\n{row_ending_at_hyphen}\nchat-session/notes.md");
    let expected = format!("read this\n\n{row_ending_at_hyphen}chat-session/notes.md");
    assert!(same_at(&shown, &expected, VIEWPORT_COLUMNS));
    // 더 넓은 화면이었다면 다음 조각이 같은 줄에 들어갔을 것이므로 접힌 자리가 아니다
    assert!(!same_at(&shown, &expected, Some(160)));
    assert!(!same_at(
        &format!("{row_ending_at_hyphen}\n2026/notes.md"),
        &format!("{row_ending_at_hyphen}2026/notes.md"),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-f8dbc7
#[test]
fn joins_token_cut_exactly_at_width() {
    assert!(same_at(
        &format!("{}\naaa", "a".repeat(148)),
        &"a".repeat(151),
        VIEWPORT_COLUMNS
    ));
    assert!(!same_at(
        &format!("{}\naaaa", "a".repeat(147)),
        &"a".repeat(151),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-3b17c6
#[test]
fn refuses_row_wider_than_width() {
    assert!(!same_at(
        &format!("{}\naa", "a".repeat(149)),
        &"a".repeat(151),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-388d7f
#[test]
fn refuses_fold_before_no_break_characters() {
    let full = full_row();
    for (shown_tail, expected_tail) in [
        (") tail", " ) tail"),
        ("/usr/local/bin", " /usr/local/bin"),
        (", tail", " , tail"),
    ] {
        assert!(!same_at(
            &format!("{full}\n{shown_tail}"),
            &format!("{full}{expected_tail}"),
            VIEWPORT_COLUMNS
        ));
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-9574c2
#[test]
fn refuses_fold_after_opening_bracket() {
    let row = format!("{}wra(", "wrap ".repeat(28));
    assert!(!same_at(
        &format!("{row}\nlonger"),
        &format!("{row} longer"),
        VIEWPORT_COLUMNS
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-940b09
#[test]
fn refuses_fold_in_uncertain_column_width() {
    let cases = [
        format!("{}✓✓✓✓", "wrap ".repeat(28)),
        format!("{}e\u{301}eee", "wrap ".repeat(28)),
        format!("{}👩\u{200d}💻 wrap", "wrap ".repeat(27)),
    ];
    for row in cases {
        assert!(!same_at(
            &format!("{row}\nwrap"),
            &format!("{row} wrap"),
            VIEWPORT_COLUMNS
        ));
        assert!(same_at(
            &format!("{row}\nwrap"),
            &format!("{row}\nwrap"),
            VIEWPORT_COLUMNS
        ));
    }
}

// parity: TS-aiSessions-paneChatCodexDisplay-24e9cf
#[test]
fn keeps_whitespace_literal_with_width() {
    assert!(!same_at(
        "print(\"ab\")",
        "print(\"a b\")",
        VIEWPORT_COLUMNS
    ));
    assert!(!same_at("first\nsecond", "first", VIEWPORT_COLUMNS));
    assert!(same_at("Use $qa-skill", "Use $qa-skill ", VIEWPORT_COLUMNS));
}

// parity: TS-aiSessions-paneChatCodexDisplay-627de2
#[test]
fn offers_literal_text_and_folded_placeholder() {
    assert_eq!(
        expect_codex_composer_texts("한글 ✓ text", 0),
        ["한글 ✓ text", "[Pasted Content 9 chars]"]
    );
}

fn matches_any(body: &str, image_count: usize, composer: &str) -> bool {
    expect_codex_composer_texts(body, image_count)
        .iter()
        .any(|expected| same(composer, expected))
}

// parity: TS-aiSessions-paneChatCodexDisplay-c8556d
#[test]
fn matches_observed_rows_after_pasting_image() {
    assert!(matches_any(
        "caption after image",
        1,
        "[Image #1] caption after image"
    ));
    assert!(matches_any("", 1, "[Image #1]"));
    assert!(matches_any(
        &"L".repeat(1500),
        0,
        "[Pasted Content 1500 chars]"
    ));
}

// parity: TS-aiSessions-paneChatCodexDisplay-959459
#[test]
fn rejects_other_caption_or_missing_image() {
    assert!(!matches_any(
        "caption after image",
        1,
        "[Image #1] another caption"
    ));
    assert!(!matches_any(
        "caption after image",
        1,
        "caption after image"
    ));
    assert!(!matches_any("", 2, "[Image #1]"));
}
