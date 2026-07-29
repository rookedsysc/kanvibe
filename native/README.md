# KanVibe Native Workspace

This workspace is the Rust + GPUI migration target for the existing Electron app. It is macOS-first for product UI work and keeps Electron, Node, and webview runtimes out of native crates.

## Crates

| Crate | Boundary |
| --- | --- |
| `kanvibe-core` | SQLite schema contract, shared domain constants, seed DB compatibility tests |
| `kanvibe-git` | Git, branch, worktree, and PR command contracts |
| `kanvibe-pty` | PTY/session environment contracts |
| `kanvibe-session` | tmux/zellij dependency policy shared by PTY and bounded process transports |
| `kanvibe-hooks` | Hook server, notification, and QA socket contracts |
| `kanvibe-theme` | Semantic color/status tokens |
| `kanvibe-i18n` | Locale and message catalog contracts |
| `kanvibe-app` | macOS GPUI application boundary |
| `qa-harness` | Native replay harness boundary for `qa/scenarios/S01-S14.json` |

## Dependency Pins

`kanvibe-app` records macOS-only pins for `gpui = "=0.2.2"`, `gpui-component = "=0.5.1"`, and `gpui-component-assets = "=0.5.1"`. The `native-ui` feature is enabled by default so a normal macOS build selects the product UI without a manual feature flag. The dependencies remain target-gated, so Linux CI continues to build the portable contracts and headless boundary.

## Existing Database Transition

Before the native app migrates an existing `kanvibe.db`, it creates an immutable SQLite online-backup beside the database as `kanvibe.db.electron-backup`. This captures committed WAL contents, is integrity-checked before publication, and is never overwritten on later launches. `kanvibe.db.native-transition.json` records whether the runtime database came from the bundled seed or from an existing Electron installation.

The native migration runner uses the same 12 TypeORM migration identifiers as Electron. Legacy bootstrap databases are baselined after missing columns and indexes are repaired; partial TypeORM histories resume only their missing data migrations. All schema and data changes run in one immediate SQLite transaction with a five-second busy timeout.

Runtime writes use the same five-second busy timeout. Task creation computes its per-status display order inside the INSERT statement so concurrent writers cannot reuse an order, and drag/status plus destination reordering commit in one immediate transaction. Failure-injection tests cover zero-timeout exclusive locks, `SQLITE_FULL`, concurrent creates, and reorder-trigger rollback without partial task mutations.

To restore the immutable Electron snapshot while KanVibe is stopped:

```sh
cargo run --manifest-path native/Cargo.toml -p kanvibe-app -- --rollback-electron-db
```

`KANVIBE_DB_PATH` may select an explicit QA or recovery database. Before restoration, the command preserves the current native database as a uniquely named `kanvibe.db.native-before-rollback-*` SQLite snapshot. Both inputs are integrity-checked, and a failed restore attempts to recover the native database automatically.

