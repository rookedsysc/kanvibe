# KanVibe Agent Conventions

## Runtime Environment Safety

- Avoid writing code that mutates generic process or server environment variables, because the desktop app process, the in-process hook server, shells, tmux, zellij, and spawned child processes can inherit those values.
- Do not set generic environment variables such as `PORT`, `NODE_ENV`, `HOST`, `PATH`, `HOME`, or similar process-wide defaults to express KanVibe runtime state.
- Treat `PORT`, `HOST`, `NODE_ENV`, `PATH`, `HOME`, and `KANVIBE_*` as examples, not the full boundary. Do not introduce behavior that can affect the overall process-wide, user-wide, shell-wide, tmux-wide, zellij-wide, or agent-wide environment.
- Avoid global environment side effects such as calling `std::env::set_var` for runtime wiring, writing shell startup files, exporting variables into interactive sessions, changing global package-manager or OS environment config, or forwarding server/runtime environments wholesale.
- Prefer explicit function arguments, typed configuration structs, or module-local state for runtime wiring such as server ports, hosts, feature flags, and mode selection. Read the process environment once at startup and pass the values down.
- If an environment variable is truly required, use a KanVibe-scoped name such as `KANVIBE_*`, document why it must be process-wide, and keep it out of user shell environments unless shell inheritance is the intended behavior.
- When constructing child process, PTY, shell, tmux, or zellij environments, start from the narrowest required environment and avoid blindly leaking server-only runtime values into interactive sessions.
- Route local child process and terminal environment creation through `create_local_shell_environment()` in `kanvibe-process` (`crates/kanvibe-process/src/shell_environment.rs`) so `PORT`, `HOST`, `NODE_ENV`, and KanVibe internal `KANVIBE_*` values are stripped before user-visible shells run.
- When changing terminal or child process environment handling, add focused tests that assert server/runtime variables do not appear in task terminal environments.

### Approved Exception: tmux `set-clipboard`

The rule above forbids tmux-wide side effects. tmux `set-clipboard` is an approved exception, because OSC 52 clipboard forwarding cannot be delivered without it and tmux offers no narrower scope.

- `set-clipboard` is a tmux **server** option. `-s`, `-g`, and `-t <session>` all land on the same server-wide value, so KanVibe cannot confine it to its own sessions. KanVibe also creates sessions on the default socket, so the effect reaches every session on that server.
- KanVibe may set `set-clipboard on` only while bootstrapping its own tmux session, and only when the user's tmux configuration does not already choose a value. An explicit user choice always wins and is never overwritten or restored later.
- Detection must follow tmux's own configuration search order, including the system config and `${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf`. Do not hardcode `$HOME/.config`. `source-file` indirection is out of scope.
- When detection cannot run, treat the result as unknown and leave the option alone. Never treat a failed check as permission to write.
- Keep this exception narrow. Any other server-wide, user-wide, shell-wide, tmux-wide, or zellij-wide side effect needs its own entry here, with the same three parts: why no narrower scope exists, what guard respects an explicit user choice, and what the failure default is. Do not add one silently.

### Approved Exception: Codex `/statusline thread-id` on User Request

The rule above forbids agent-wide side effects. Typing `/statusline thread-id` into a running Codex TUI is an approved exception, because a Codex 0.169 TUI attached to the shared app-server daemon exposes no other signal that ties a pane to its conversation.

- Why no narrower scope exists: the daemon's protocol has no method that maps a TUI connection or PID to the thread it shows, and hooks run under the daemon, which cannot prove pane ownership. The footer drawn by the TUI itself is the only exact signal. The command changes only that TUI session's status line and is not written to `~/.codex/config.toml`, but KanVibe cannot restore the previous status line items for the user.
- Guard that respects the user's choice: KanVibe types the command only when the user presses "대화 ID 표시" in the pane chat view, never while polling. It pastes the command once, only on the verified version allowlist, only when Codex's own dim empty-composer hint proves the input box is empty, inside the task mutation queue, and only after re-checking the same pane and CLI process before the paste and before Enter.
- Failure default: when any check fails or the pasted command does not echo back in the composer, KanVibe does not send Enter and leaves the conversation unconnected. It never falls back to typing characters one by one, guessing the newest conversation, or changing Codex configuration.

## App-Wide UI Settings

- Store app-wide UI preferences in the app settings layer (`AppSettingsService` in `kanvibe-db`, backed by the `app_settings` table), not in view state or per-route state.
- Treat `app_settings` as the source of truth for preferences that must survive app restarts, such as "don't show again" dismissals.
- Use view state and per-route state only for route-local render data. Do not persist global UI preferences there.
- If persisted per-route state contains a former global preference field, strip or ignore that field when reading it.
- Avoid per-window session state (window-scoped globals that vanish when the app quits) for app-wide preferences unless the requested behavior is explicitly window-session scoped.

