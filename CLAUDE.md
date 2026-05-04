# Kanvibe Coding Conventions

## Shortcut Handling

- Keep shortcut definitions, formatting, capture, and matching in shared shortcut utilities instead of duplicating platform-specific checks in components or Electron handlers.
- Express cross-platform shortcuts with the `Mod` modifier. `Mod` means `Command` on macOS and `Control` on Linux or other non-macOS platforms.
- Renderer keyboard events and Electron `before-input-event` inputs must be routed through the same shared matcher so both environments honor identical shortcut behavior.
- Do not introduce app actions for reserved OS or Electron shortcuts such as `Cmd/Ctrl+R`. If a shortcut must be unavailable, add it to the shared blocked-shortcut list and block it in both renderer and Electron main routing.
- Shortcut capture UIs must reject blocked shortcuts instead of saving them. Runtime shortcut handlers must also ignore blocked shortcuts in case a stale setting already exists.
- When adding or changing a shortcut, cover the shared matcher, Electron input matching, renderer global handling, and any user-configurable capture flow with focused tests.
