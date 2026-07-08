# KanVibe Native Workspace

This workspace is the Rust + GPUI migration target for the existing Electron app. It is macOS-first for product UI work and keeps Electron, Node, and webview runtimes out of native crates.

## Crates

| Crate | Boundary |
| --- | --- |
| `kanvibe-core` | SQLite schema contract, shared domain constants, seed DB compatibility tests |
| `kanvibe-git` | Git, branch, worktree, and PR command contracts |
| `kanvibe-pty` | PTY/session environment contracts |
| `kanvibe-hooks` | Hook server, notification, and QA socket contracts |
| `kanvibe-theme` | Semantic color/status tokens |
| `kanvibe-i18n` | Locale and message catalog contracts |
| `kanvibe-app` | macOS GPUI application boundary |
| `qa-harness` | Native replay harness boundary for `qa/scenarios/S01-S14.json` |

## Dependency Pins

`kanvibe-app` records macOS-only optional pins for `gpui = "=0.2.2"`, `gpui-component = "=0.5.1"`, and `gpui-component-assets = "=0.5.1"`. The feature is intentionally disabled by default so Linux CI can run core tests in this workspace while product UI code remains macOS-first.

> [crates.io gpui API](https://crates.io/api/v1/crates/gpui) reports `"0.2.2"`.

> [crates.io gpui-component API](https://crates.io/api/v1/crates/gpui-component) reports `"0.5.1"`.

> [GPUI Component docs](https://longbridge.github.io/gpui-component/) say `"recommend using the git version for now"`, so the Slice 1 macOS UI implementation must re-check whether registry pins or git pins are the safer integration point before enabling the feature.

> [Zed workspace Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/Cargo.toml) contains `"alacritty_terminal"` and pins the Zed Alacritty fork revision recorded in workspace metadata.

> [Zed terminal Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/crates/terminal/Cargo.toml) contains `"gpui.workspace"`, confirming the current Zed terminal path is GPUI plus Alacritty terminal core rather than a webview terminal.

## Current Gate

Run from this directory:

```sh
cargo test --workspace
cargo build --workspace
```

The first schema test validates the Electron seed database at `../qa/seed/kanvibe-seed.sqlite` without mutating it. Later slices will replace the CLI-based schema assertion with direct Rust SQLite read models while keeping the same seed DB as the compatibility fixture.

## Packaging

Linux hosts can verify the portable native contracts and release binary:

```sh
cargo build --release
```

macOS packaging is intentionally Darwin-gated because the product UI enables GPUI through the `native-ui` feature and uses macOS codesign/DMG tooling:

```sh
scripts/package-macos-app.sh
```

The script builds `kanvibe-app` with `--features native-ui`, stages `native/dist/KanVibe.app`, writes `Info.plist` and `PkgInfo`, ad-hoc signs with `codesign --sign -` when available, and creates `native/dist/KanVibe-0.1.0.dmg` with `hdiutil create -format UDZO`. Use `--skip-sign` to create an unsigned local bundle on macOS, or `--no-dmg` when only the `.app` bundle is needed.

## Native UI Entrypoint

`kanvibe-app` now includes a macOS-only GPUI entry module at `crates/kanvibe-app/src/native_ui.rs`. The module is compiled only with `#[cfg(all(target_os = "macos", feature = "native-ui"))]` and uses the pinned `gpui`/`gpui-component` entry pattern:

- `Application::new()`
- `gpui_component::init(cx)`
- `WindowOptions`
- `Root::new(view, window, cx)`
- gpui-component `Button`

The first native UI slice renders the seed board as a GPUI root window with project/task counts, all board columns, first-card titles, and semantic color tokens. Linux verification covers the shared `NativeUiRenderSpec`; macOS visual parity still requires the Phase 5 GPUI screenshot/video run.

Scoped launch overrides:

| Env var | Purpose |
| --- | --- |
| `KANVIBE_REPO_ROOT` | Repository root for message catalogs and QA fixtures |
| `KANVIBE_DB_PATH` | SQLite DB path for native app launch |
| `KANVIBE_LOCALE` | `ko` or `en`; falls back to Korean |

Linux-safe checks:

```sh
cargo test -p kanvibe-app
cargo check -p kanvibe-app --features native-ui
```

On macOS, use the bundle script to compile the actual GPUI module:

```sh
scripts/package-macos-app.sh
```

## Phase 5 macOS Runner

The Phase 5 handoff runner creates a numbered parity run directory, packages the release app, builds the debug `native-ui` binary used by the QA socket, and writes native protocol/replay/app-replay/visual-parity/performance/full-parity artifacts into that run:

```sh
scripts/phase5-macos-run.sh --window-id <numeric-window-id> --ffmpeg /opt/homebrew/bin/ffmpeg
```

The runner exports `KANVIBE_QA_ARTIFACT_ROOT=qa/parity/<run-id>` so native replay screenshots and videos target `qa/parity/<run-id>/screens/` and `qa/parity/<run-id>/videos/`. It requires a numeric macOS window id through `--window-id` or `KANVIBE_QA_WINDOW_ID`; pass `--skip-sign`, `--no-dmg`, or `--skip-package` for local iteration. Non-macOS hosts exit `78` by design.

## QA Control Protocol

The native app defines a debug-only Unix socket protocol for Phase 5 automation. When a debug build is launched with `KANVIBE_QA_SOCKET=<path>`, the app can open a line-delimited JSON control channel. Release user builds keep the control channel disabled.

On macOS, screenshot capture uses `screencapture -x -l <window-id> <path>`. Video capture records a `screencapture` frame sequence and encodes it with `ffmpeg`; set `KANVIBE_QA_FFMPEG=<absolute-ffmpeg-path>` when the debug app is launched from a narrow environment without `PATH`. Both screenshot and video capture require the scoped `KANVIBE_QA_WINDOW_ID=<numeric-id>` environment variable. Linux hosts continue to report capture as blocked.

Protocol commands:

| Command | Purpose |
| --- | --- |
| `ping` | Health check |
| `queryElement` | Check a known UI element id and optional text |
| `queryText` | Read text for a known UI element id |
| `syntheticClick` | Protocol boundary for semantic element click injection |
| `syntheticKey` | Protocol boundary for keyboard input injection |
| `syntheticMouse` | Protocol boundary for mouse input injection |
| `dumpScreenshot` | macOS `screencapture` capture for the configured `KANVIBE_QA_WINDOW_ID` |
| `startVideoCapture` | Start macOS frame capture for a scenario MP4 artifact |
| `stopVideoCapture` | Stop frame capture and encode the scenario MP4 with `ffmpeg` |
| `dbSnapshot` | Read board/project/status counts from the native spec |

Current Linux verification proves the protocol, scenario mapping, replay-plan generation, Unix socket client round trip, per-scenario seed DB copy preparation under `../qa/parity/native-db/`, per-scenario native video artifact planning under `../qa/parity/native-videos/`, full replay command execution over a debug socket, and the macOS screenshot/video command contracts. The execution report separates transport success from current behavior blockers. Actual GPUI event dispatch and capture output still require the macOS Phase 5 runtime.

Generate the scenario mapping, replay-plan, replay-execution, external native-app launch, external native-app replay, visual parity, and performance artifacts from this directory:

```sh
cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json
cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json
cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json
cargo run -p qa-harness -- qa-app-launch --repo-root .. --output ../qa/parity/slice-qa-control/qa-app-launch.json
cargo run -p qa-harness -- qa-app-replay --repo-root .. --output ../qa/parity/slice-qa-control/qa-app-replay.json
cargo run -p qa-harness -- native-visual-parity --repo-root .. --output ../qa/parity/slice-qa-control/native-visual-parity.json
cargo run -p qa-harness -- native-performance --repo-root .. --output ../qa/parity/slice-qa-control/native-performance.json
```

`qa-app-launch` starts the real `kanvibe-app` binary with a narrow environment containing `KANVIBE_REPO_ROOT`, `KANVIBE_DB_PATH`, `KANVIBE_LOCALE`, and `KANVIBE_QA_SOCKET`. On Linux it currently records a blocker because the product UI is macOS-gated and the binary exits as a headless scaffold before opening the QA socket.

`qa-app-replay` starts the real `kanvibe-app` binary once per scenario with that scenario's copied seed DB, waits for the debug QA socket, and then reuses the replay plan against the external process. On Linux it records the same headless-stub blocker before socket replay begins; on macOS it is the handoff point for real GPUI replay, screenshots, and videos. If `KANVIBE_QA_ARTIFACT_ROOT` is set, generated capture commands use `<artifact-root>/screens` and `<artifact-root>/videos`; otherwise they keep the standalone `../qa/parity/native-screens/` and `../qa/parity/native-videos/` paths.

`native-visual-parity` inventories `qa/baseline/MANIFEST.md`, scenario-declared screen/video artifacts, and the native screenshot/video paths expected from the replay plan. It reports missing baseline files as failures and missing native captures as blockers. It does not mark visual parity as passed; after macOS replay creates the native files, the Phase 5 QA report still needs a structural side-by-side review for layout, text, color tokens, and spacing hierarchy.

`native-performance` compares native artifacts against the Phase 1 performance targets. It can verify generated bundle/release artifact sizes immediately, and keeps cold start, idle memory, and terminal scroll FPS blocked until the macOS GPUI runtime run records real measurements.

## Slice 1 Status

The workspace now opens the Electron seed database read-only through `kanvibe-core`, groups tasks into the five board columns, loads ko/en board labels from the existing message catalogs, and builds a read-only board shell model in `kanvibe-app`. The macOS GPUI feature remains target-gated; final visual screenshot parity is still required in Phase 5 on a macOS runtime.

Slice 1 QA reports:

- `../qa/parity/slice-1/read-only-board-en.json`
- `../qa/parity/slice-1/read-only-board-ko.json`

Generate them from this directory:

```sh
cargo run -p qa-harness -- readonly-board --repo-root .. --locale en --output ../qa/parity/slice-1/read-only-board-en.json
cargo run -p qa-harness -- readonly-board --repo-root .. --locale ko --output ../qa/parity/slice-1/read-only-board-ko.json
```

## Slice 2 Status

`kanvibe-core` now includes board write contracts for create, edit, status move, drag-style column move, reorder, delete, project color update, and done pagination. These operate on copied SQLite fixtures in tests and do not mutate the shared seed DB.

Slice 2 QA report:

- `../qa/parity/slice-2/board-interactions.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- board-interactions --repo-root .. --output ../qa/parity/slice-2/board-interactions.json
```

## Slice 3 Status

`kanvibe-app` now models task-detail route localization, existing-window focus decisions, dock shortcut labels/matching, and PR slot insertion. `kanvibe-pty` includes the local shell environment sanitizer used for terminal sessions, and `kanvibe-core` persists sidebar default/hint AppSettings.

Slice 3 QA report:

- `../qa/parity/slice-3/task-detail-pty-dock.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- task-detail --repo-root .. --output ../qa/parity/slice-3/task-detail-pty-dock.json
```

## Slice 4 Status

`kanvibe-git` now covers local Git command execution, changed-file listing, original/current file reads, editor saves, managed worktree path derivation, and branch worktree creation. `kanvibe-core` persists branch-from-task metadata onto the existing SQLite schema.

Slice 4 QA report:

- `../qa/parity/slice-4/git-diff-worktree.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- git-diff --repo-root .. --output ../qa/parity/slice-4/git-diff-worktree.json
```

## Slice 5 Status

`kanvibe-hooks` now models scoped hook server URLs, hook installer URL extraction, expected/reachable hook validation, AI provider session aggregation, notification center behavior, board event notifications, and visible hook provider status. The native QA harness emits S11 evidence for notifications, hooks, AI sessions, and the background sync setting.

Slice 5 QA report:

- `../qa/parity/slice-5/notifications-hooks-ai.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- notifications-hooks --repo-root .. --output ../qa/parity/slice-5/notifications-hooks-ai.json
```

## Slice 6 Status

`kanvibe-core` now persists typed app settings and pane layout configs against the existing SQLite tables. `kanvibe-pty` models tmux/zellij dependency checks, install command policy, tmux pane layout commands, and zellij KDL generation. `kanvibe-app` models settings shell state and internal-window route focusing policy.

Slice 6 QA report:

- `../qa/parity/slice-6/settings-layout-remote.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- settings-layout-remote --repo-root .. --output ../qa/parity/slice-6/settings-layout-remote.json
```