> [crates.io gpui API](https://crates.io/api/v1/crates/gpui) reports `"0.2.2"`.

> [crates.io gpui-component API](https://crates.io/api/v1/crates/gpui-component) reports `"0.5.1"`.

> [GPUI Component docs](https://longbridge.github.io/gpui-component/) say `"recommend using the git version for now"`, so the Slice 1 macOS UI implementation must re-check whether registry pins or git pins are the safer integration point before enabling the feature.

> [Zed workspace Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/Cargo.toml) contains `"alacritty_terminal"` and pins the Zed Alacritty fork revision recorded in workspace metadata.

> [Zed terminal Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/crates/terminal/Cargo.toml) contains `"gpui.workspace"`, confirming the current Zed terminal path is GPUI plus Alacritty terminal core rather than a webview terminal.

## Current Gate

From the repository root, use the Node-free product commands:

```sh
./kanvibe-native check
./kanvibe-native test
./kanvibe-native build
```

`./kanvibe-native dev`, `package`, `phase5`, and `verify-phase5` expose the
remaining application and macOS evidence paths without Node or pnpm. The
package-manager desktop scripts remain a retained Electron parity baseline,
now also exposed under `legacy:electron:*`; they are not the native product
bootstrap and cannot be removed before the real-macOS stabilization gate.

The first schema test validates the Electron seed database at
`qa/seed/kanvibe-seed.sqlite` without mutating it. Later slices will replace
the CLI-based schema assertion with direct Rust SQLite read models while
keeping the same seed DB as the compatibility fixture.

## Packaging

Linux hosts can verify the portable native contracts and release binary:

```sh
cargo build --release
```

macOS packaging is intentionally Darwin-gated because the product UI enables GPUI through the `native-ui` feature and uses macOS codesign/DMG tooling:

```sh
scripts/package-macos-app.sh
```

The script builds the app and updater helper with `--features native-ui`, derives the product version from the app crate, stages `native/dist/KanVibe.app`, preserves the existing `com.kanvibe.desktop` bundle identity, bundles the message catalogs, seed database, `icon.icns`, and `Contents/Helpers/KanVibeUpdater`, writes `Info.plist`/`PkgInfo`, and creates `native/dist/KanVibe-<version>.dmg`. Use `--skip-sign` to create an unsigned local bundle on macOS, or `--no-dmg` when only the `.app` bundle is needed.

Release packaging is explicit and fails closed when the Developer ID identity,
notarytool keychain profile, DMG, hardened runtime, app/DMG notarization,
stapling, Gatekeeper assessment, clean Git source, 40-character build commit,
or either Apple Rust target is unavailable. The native release policy is one
universal DMG: both app and helper must contain arm64 and x86_64 slices. The
script signs the helper before sealing the app, records the commit in
`KanVibeBuildCommit`, and emits a checksum only after the final DMG staple:

```sh
native/scripts/package-macos-app.sh \
  --release \
  --sign-identity "Developer ID Application: Example (TEAMID)" \
  --notary-profile kanvibe-notary

native/scripts/verify-macos-release.sh
```

`.github/workflows/native-release-candidate.yml` is a manual, non-publishing
candidate job. It expects `MACOS_CERTIFICATE_P12_BASE64`,
`MACOS_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID_IDENTITY`, `APPLE_ID`,
`APPLE_APP_SPECIFIC_PASSWORD`, and `APPLE_TEAM_ID` as GitHub Actions secrets,
uses one temporary keychain for signing and the notarytool profile, uploads
only the DMG/checksum/notary evidence, and deletes the keychain in an
`always()` cleanup step.

The verifier rejects ad-hoc signatures and checks checksum, commit, nested
helper, staged app, and the app inside a read-only mounted DMG. Native fatal
errors and panics append structured version/commit/PID diagnostics under
KanVibe userData in `native-crash.log`, rotating one previous 1 MiB file.
Clean-machine install/update/rollback and published-asset/Homebrew transitions
remain separate release gates.

## Native UI Entrypoint

`kanvibe-app` now includes a macOS-only GPUI entry module at `crates/kanvibe-app/src/native_ui.rs`. The module is compiled only with `#[cfg(all(target_os = "macos", feature = "native-ui"))]` and uses the pinned `gpui`/`gpui-component` entry pattern:

- `Application::new()`
- `gpui_component::init(cx)`
- `WindowOptions`
- `Root::new(view, window, cx)`
- gpui-component `Button`

The native UI renders the seed board as a GPUI root window with project/task counts, all board columns, every visible task card, and semantic project/priority/branch/PR/agent/session/SSH badges. It also owns locale-aware board/settings/pane-layout/task/diff/not-found routing, history navigation, macOS titlebar/menu wiring, new-window and history shortcuts, and visible loading/empty/error shells. Linux verification covers the shared contracts and a temporary GPUI type-check boundary; macOS visual and lifecycle parity still require the Phase 5 screenshot/video run.

### Product Launch Paths

`NativeUiLaunchConfig` resolves product defaults first and treats every `KANVIBE_*` path as a QA-only override, so a packaged `.app` opened from Finder boots without any environment variables:

| Value | Product default | Electron counterpart |
| --- | --- | --- |
| Resource root | `Contents/Resources` when the executable sits in `Contents/MacOS`, otherwise the current directory | `app.getAppPath()` / `process.resourcesPath` |
| Runtime DB | `<userData>/kanvibe.db` — macOS `~/Library/Application Support/KanVibe`, Linux `~/.config/KanVibe` | `getRuntimeDatabasePath()` |
| First-run DB init | copies `resources/database/app.seed.db` from the resource root | `ensureRuntimeDatabaseFile()` |

`scripts/package-macos-app.sh` stages `messages/` and `resources/database/app.seed.db` into `Contents/Resources` so both lookups resolve inside the bundle. The macOS Finder smoke test (launch the signed `.app` with no environment and confirm the board window opens) still belongs to the Phase 5 macOS run.

Scoped launch overrides (QA only):

| Env var | Purpose |
| --- | --- |
| `KANVIBE_REPO_ROOT` | Resource root override for message catalogs and QA fixtures |
| `KANVIBE_DB_PATH` | Exact SQLite DB path for native app launch |
| `KANVIBE_APP_DATA_DIR` | userData directory override; the DB is resolved as `<dir>/kanvibe.db` |
| `KANVIBE_LOCALE` | `ko`, `en`, or `zh`; unrecognized codes fall back to Korean |

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

Every `phase5-macos-run.sh` run also copies
`qa/checklists/terminal-macos.md`, `qa/checklists/updater-macos.md`, and a machine-readable
`evidence-manifest.json` template into its run directory. S03/S04/S13/S14 are
not terminal-parity PASS until the copied checklist contains artifact references
for scroll direction/speed, selection, Cmd+C/V, bracketed paste, tmux/nvim,
focus, resize, restart, and remote SSH/zellij behavior. Release approval also
requires a signed successful update and a forced health-timeout rollback with
unchanged user data. After completing both checklists, verify the run's PASS
reports and non-empty S01-S14 screen/video files:

```sh
native/scripts/verify-phase5-run.sh --run qa/parity/run-024
```

Rollout approval requires two numerically consecutive runs with the same source
commit, scenario-definition digest, and app version:

```sh
native/scripts/verify-phase5-run.sh \
  --run qa/parity/run-025 \
  --previous-run qa/parity/run-024
```

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

The Done transition follows Electron's optimistic-cleanup contract. Because moving a task to Done clears `session_type`, `session_name`, and `worktree_path`, the DB would otherwise lose the only record of what needs cleaning. `update_task_status` and `move_task_to_column` therefore return a `DoneCleanupPlan` carrying the pre-transition snapshot; the caller runs the actual session/worktree cleanup and reports the result through `finish_done_cleanup`, which either keeps the cleared row or rolls the task back to its previous status, session, and worktree. A rollback is skipped when the task has already moved off Done in the meantime. Executing the cleanup commands themselves stays with the session/worktree service layer.

The native branch form now creates the actual local or bounded-SSH worktree, records the deterministic session/worktree identity, and installs provider bindings off the GPUI thread. If the DB write fails after local Git creation, the new worktree and branch are removed before the error is returned.

Slice 2 QA report:

- `../qa/parity/slice-2/board-interactions.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- board-interactions --repo-root .. --output ../qa/parity/slice-2/board-interactions.json
```

## Slice 3 Status

`kanvibe-app` now models task-detail route localization, existing-window focus decisions, dock shortcut labels/matching, and PR slot insertion. `kanvibe-pty` includes the local shell environment sanitizer used for terminal sessions, and `kanvibe-core` persists sidebar default/hint AppSettings.

For project tasks without session metadata, task detail offers explicit tmux and zellij connect actions and persists the chosen identity before starting the PTY. Tasks without a project show a no-terminal state rather than launching an untracked shell.

Slice 3 QA report:

- `../qa/parity/slice-3/task-detail-pty-dock.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- task-detail --repo-root .. --output ../qa/parity/slice-3/task-detail-pty-dock.json
```

## Slice 4 Status

`kanvibe-git` now covers local and bounded-SSH Git command execution, changed-file listing, byte-preserving original/current file reads, conflict-guarded editor saves, managed worktree path derivation, and guarded branch/worktree cleanup. Diff numstat identifies binary files before content loading, so GPUI disables unsafe text preview/editing for those paths. The Diff route runs Git/SSH loads and saves off the UI thread, groups files by folder, records viewed selections, exposes bounded file-tree resizing, and shows loading/retry/dirty/saving/conflict state. Remote editor writes accept relative normal paths only, reject symbolic-link targets and canonical parent escapes, and cap each expected/new payload at 64 KiB so content transport stays below conservative SSH argv limits. `kanvibe-core` persists branch-from-task metadata onto the existing SQLite schema.

Slice 4 QA report:

- `../qa/parity/slice-4/git-diff-worktree.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- git-diff --repo-root .. --output ../qa/parity/slice-4/git-diff-worktree.json
```

## Slice 5 Status

`kanvibe-hooks` now models scoped hook server URLs, hook installer URL extraction, expected/reachable hook validation, and an Electron-compatible persistent notification store in `app_settings.app_notifications`. The store preserves camelCase JSON, newest-first 100-item retention, the four-second in-process dedupe boundary, and individual/all read transitions. `kanvibe-app` owns the process service, GPUI notification panel, hook/background publishers, one-shot task/review activation routing, and a bounded macOS `UNUserNotificationCenter` adapter. Denied or unsupported system delivery leaves the in-app record intact. A real signed app notification click is still an S11 macOS gate.

The native release checker preserves the release-page workflow and enables Install only when the selected stable release has one exact `KanVibe-<version>.dmg` asset with bounded size and a GitHub SHA-256 digest. Installation downloads off the UI thread, verifies size/digest, Developer ID identity, stapled notarization, Gatekeeper acceptance, bundle ID/version, and the running app's TeamIdentifier before staging. A separately signed `KanVibeUpdater` helper performs sibling renames after the app exits, retains the old bundle until the new version writes a nonce-bound health acknowledgement, and restores/relaunches the old bundle on launch or health timeout. Portable tests prove URL/digest/journal/rollback contracts; a published signed asset and forced rollback still require real-macOS Phase 5 evidence.

The dedicated `kanvibe-ai` crate owns Claude/Codex/Gemini/OpenCode session discovery, worktree matching, query/role filtering, pagination, and message detail parsing. The native QA harness emits S11 evidence for notifications, hooks, AI sessions, and the background sync setting.

Slice 5 QA report:

- `../qa/parity/slice-5/notifications-hooks-ai.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- notifications-hooks --repo-root .. --output ../qa/parity/slice-5/notifications-hooks-ai.json
```

## Slice 6 Status

`kanvibe-core` now persists typed app settings and pane layout configs against the existing SQLite tables. `kanvibe-session` owns the single tmux/zellij check/install policy, `kanvibe-git` executes that policy through bounded local or SSH transports, and `kanvibe-pty` re-exports the compatibility contract while modeling tmux pane layout commands and zellij KDL generation. `kanvibe-app` connects dependency check/install/retry state to Task Detail and refuses to bind or spawn a persistent terminal until the selected tool is available.

Task Detail also exposes a real Hooks dock target and provider-by-provider
Claude, Codex, Gemini, and OpenCode readiness. Readiness requires every
generated provider file, the expected local/SSH callback URL, and a reachable
native hook health endpoint. Check and preservation-safe install/repair work is
off the GPUI thread; SSH callback health accepts only a parsed IP address and
port and runs through the bounded transport.

The native Pane Layout route mirrors the Electron editor's six direct layout
choices instead of cycling rows. It preserves commands by pane index when the
shape changes, labels each pane by its geometric position, creates overrides
for projects that previously inherited the global layout, and keeps the
project reset-to-global path.

Task Detail now consumes both sidebar settings: each window starts from the
persisted default collapsed value, exposes project/branch/session/SSH context
in a collapsible native sidebar, and persists the one-time collapsed-sidebar
hint dismissal. The existing Done confirmation also persists its
don't-ask-again choice before applying the status mutation.

The native app also owns one cancelable background-sync worker for its full process lifetime. It reads the persisted enable/interval settings, reuses the local and remote project/worktree synchronizers, syncs active-task pull requests and branch fast-forwards, wakes GPUI windows only for board changes, exposes the last run in Settings, and joins the worker during shutdown. Pull-request sync uses a bounded `gh` contract locally or through the task's existing SSH identity, persists only changed URLs, selects the latest updated PR, and deduplicates merged-PR events for the worker lifetime. Review-worthy merged PRs, new worktrees, pull outcomes, and failures accumulate until the user dismisses the GPUI review; merged tasks are selected for cleanup by default. Settings can check or install `gh` on the local machine and each registered SSH target without blocking the UI, caches successful availability checks for 60 seconds, and blocks repeated non-transport remote install failures until restart.

Remote projects can now be registered from Settings with an SSH host alias. The native Git adapter uses bounded OpenSSH commands with `ControlMaster=auto`, a private reusable control path, connection-health options, a narrow HOME/PATH/SSH-agent/locale/GitHub-auth child environment, typed timeout/transport/output failures, and one transport retry. Background sync discovers remote worktrees and preserves the SSH host on their task bindings without matching same-path local orphan tasks. Done/delete cleanup, native diff/editor, provider hook installation, GitHub PR lookup, active-branch pull, and read-only AI history access dispatch through the same exact SSH identity. Remote hooks preserve existing Claude/Gemini/Codex settings and task fan-out, derive the callback address from `SSH_CONNECTION`, reject symlink escapes, and update the common Git exclude idempotently. Remote AI access is confined to canonical paths under the remote HOME, refuses symlink escapes, bounds file inventory/content, and runs only fixed read-only OpenCode queries.

Task detail loads provider history off the GPUI thread and renders source availability, provider filters, body search, role filters, session pagination, and message detail. Local and remote readers share the same typed parsers and preserve the Electron formats: Claude/Codex JSONL, Gemini chat JSON/project metadata, and OpenCode SQLite.

Settings also supports live system/light/dark theme changes and physical capture of the task-search shortcut. The full `ko`, `en`, and `zh` message catalogs are validated for identical string paths and placeholders and are available to GPUI; replacing every remaining fallback label is still in progress. Modal Escape is scoped so it does not steal terminal input, and the release dialog uses a tab group. The pinned GPUI versions do not expose a VoiceOver semantic label/role API in their current source, so accessibility parity remains unverified until an API path is selected and tested on macOS.

Slice 6 QA report:

- `../qa/parity/slice-6/settings-layout-remote.json`

Generate it from this directory:

```sh
cargo run -p qa-harness -- settings-layout-remote --repo-root .. --output ../qa/parity/slice-6/settings-layout-remote.json
```
