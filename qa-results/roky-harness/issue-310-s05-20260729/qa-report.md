# Roky Harness QA — Issue #310 S05 portable slice

## Verdict

`PASS` for the portable S05 implementation slice. This is a prompt-separated self-review because no isolated QA agent/runtime was requested. Real packaged macOS S05 remains `BLOCKED` pending a macOS executor, window capture, and video artifact.

## Acceptance coverage

| Scenario | Expected outcome | Evidence |
| --- | --- | --- |
| Text diff | Original/current content loads and guarded save rejects stale content | `tests::native_diff_snapshot_loads_original_and_current_file_content` |
| Binary diff | Numstat marks binary and the app skips text content loading | `tests::changed_files_include_branch_diff_and_working_tree_entries`; app snapshot test |
| Route load | Git/SSH work does not run from GPUI render | `load_diff_snapshot` worker/cache and portable GPUI type-check |
| File tree | Folder grouping, viewed marker, bounded width controls | GPUI source contract assertions and portable GPUI type-check |
| Editor | Dirty/saving/error states prevent duplicate/cancel-during-save actions | GPUI source contract assertions and portable GPUI type-check |

## Command evidence

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | exit 0 |
| `cargo test --workspace` | exit 0; 155 tests |
| `cargo build --workspace --release` | exit 0 |
| Temporary portable `cargo check -p kanvibe-app --all-features` | exit 0; target-only dependency boundary restored |
| `cargo check -p kanvibe-app --target aarch64-apple-darwin --no-default-features` with cross SQLite pkg-config | exit 0 |
| `bash -n scripts/package-macos-app.sh` | exit 0 |
| `bash -n scripts/phase5-macos-run.sh` | exit 0 |
| Linux `scripts/package-macos-app.sh` probe | expected exit 78 (Darwin-only gate) |
| Workflow YAML parse | exit 0 |
| `git diff --check` | exit 0 |

The first two packaging-probe attempts failed in the QA command itself: zsh reserves `status`, then the host lacked the `python` alias. The same gate was rerun with `probe_status` and `python3` and passed.

## Diff-to-plan mapping

Current-slice logical hunk count: 9. Unmapped: 0. Behavior-preserving: 0.

| File:lines | Hunk summary | Mapped to | Behavior change? |
| --- | --- | --- | --- |
| `kanvibe-git/src/lib.rs:96,1863` | Binary flag and numstat parsing | Todo 5 binary policy | Yes |
| `kanvibe-git/src/lib.rs:2134` | Real Git binary fixture/assertion | Todo 5 binary regression | Yes |
| `kanvibe-app/src/lib.rs:1096,4215` | Typed binary snapshot and skipped content reads | Todo 5 app diff contract | Yes |
| `kanvibe-app/src/lib.rs:6311,7584` | Binary behavior and GPUI consumer assertions | Todo 5 regression/consumer evidence | Yes |
| `kanvibe-app/src/native_ui.rs:42,535` | Named sidebar bounds and route state | Todo 5 resizable/viewed state | Yes |
| `kanvibe-app/src/native_ui.rs:1960` | Asynchronous cached load/retry/navigation | Todo 5 nonblocking local/SSH diff | Yes |
| `kanvibe-app/src/native_ui.rs:4766` | Asynchronous guarded save and dirty/saving state | Todo 5 editor state | Yes |
| `kanvibe-app/src/native_ui.rs:5094` | Folder/viewed/binary/sidebar UI | Todo 5 file-tree and binary parity | Yes |
| `native/README.md:226`; transition plan Todo 5 | Runtime behavior and gate evidence | Roky evidence/documentation | Yes |

## Design and architecture review

- Placement: Git binary classification remains in `kanvibe-git`; app orchestration and GPUI state remain in `kanvibe-app`.
- Simplicity: existing snapshot/save contracts and worker-channel convention were reused; no new crate or abstraction layer.
- Visibility: no symbol was made public solely for tests.
- Constants: sidebar dimensions/step are named.
- Boundary: filesystem/SSH I/O is outside render and outside the GPUI thread.
- Comments: no new explanatory comments; names carry intent.
- Dead code: targeted reference search found production consumers for all new state/functions.

## Skipped/blocked

- Real macOS GPUI event dispatch, screenshot, and S05 MP4 were not run on this Linux host.
- This slice does not authorize Electron removal; Issue #310 requires two unchanged S01–S14 macOS passes plus signing/notarization/update/stabilization gates first.

## Follow-up terminal focus slice

- Overlay close paths now set one pending focus restoration, applied only after the modal stack is empty and only when the current task owns a live terminal entity.
- Delete confirmation is now part of the modal stack, so Escape and pointer cancellation share the same restoration contract.
- Focused source-contract test, all-feature Clippy, portable GPUI type-check, formatter, and diff check exit 0.
- Upstream dependency research verifies 10/10 source links. `gpui-terminal` main still lacks selection/scrollback; open PR #2 addresses only scrolling and has unresolved changes requested. A licensed local patch/fork remains the next implementation slice.