### Sidebar Hint Dismissal

- The sidebar fold hint dismissal is app-wide.
- The dismissal should be written through `AppSettingsService::dismiss_sidebar_hint()` and persisted as `sidebar_hint_dismissed` in `app_settings`.
- The task detail view (`TaskDetailView`) may keep a local copy for rendering, but it must not store the dismissal in task-detail per-route state.

## Shortcut Handling

- Define shortcut commands in the shared shortcut module (`kanvibe_domain::shortcuts`, `ShortcutCommandId` and `shortcut_command_definitions()`) with semantic command names, then consume those definitions from GPUI views and the app-level dispatcher (`crates/kanvibe-app/src/shortcuts.rs`).
- Keep shortcut formatting, capture, and key-event matching behind the shared shortcut interface (`kanvibe_domain::shortcuts::notation` and `key_event`).
- Express cross-platform shortcuts with the `Mod` modifier. `Mod` means `Command` on macOS and `Control` on Linux or other non-macOS platforms.
- Resolve the active shortcut platform through the shared platform helper (`ShortcutPlatform::current()`) instead of checking `cfg!(target_os)`, `Keystroke` `platform`/`control` modifiers, or the OS name directly in feature code.
- View key handlers (`on_key_down`) and the app-wide keystroke interceptor (`App::intercept_keystrokes` in `install_shortcut_dispatch()`) must convert GPUI keystrokes with `shortcut_event_from_keystroke()` and use the same shared matcher (`match_shortcut_event()`) so macOS and Linux behavior stays consistent.
- Shortcut capture UIs must store normalized shortcut strings from the shared capture helper (`capture_shortcut_from_event()`), not hand-built modifier strings.
- When adding or changing a shortcut, cover the shared command definition, display formatting, key-event matching, the app-level dispatch through GPUI key contexts, and any user-configurable capture flow with focused tests.
- Task detail dock shortcuts must use the shared dock shortcut helper (`task_detail_dock_shortcut()`) and bind to `Mod+{number}`. The app-level keystroke interceptor must handle these before the focused terminal (`Terminal` key context) receives them.
- Terminal tab shortcuts use `Mod+Alt+{number}` (`terminal_tab_shortcut()`), so plain `Mod+{number}` stays with the task detail dock.
- Do not bind an app shortcut to `Cmd+Shift+{number}` on macOS. The system claims `Cmd+Shift+3/4/5` for screenshots and never delivers them to the app, so such a binding fails silently on some numbers only.
- Task detail dock numbering excludes the back-to-board button and must be derived from the dock item array order (`TASK_DETAIL_DOCK_ITEM_IDS`), not hard-coded per item. Keep PR as the conditional slot after the first three dock items: with PR it is slot 4 and later items shift to 5+, without PR the next dock item gets slot 4.

## Task Navigation

- Route all task-detail transitions through `navigate_to_task_detail()` in `crates/kanvibe-app/src/windows.rs`.
- Before navigating the current window to a task detail route, focus an already-open window for the same task detail route when one exists.
- Keep explicit new-window actions on `navigate_to_task_detail()` with `TaskNavigationRequest { should_open_new_window: true, .. }`, or on `open_href()` for in-app links, so the window registry can reuse its existing-window focus policy.
- Do not hand-roll `AppWindow::navigate()` with a task detail href, or call `open_app_window()` for a task detail route, in feature views.

## UI Color Tokens

- Use `#0064FF` as the primary point color for PR buttons, primary actions, selected states, links, focus borders, and other important interactive highlights.
- Keep point-color usage behind the semantic theme tokens in `crates/kanvibe-app/src/theme.rs` instead of hard-coding hex values in views: `ThemeTokens.brand` (`primary`, `hover`, `active`, `subtle`; formerly the CSS tokens `--color-brand-primary`, `--color-brand-hover`, `--color-brand-active`, `--color-brand-subtle`) and `tag.pull_request` (formerly `--color-tag-pr-*`).
- Use `#202632` for neutral button-like surfaces that should read as actionable but not alerting, such as compact shortcut buttons, base/project badges, and non-notification controls.
- Keep neutral button usage behind semantic tokens: `ThemeTokens.button_neutral` (formerly `--color-button-neutral-*`), `tag.project` (formerly `--color-tag-project-*`), and `tag.base_branch` (formerly `--color-tag-base-*`).
- Do not use the primary point color for warning, error, success, or notification severity. Keep those on the existing `ThemeTokens.status` semantic tokens.
