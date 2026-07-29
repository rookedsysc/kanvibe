# Roky Harness QA — Issue #310 live GPUI QA dispatch

## Verdict

`PASS` for the portable QA-dispatch slice. The real packaged macOS S01–S14
replay remains `BLOCKED` until it runs twice on a macOS executor with the
required screenshots, videos, database snapshots, and manual checklist.
This is a prompt-separated self-review because isolated QA-agent delegation
was not requested.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Runtime socket reaches live GPUI state | PASS | `qa_runtime_channel`, `watch_qa_runtime`, and `Entity::update_in` route each request into the open `KanVibeRoot` |
| Production actions replace synthetic runtime mutation | PASS | Navigation, create/submit, settings, notifications, filters, Vim/search, context actions, drag/drop, dock, and diff selection use GPUI actions or the same `KanVibeRoot` handlers as pointer/keyboard UI |
| Scenario values reach production handlers | PASS | `SyntheticClick.payload` preserves form, project, status, layout, and setting values from the S01–S14 scenario step |
| Runtime queries reflect live state | PASS | Route, overlays, dock, diff, filters, task visibility, settings, pane layout, and DB snapshots are built from current `KanVibeRoot` state |
| Capture lifecycle persists | PASS | Start/stop video commands share the window-owned runtime capture state |
| Unsupported input cannot report a false pass | PASS | Unsupported mouse/semantic/key targets return `accepted: false` with a structured dispatch status |
| Release builds expose QA socket | PASS | No; the runtime Unix listener remains `debug_assertions`-only |

## Command evidence

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-features --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS — 156 tests |
| `cargo build --workspace --release` | PASS |
| `git diff --check` | PASS |
| `cargo check -p kanvibe-app --target aarch64-apple-darwin` | BLOCKED before the app crate in `ring`; this Linux host has no Apple SDK/sysroot |

The portable app source-contract test also proves that runtime startup uses
`spawn_debug_qa_runtime_socket_from_env`, updates the live entity, dispatches a
real GPUI action, and no longer starts the static-state runtime socket.
Replay-plan tests prove that S02 form values are serialized into the socket
command payload.

## Architecture review

- Socket transport and request/response ownership remain in `qa_control.rs`.
- GPUI action mapping and live state queries remain with `KanVibeRoot` in
  `native_ui.rs`.
- The bridge uses the existing standard-library channel and GPUI async-window
  conventions; no new dependency or cross-crate framework was added.
- The headless state model remains available only for portable protocol tests
  and in-process smoke reports. It is no longer the state mutated by a running
  GPUI application.

## Remaining real-macOS gates

- Compile and launch the debug GPUI app through the macOS CI/Phase 5 path.
- Execute S01–S14 twice against fresh database copies and confirm that every
  command reports `gpui-production-action-dispatched`.
- Validate screenshot/video capture, async task/diff completion ordering,
  pointer and keyboard behavior, and persisted DB/file/git results.
- Complete signing, notarization, Gatekeeper, DMG update, stabilization, and
  rollback evidence before changing defaults or removing Electron.
