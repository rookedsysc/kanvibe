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
