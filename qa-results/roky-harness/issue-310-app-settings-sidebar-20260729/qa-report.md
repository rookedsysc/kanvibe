# Roky Harness QA — Issue #310 app settings/sidebar

## Verdict

`PASS` for settings consumption and S13 replay. Packaged sidebar, Done,
notification, and release interaction evidence remains in Phase 5.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Default collapse | PASS (source contract) | Every native window initializes Task Detail sidebar state from `sidebar_default_collapsed` |
| Native sidebar | PASS (source contract) | Expanded view shows project, branch, session type, and SSH/local context |
| Hint dismissal | PASS | `dismiss_native_sidebar_hint` persists once and updates in-memory settings immediately |
| Done dismissal | PASS | Existing confirmation persists `done_alert_dismissed` before applying Done status/move |
| Notification settings | PASS (source contract) | Enable toggle and per-status selection call typed settings updates |
| S13 replay | PASS | Sidebar collapse and hint dismissal run before the existing-window focus assertion; DB snapshot verifies persistence |
| Packaged runtime | BLOCKED | Real sidebar pointer/keyboard, Done, notification, and release evidence requires macOS |

## Command evidence

| Command | Result |
| --- | --- |
| `cargo test -p kanvibe-app --lib` | PASS — 69 tests |
| `cargo test -p qa-harness` | PASS after S13 DB command count update |
| Focused app/QA Clippy with warnings denied | PASS |
| S13 JSON parse and `git diff --check` | PASS |

## Roky review

- Hint persistence is a dedicated typed app function; UI code does not write
  settings keys directly.
- Failed persistence leaves the hint visible and surfaces the mutation error.
- The ledger advances from `SERVICE` to `RUNTIME` without claiming packaged
  behavior before Phase 5.
- The context-loader skill's repository-local principle files were unavailable,
  so existing Rust architecture, Electron behavior, and Roky acceptance rules
  were used as the fallback.
