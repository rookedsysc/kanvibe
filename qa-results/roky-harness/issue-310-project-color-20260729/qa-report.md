# Roky Harness QA — Issue #310 project color

## Verdict

`PASS` for the portable Rust service and production GPUI source contract.
Packaged pointer/keyboard behavior and the resulting card screenshot/DB evidence
remain part of the real-macOS S08 gate.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Electron preset parity | PASS | One shared eight-color preset list matches `ProjectColorEditor.tsx` |
| Input safety | PASS | Rust accepts only `#RRGGBB` and normalizes valid colors to uppercase |
| Stale target safety | PASS | A deleted/missing project returns a stable user-facing error before update |
| Worktree grouping | PASS | Updating a main project changes the matching main/worktree repository rows |
| Invalid input rollback | PASS | Focused test proves invalid input returns before any DB mutation |
| Production GPUI consumer | PASS (source contract) | Task detail renders selectable preset buttons, disables them during save, writes off the UI thread, and refreshes the board snapshot |
| S08 replay contract | PASS | `setProjectColor` dispatches through the live GPUI entity and an explicit eventual assertion waits for the reloaded task color |
| Packaged runtime | BLOCKED | S08 still needs real GPUI pointer/keyboard, DB, and screenshot evidence |

## Command evidence

| Command | Result |
| --- | --- |
| Focused RED test before implementation | FAIL as expected — updater function absent |
| Focused project-color test after implementation | PASS |
| `cargo clippy -p kanvibe-app --all-targets --all-features -- -D warnings` | PASS |
| `./kanvibe-native check` | PASS |
| `./kanvibe-native test` | PASS — 165 workspace tests |
| `./kanvibe-native build` | PASS |
| Apple-target cross-check | BLOCKED as expected before app type-check — Linux host C compiler cannot build `ring` with Apple `-arch`/deployment flags |
| `git diff --check` | PASS |

## Roky review

- Backend boundary reuses `KanvibeDb::update_project_color`; no duplicate SQL
  path was introduced.
- UI state is keyed by project id, preventing duplicate writes while one save
  is active.
- Failure is logged through existing native diagnostics and remains visible in
  the task-detail error surface.
- The repository-local context-loader references were unavailable, so the
  existing Rust architecture and Roky design/QA contracts were used as the
  fallback.
