# KanVibe Agent Conventions

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

## UI Color Tokens

- Use `#0064FF` as the primary point color for PR buttons, primary actions, selected states, links, focus borders, and other important interactive highlights.
- Keep point-color usage behind semantic tokens such as `--color-brand-primary`, `--color-brand-hover`, `--color-brand-active`, `--color-brand-subtle`, and `--color-tag-pr-*` instead of hard-coding hex values in components.
- Use `#202632` for neutral button-like surfaces that should read as actionable but not alerting, such as compact shortcut buttons, base/project badges, and non-notification controls.
- Keep neutral button usage behind semantic tokens such as `--color-button-neutral-*`, `--color-tag-project-*`, and `--color-tag-base-*`.
- Do not use the primary point color for warning, error, success, or notification severity. Keep those on the existing `status-*` semantic tokens.
