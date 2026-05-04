# Kanvibe Coding Conventions

## Shortcut Handling

- Define shortcut commands in shared shortcut utilities with semantic command names, then consume those definitions from renderer components and Electron main handlers.
- Keep shortcut formatting, capture, browser-event matching, and Electron `before-input-event` matching behind the shared shortcut interface.
- Express cross-platform shortcuts with the `Mod` modifier. `Mod` means `Command` on macOS and `Control` on Linux or other non-macOS platforms.
- Resolve the active shortcut platform through the shared platform helper instead of checking `navigator.platform`, `process.platform`, `metaKey`, or `ctrlKey` directly in feature code.
- Renderer keyboard events and Electron `before-input-event` inputs must be routed through the same shared matcher so macOS and Linux behavior stays consistent.
- Shortcut capture UIs must store normalized shortcut strings from the shared capture helper, not hand-built modifier strings.
- When adding or changing a shortcut, cover the shared command definition, display formatting, Electron input matching, renderer global handling, and any user-configurable capture flow with focused tests.
