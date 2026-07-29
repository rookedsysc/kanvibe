# Roky Harness QA — Issue #310 pane layout editor

## Verdict

`PASS` for direct layout persistence, missing-project override creation,
production GPUI source contract, and S10 replay. Packaged pointer/keyboard and
terminal split evidence remains in the macOS Phase 5 gate.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Six layout types | PASS | Single, horizontal, vertical, both asymmetric triples, and quad are direct choices |
| No cycle dependency | PASS | `save_native_pane_layout_type` writes the requested enum directly |
| Command preservation | PASS | Existing commands survive by pane index; added panes start empty and removed panes truncate |
| Missing override creation | PASS | A root project without a row can create its own override; project existence is checked first |
| Pane semantics | PASS (source contract) | Commands are labeled Top/Bottom, Left/Right, asymmetric positions, or quad corners |
| Reset | PASS | Project override deletion returns the project to the global effective layout; global reset is rejected |
| S10 replay | PASS | S10 now creates `qa-project-api`'s previously missing override and verifies `vertical_2` in the DB snapshot |
| Packaged runtime | BLOCKED | Real pointer/keyboard editing and tmux/zellij pane creation require macOS evidence |

## Command evidence

| Command | Result |
| --- | --- |
| Focused RED compile test | FAIL as expected — direct-save API absent |
| Focused missing-override test | PASS |
| `cargo test -p kanvibe-app --lib` | PASS — 69 tests |
| `cargo test -p qa-harness` | PASS — 22 tests |
| Focused app/QA Clippy with warnings denied | PASS |
| S10 JSON parse | PASS |

## Roky review

- Existing layout identity wins over caller-supplied target data, preventing a
  layout id from being retargeted to another project.
- New overrides validate their project before writing.
- S10 exercises creation rather than only updating an already seeded row.
- The ledger advances from `SERVICE` to `RUNTIME`; it remains fail-closed until
  packaged terminal layout evidence passes.
- The context-loader skill's repository-local principle files were unavailable,
  so existing Rust architecture, Electron behavior, and Roky acceptance rules
  were used as the fallback.
