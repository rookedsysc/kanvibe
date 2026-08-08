# KanVibe Agent Conventions

## Runtime Environment Safety

- Avoid writing code that mutates generic process or server environment variables, because Electron main, hook servers, shells, tmux, zellij, and spawned child processes can inherit those values.
- Do not set generic environment variables such as `PORT`, `NODE_ENV`, `HOST`, `PATH`, `HOME`, or similar process-wide defaults to express KanVibe runtime state.
- Treat `PORT`, `HOST`, `NODE_ENV`, `PATH`, `HOME`, and `KANVIBE_*` as examples, not the full boundary. Do not introduce behavior that can affect the overall process-wide, user-wide, shell-wide, tmux-wide, zellij-wide, or agent-wide environment.
- Avoid global environment side effects such as mutating `process.env` for runtime wiring, writing shell startup files, exporting variables into interactive sessions, changing global package-manager or OS environment config, or forwarding server/runtime environments wholesale.
- Prefer explicit function arguments, typed configuration objects, or module-local state for runtime wiring such as server ports, hosts, feature flags, and mode selection.
- If an environment variable is truly required, use a KanVibe-scoped name such as `KANVIBE_*`, document why it must be process-wide, and keep it out of user shell environments unless shell inheritance is the intended behavior.
- When constructing child process, PTY, shell, tmux, or zellij environments, start from the narrowest required environment and avoid blindly leaking server-only runtime values into interactive sessions.
- Route local child process and terminal environment creation through `createLocalShellEnvironment()` so `PORT`, `HOST`, `NODE_ENV`, and KanVibe internal `KANVIBE_*` values are stripped before user-visible shells run.
- When changing terminal or child process environment handling, add focused tests that assert server/runtime variables do not appear in task terminal environments.

### Approved Exception: tmux `set-clipboard`

The rule above forbids tmux-wide side effects. tmux `set-clipboard` is an approved exception, because OSC 52 clipboard forwarding cannot be delivered without it and tmux offers no narrower scope.

- `set-clipboard` is a tmux **server** option. `-s`, `-g`, and `-t <session>` all land on the same server-wide value, so KanVibe cannot confine it to its own sessions. KanVibe also creates sessions on the default socket, so the effect reaches every session on that server.
- KanVibe may set `set-clipboard on` only while bootstrapping its own tmux session, and only when the user's tmux configuration does not already choose a value. An explicit user choice always wins and is never overwritten or restored later.
- Detection must follow tmux's own configuration search order, including the system config and `${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf`. Do not hardcode `$HOME/.config`. `source-file` indirection is out of scope.
- When detection cannot run, treat the result as unknown and leave the option alone. Never treat a failed check as permission to write.
- Keep this exception narrow. Any other server-wide, user-wide, shell-wide, tmux-wide, or zellij-wide side effect needs its own entry here, with the same three parts: why no narrower scope exists, what guard respects an explicit user choice, and what the failure default is. Do not add one silently.

## App-Wide UI Settings

- Store app-wide UI preferences in the app settings layer (`appSettingsService` / `AppSettings`), not in route-specific state or route cache.
- Treat `AppSettings` as the source of truth for preferences that must survive app restarts, such as "don't show again" dismissals.
- Use route state and route cache only for route-local render data. Do not persist global UI preferences in route cache.
- If an old route cache contains a former global preference field, strip or ignore that field when reading the cache.
- Avoid `sessionStorage` or `localStorage` for app-wide preferences unless the requested behavior is explicitly browser-session scoped.

### Sidebar Hint Dismissal

- The sidebar fold hint dismissal is app-wide.
- The dismissal should be written through `dismissSidebarHint()` and persisted as `sidebar_hint_dismissed` in `AppSettings`.
- `TaskDetailRoute` may keep a local React state copy for rendering, but it must not write `sidebarHintDismissed` into task-detail route cache.

## Shortcut Handling

- Define shortcut commands in shared shortcut utilities with semantic command names, then consume those definitions from renderer components and Electron main handlers.
- Keep shortcut formatting, capture, browser-event matching, and Electron `before-input-event` matching behind the shared shortcut interface.
- Express cross-platform shortcuts with the `Mod` modifier. `Mod` means `Command` on macOS and `Control` on Linux or other non-macOS platforms.
- Resolve the active shortcut platform through the shared platform helper instead of checking `navigator.platform`, `process.platform`, `metaKey`, or `ctrlKey` directly in feature code.
- Renderer keyboard events and Electron `before-input-event` inputs must be routed through the same shared matcher so macOS and Linux behavior stays consistent.
- Shortcut capture UIs must store normalized shortcut strings from the shared capture helper, not hand-built modifier strings.
- When adding or changing a shortcut, cover the shared command definition, display formatting, Electron input matching, renderer global handling, and any user-configurable capture flow with focused tests.
- Task detail dock shortcuts must use the shared dock shortcut helpers and bind to `Mod+{number}`. Electron `before-input-event` must intercept these before terminal input receives them.
- Terminal tab shortcuts use `Mod+Alt+{number}`, so plain `Mod+{number}` stays with the task detail dock.
- Do not bind an app shortcut to `Cmd+Shift+{number}` on macOS. The system claims `Cmd+Shift+3/4/5` for screenshots and never delivers them to the app, so such a binding fails silently on some numbers only.
- Task detail dock numbering excludes the back-to-board button and must be derived from the dock item array order, not hard-coded per item. Keep PR as the conditional slot after the first three dock items: with PR it is slot 4 and later items shift to 5+, without PR the next dock item gets slot 4.

## Task Navigation

- Route all task-detail transitions through `navigateToTaskDetail()` in `src/desktop/renderer/utils/taskNavigation.ts`.
- Before navigating the current window to a task detail route, focus an already-open window for the same task detail route when one exists.
- Keep explicit new-window actions on `openInternalRouteInNewWindow()` or `navigateToTaskDetail(..., { openInNewWindow: true })` so Electron main can reuse its existing-window focus policy.
- Do not hand-roll `router.push("/task/...")`, `redirect("/.../task/...")`, or `window.location.hash` for task detail transitions in feature components.

## UI Color Tokens

- Use `#0064FF` as the primary point color for PR buttons, primary actions, selected states, links, focus borders, and other important interactive highlights.
- Keep point-color usage behind semantic tokens such as `--color-brand-primary`, `--color-brand-hover`, `--color-brand-active`, `--color-brand-subtle`, and `--color-tag-pr-*` instead of hard-coding hex values in components.
- Use `#202632` for neutral button-like surfaces that should read as actionable but not alerting, such as compact shortcut buttons, base/project badges, and non-notification controls.
- Keep neutral button usage behind semantic tokens such as `--color-button-neutral-*`, `--color-tag-project-*`, and `--color-tag-base-*`.
- Do not use the primary point color for warning, error, success, or notification severity. Keep those on the existing `status-*` semantic tokens.
