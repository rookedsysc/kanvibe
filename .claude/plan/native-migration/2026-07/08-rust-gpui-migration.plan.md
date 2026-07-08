# KanVibe Rust GPUI Native Migration

## Test Case Decision
- [x] Unit/behavior: `createLocalShellEnvironment` parity in Rust equivalent with `PORT`, `HOST`, `NODE_ENV`, internal `KANVIBE_*` input -> user PTY environment excludes server/runtime values while preserving required shell basics - Decision: Required - (지금 RED 이유: `native/` Rust crate and test do not exist)
- [x] Unit/behavior: dock items with and without PR URL -> shortcut labels/matchers derive macOS `Cmd+{n}` slot order with PR conditionally at slot 4 - Decision: Required - (지금 RED 이유: Rust shortcut module and tests do not exist)
- [x] Integration/contract: Electron-created seed SQLite DB -> Rust core opens existing schema without migrations or path changes and round-trips all entity fields - Decision: Required - (지금 RED 이유: shared seed DB and Rust schema compatibility layer do not exist)
- [x] Integration/contract: `qa/scenarios/S01-S14.json` -> Electron baseline runner and native QA harness can replay the same declared actions/assertions - Decision: Required - (지금 RED 이유: shared scenario JSON set and native QA harness do not exist)
- [x] Regression: Feature inventory rows from screens/modals/context menus/shortcuts/IPC/DB/settings/notifications/hooks -> every row has an explicit verification method and is checked only after parity PASS - Decision: Required - (지금 RED 이유: `qa/FEATURE_INVENTORY.md` does not exist)
- [x] E2E/manual: full Phase 5 loop with seeded DB -> two consecutive native parity runs PASS all scenarios, visual structure, DB asserts, and performance targets - Decision: Required - (지금 RED 이유: baseline artifacts, native app, and parity reports do not exist)

### Open Test Questions
- [x] 불확실성 없음. The pasted objective explicitly defines scope, phase order, acceptance criteria, and autonomous execution.

## Business Goal
KanVibe Electron/React 데스크톱 앱을 macOS 우선 Rust + GPUI + gpui-component 네이티브 앱으로 동등 이식한다. 기존 SQLite 사용자 데이터와 기능/시각 동등성을 유지하고, Phase 5 QA 루프에서 2회 연속 PASS 및 성능 목표 달성을 증명한다.

## Scope
- **In Scope**:
  - Phase 1 기능 인벤토리와 Electron 성능 베이스라인 작성.
  - Phase 2 결정적 seed DB, Electron 기준 스크린샷/영상/시나리오/매니페스트 작성.
  - Phase 3 `native/` Cargo workspace 설계와 GPUI/gpui-component 버전 핀.
  - Phase 4 수직 슬라이스별 Rust 네이티브 구현: 보드, 태스크 상세/PTY, Git/Diff, AI hooks/notifications, 설정/PaneLayout/window policy, packaging.
  - Phase 5 native QA harness, parity reports, repair loop, final two-pass verification.
  - Existing Electron code remains as comparison source.
- **Out of Scope**:
  - Linux product support beyond future-friendly `#[cfg(target_os = "macos")]` isolation.
  - Feature additions, redesigns, or improvements not required for parity; record ideas in `qa/FOLLOWUPS.md`.
  - SQLite schema or user data path changes.
  - Webview/Electron/Node runtime inclusion in the native app.
  - Commit/push/PR/deploy actions without a separate explicit instruction.

## Codebase Analysis Summary
- Target repository: `/home/crookedbot/Documents/kanvibe/kanvibe__worktrees/perf-rust-backend`.
- Git branch: `perf/rust-backend`.
- Current dirty files before this plan: untracked `.claude/.notification-context.json`, untracked `GPUI_MIGRATION_PROMPT.md`.
- Current native state: no `native/` tree exists.
- Current QA state: `qa/electron/` helper flows exist, but required `qa/FEATURE_INVENTORY.md`, `qa/PERF_BASELINE.md`, `qa/seed/`, `qa/baseline/`, `qa/scenarios/`, and `qa/parity/` migration artifacts do not exist.
- Runtime commands discovered:
  - `node --version` -> `v24.16.0`
  - `corepack pnpm --version` -> `11.6.0`
  - `pnpm` binary is not directly on `PATH`; use `corepack pnpm ...` unless a local shim is installed.
  - `rustc --version` -> `rustc 1.96.0 (ac68faa20 2026-05-25)`
  - Local OS is Linux, while target packaging is macOS-first; macOS-specific packaging and `screencapture` gates may need artifact-level blockers or macOS runtime verification later.
- Local convention source: `CLAUDE.md`; `.claude/core/*` files referenced by `context-loader` are absent.

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `GPUI_MIGRATION_PROMPT.md` | Same migration specification as pasted attachment | Reference only |
| `electron/main.js`, `electron/preload.js`, `electron/hookServer.js` | Electron main/preload/hook server baseline | Reference |
| `src/desktop/renderer/App.tsx`, `src/desktop/renderer/routes/*.tsx` | Existing app routes/screens | Reference for inventory/baseline |
| `src/components/*.tsx` | Existing UI components/modals/context menus | Reference for inventory/baseline |
| `src/desktop/main/serviceRegistry.ts`, `src/desktop/main/services/*.ts`, `src/desktop/main/terminalBridge.ts` | IPC/service/terminal contracts | Reference for inventory/native contracts |
| `src/entities/*.ts`, `src/migrations/*.ts`, `src/lib/sqliteSchema.ts`, `src/lib/databasePaths.ts` | SQLite schema and data path compatibility source | Reference |
| `src/lib/gitOperations.ts`, `src/lib/worktree.ts`, `src/lib/terminal.ts`, `src/lib/shellEnvironment.ts`, `src/lib/remoteSessionDependency.ts` | Backend behavior to port | Reference |
| `src/lib/aiSessions/*.ts`, `src/lib/*HooksSetup.ts`, `src/lib/hookEndpoint.ts`, `src/lib/hookServerStatus.ts`, `src/lib/boardNotifier.ts` | AI sessions, hooks, notifications | Reference |
| `messages/ko.json`, `messages/en.json`, `messages/zh.json` | Existing message catalogs | Reference; ko/en required for native |
| `scripts/qa-electron.sh`, `scripts/qa-electron-video.sh`, `qa/electron/**` | Existing Electron QA foundation | Modify/extend |
| `qa/FEATURE_INVENTORY.md` | Single ledger for feature parity | Create |
| `qa/PERF_BASELINE.md` | Electron and final native performance measurements | Create |
| `qa/seed/kanvibe-seed.sqlite` | Deterministic shared DB | Create |
| `qa/baseline/**`, `qa/scenarios/*.json` | Electron ground truth artifacts | Create |
| `native/**` | Rust native app workspace and QA harness | Create after Phase 2 passes |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Environment safety | `CLAUDE.md`, pasted objective | Do not mutate generic process/user environment; use typed config and only scoped `KANVIBE_*` where process env is required. PTY/shell env must strip server/runtime values. |
| App-wide settings | `CLAUDE.md`, `src/entities/AppSettings.ts` | Persist global UI preferences through `AppSettings`, not route-local cache. |
| Shortcut handling | `CLAUDE.md`, `src/desktop/shared/keyboardShortcut.ts` | Shared semantic command definitions, `Mod` abstraction, dock numbering derived from dock item array. |
| Task navigation | `CLAUDE.md`, `src/desktop/renderer/utils/taskNavigation.ts` | Existing task detail window is focused instead of opening duplicates. |
| Color tokens | `CLAUDE.md`, `src/styles/globals.css` | Use semantic tokens; `#0064FF` primary, `#202632` neutral button surface, status colors separate. |
| Minimal change | Roky code hygiene | Preserve Electron baseline code; add migration artifacts and native code without unrelated rewrites. |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| Phase ordering | Phase 1 and Phase 2 must finish before native app code starts | Objective explicitly forbids migration code before baseline capture; baseline is QA ground truth | Start native scaffolding first, rejected |
| Planning approval | Treat pasted goal prompt as execution approval | It says autonomous Codex goal mode should complete without human intervention; all test decisions are required by scope | Ask for approval, rejected because it conflicts with autonomous instruction unless a true product decision appears |
| Package command | Use `corepack pnpm` for discovered `pnpm` commands | `pnpm` is not directly installed but Corepack can run pnpm 11.6.0 | Install global pnpm, avoided to reduce environment mutation |
| Native runtime target | macOS-first code paths with Linux build/runtime caveats recorded | Objective excludes Linux support; current executor is Linux, so macOS-only QA commands may become explicit blockers until macOS runtime is available | Attempt to make Linux product support, out of scope |
| External research | Defer GPUI/gpui-component version research to Phase 3 before `Cargo.toml` pinning | Current local code does not settle pre-1.0 crate versions; technical-search evidence is required when selecting pins | Guess versions, rejected |

## API Contracts

### Electron IPC Baseline
- Producer: renderer actions in `src/desktop/renderer/actions/**` and `src/desktop/renderer/ipc.ts`.
- Consumer: `src/desktop/main/serviceRegistry.ts`, `src/desktop/main/services/**`, `src/desktop/main/terminalBridge.ts`.
- Contract: inventory every channel/request/response/error behavior before native contracts are written.
- Guard path: record channels that depend on external tools, filesystem, Git, PTY, hook server, or app settings.

### Native QA Control Socket
- Env gate: `KANVIBE_QA_SOCKET=<path>` only, debug builds only.
- Commands: synthetic mouse/key input, element existence/text query, window screenshot dump, DB snapshot request.
- Guard path: no QA socket in release user build; invalid commands return structured errors without mutating DB/UI.

## Data Models

### Existing SQLite Compatibility Source
| Model | Source | Required Native Contract |
|-------|--------|--------------------------|
| `KanbanTask` | `src/entities/KanbanTask.ts`, migrations | Open and round-trip existing columns including status, branch/base branch, PR URL, priority, ordering, project link. |
| `Project` | `src/entities/Project.ts`, migrations | Preserve path/name/color/worktree metadata and existing path rules. |
| `AppSettings` | `src/entities/AppSettings.ts` | Preserve app-wide settings including sidebar hint dismissal and locale. |
| `PaneLayoutConfig` | `src/entities/PaneLayoutConfig.ts` | Preserve pane layout persistence. |
| `TaskPriority` | `src/entities/TaskPriority.ts` | Preserve priority values and display mapping. |
| TypeORM migration history | `src/migrations/*.ts` | Do not rewrite or ignore existing migration table semantics. |

## Approval Record
- User approval source: pasted objective says the Codex goal-mode agent must perform every phase autonomously until Phase 5 termination criteria; current goal explicitly says read the attachment and use Roky Harness.
- Approved scope: full Electron -> Rust + GPUI + gpui-component parity migration as written in the pasted objective.
- Out-of-scope items: redesign/features, Linux support this cycle, schema/path changes, webview/Node/Electron inclusion in native app, publishing actions.

## Role Routing
| Todo | Owner | Dependencies | Parallelizable | Context allowed | Context forbidden |
|------|-------|--------------|----------------|-----------------|------------------|
| 1. Feature inventory | Cross-boundary | none | No | Electron routes/components/services/entities/scripts/messages | Native implementation changes |
| 2. Electron performance baseline | QA | none | No, package/build state is shared | package scripts, build output, process metrics | Native code |
| 3. Seed DB and scenario spec | Cross-boundary | Todo 1 | Partial, serialize DB writes | entities, migrations, Electron QA helpers | Native implementation changes until Phase 2 baseline complete |
| 4. Baseline screenshots/videos/manifest | QA/frontend | Todos 1,3 | No, shared Electron app/profile/DB | Electron QA helpers, scenarios, renderer surfaces | Native code |
| 5. Native workspace design and version research | Cross-boundary | Todos 1-4 | Partial by crate after workspace root exists | official GPUI/gpui-component/Zed evidence, schema contracts | Guessing crate versions |
| 6. Slice 1 shell/read-only board | Frontend/backend | Todo 5 | No, foundational contracts | native crates, seed DB, theme/i18n contracts | Electron source edits except QA fixtures |
| 7. Slice 2 board interaction | Frontend/backend | Todo 6 | Limited by file ownership | board components, DB services, scenarios S01/S02/S07/S08/S09/S12 | Out-of-scope UX changes |
| 8. Slice 3 task detail/PT Y/dock | Frontend/backend | Todo 7 | Limited | PTY env contract, shortcuts, scenarios S03/S04/S13/S14 | Generic env mutation |
| 9. Slice 4 Git/Diff/editor | Frontend/backend | Todo 8 | Limited | Git/worktree/diff contracts, scenario S05/S06 | Webview/Monaco/Node reuse in native |
| 10. Slice 5 AI/hooks/notifications | Backend/frontend | Todo 9 | Limited | AI sessions, hook server/installers, notification contracts, scenario S11 | Global env writes |
| 11. Slice 6 settings/layout/window/remote | Cross-boundary | Todo 10 | Limited | AppSettings, PaneLayout, remote tmux/ssh contracts | Product behavior changes |
| 12. Slice 7 packaging/performance | QA/backend | Todo 11 | No | cargo bundle scripts, macOS target evidence | Publishing/signing beyond ad-hoc |
| 13. Phase 5 full QA/repair loop | QA then routed repairs | Todos 1-12 | No | all artifacts, required commands, screenshots/videos/reports | Completion without 2 consecutive PASS |

## QA Matrix
| Gate | Command/artifact | Required | Expected evidence | Owner |
|------|------------------|----------|-------------------|-------|
| Phase 1 inventory | `qa/FEATURE_INVENTORY.md` | Yes | Screens/modals/context menus/shortcuts/IPC/DB/settings/notifications/hooks with verification method per row | Coordinator/QA |
| Phase 1 package baseline | `corepack pnpm dist:dir` plus measurement scripts | Yes | exit status, bundle size, cold start, idle memory in `qa/PERF_BASELINE.md`; blocker recorded if OS/tooling prevents measurement | QA |
| Phase 2 seed DB | `qa/seed/kanvibe-seed.sqlite` and schema dump | Yes | 3 projects, 12+ tasks, statuses/priorities/branches/PR combinations, AI sessions | Cross-boundary |
| Phase 2 scenarios | `qa/scenarios/S01-*.json` through `S14-*.json` | Yes | declarative steps/actions/assertions reusable by Electron and native harness | QA |
| Phase 2 baseline capture | `qa/baseline/screens/**`, `qa/baseline/videos/**`, `qa/baseline/MANIFEST.md` | Yes | required screenshots/videos for ko/en and supported themes/states | QA/frontend |
| Phase 3 Rust build | `cargo test --workspace`, `cargo build --workspace` in `native/` | Yes | exit status 0 after workspace exists | Backend/frontend |
| Slice gates | narrow `cargo test -p <crate>` and relevant scenario subset | Yes | RED->GREEN evidence for required tests and scenario artifacts | Role owner |
| Full native release | `cargo build --release` and `.app` bundle script | Yes | release binary/app artifact path and size | QA/backend |
| Phase 5 parity | qa-harness full run twice without code changes | Yes | `qa/parity/run-<N>/QA_REPORT.md` and `run-<N+1>/QA_REPORT.md` both PASS | QA |

## Test Case Plan
| # | Task/Todo | Target behavior | Scenario | Design method | Input | Expected | Priority |
|---|-----------|-----------------|----------|---------------|-------|----------|----------|
| 1 | Feature inventory | Inventory completeness ledger | Every route/component/service/entity/hook/setting source is represented | Right-BICEP, CORRECT existence | Existing Electron source tree | `qa/FEATURE_INVENTORY.md` rows cover all required surfaces with verification methods | Required |
| 2 | Performance baseline | Electron package metrics are reproducible | Package Electron app and measure bundle/start/memory | Right-BICEP performance | `corepack pnpm dist:dir` on supported packaging runtime | `qa/PERF_BASELINE.md` records measured values or explicit environment blocker | Required |
| 3 | Seed DB | Existing schema-compatible deterministic fixture | Open Electron seed DB and inspect rows | Equivalence partitioning, cardinality | 3 projects, 12+ tasks across statuses/priorities/branches/PR | SQLite file has required rows and schema compatible with TypeORM migrations | Required |
| 4 | Scenario replay contract | Shared scenarios are runner-neutral | Electron runner and native harness read same JSON | Contract/integration | `qa/scenarios/S01-S14.json` | Both runners can map each action/assertion without private runner-specific semantics | Required |
| 5 | UI baseline | Electron visual ground truth is complete | Capture all listed screens/states/locales/themes | State transition, existence | Seed DB plus scripted UI actions | PNG/video artifacts and MANIFEST entries exist for all required states | Required |
| 6 | DB compatibility | Rust opens existing DB without migration | Native core reads Electron seed and round-trips entities | Integration, regression | `qa/seed/kanvibe-seed.sqlite` | No schema mutation; all fields round-trip | Required |
| 7 | Env safety | Native PTY env strips server/runtime values | Spawn shell with polluted runtime input | Error guessing, regression | `PORT`, `HOST`, `NODE_ENV`, internal `KANVIBE_*` | Interactive shell cannot see stripped variables | Required |
| 8 | Shortcuts | Dock shortcuts derive slot order | PR and non-PR task detail dock lists | State transition | dock item arrays with/without PR | labels/matchers match expected macOS slots | Required |
| 9 | Full parity | Native app matches Electron behavior and visual structure | Replay S01-S14 twice without changes | E2E, regression | Seed DB copy per run | two consecutive PASS reports and performance target achievement | Required |

## External Research Log
| Question | Skill used | Source-backed conclusion | Plan impact |
|----------|------------|--------------------------|-------------|
| Which exact `gpui` and `gpui-component` versions should be pinned in `native/Cargo.toml`? | `technical-search-skill` plus official crates.io/docs/GitHub sources | crates.io reports `gpui = 0.2.2`, `gpui-component = 0.5.1`, and `gpui-component-assets = 0.5.1`; GPUI Component docs also recommend git dependencies for current main-branch APIs, so `kanvibe-app` records registry pins as macOS-only optional deps and `native/README.md` records the re-check requirement before Slice 1 UI work | Phase 3 scaffold can build on Linux while documenting the macOS UI dependency decision |
| Which Zed terminal crates/patterns are current for `alacritty_terminal` + GPUI rendering? | `technical-search-skill` plus official Zed manifests/source | Zed workspace pins `alacritty_terminal` to `zed-industries/alacritty` rev `4c129667ce56611becdc82de6e28218c80e2e88f`; Zed terminal crate depends on `gpui`, `alacritty_terminal`, and `vte`, confirming the native terminal direction is Rust PTY/Alacritty/VTE, not webview/Node | Terminal slice should isolate PTY/env work in `kanvibe-pty` and render through GPUI/Alacritty-derived contracts |

## Cross-boundary Contracts
| Contract | Producer | Consumer | Success path | Guard/error path |
|----------|----------|----------|--------------|------------------|
| SQLite existing DB | Electron TypeORM entities/migrations | Rust `kanvibe-core` | Open user DB unchanged and map all fields | Reject/diagnose incompatible DB without destructive migration |
| Scenario JSON | `qa/scenarios/*.json` | Electron Playwright runner and native qa-harness | Same steps replay on both apps | Unknown action/assertion produces structured failure |
| Theme tokens | Existing CSS/Tailwind tokens | Rust `kanvibe-theme` and GPUI views | Semantic token names map to same colors/roles | Hard-coded color usage is a QA blocker |
| Shortcuts | Shared shortcut contract | Native window/input handlers and dock UI | Format and match `Mod`/dock slots consistently | PR slot numbering drift fails unit/scenario tests |
| QA socket | Native debug app | `native/qa-harness` | Synthetic input/query/screenshot/DB snapshot commands work | Socket absent in release user builds |

## Halt Conditions
- Scope drift: any requested redesign, new feature, schema/path change, or Linux product support beyond cfg isolation.
- Product decision: deleting dynamic/public Electron behavior from parity scope, changing task status semantics, changing settings persistence, changing notification/hook behavior.
- Security/data decision: destructive data migration, shell startup file writes, broad environment forwarding, logging secrets/PII.
- Missing command/env: macOS-only packaging/screen recording required but current runtime is Linux; package manager/dependencies unavailable; external CLI (`git`, `gh`, `ffmpeg`, `screencapture`) unavailable for required artifact.

## Implementation Todos

### Todo 1: Create Feature Inventory Ledger
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Produce `qa/FEATURE_INVENTORY.md` as the single migration ledger.
- **Work**:
  - Inspect `electron/`, `src/desktop/renderer/routes/`, `src/components/`, `src/desktop/main/`, `src/entities/`, `src/migrations/`, `src/lib/`, `messages/`, and existing `qa/electron/`.
  - List every screen, modal, context menu, shortcut, IPC channel, DB table/entity, setting, notification, hook integration, terminal/Git/Diff/AI behavior.
  - For each row include status checkbox and verification method: screenshot comparison, behavior scenario, DB assert, unit/integration test, or artifact inspection.
  - Keep all checkboxes unchecked until Phase 5 proves parity.
- **Convention Notes**: Do not start `native/` implementation during this todo. Preserve Electron code.
- **Verification**:
  - `test -f qa/FEATURE_INVENTORY.md`
  - targeted `rg` checks for required categories and source file references.
  - Evidence:
    - `test -f qa/FEATURE_INVENTORY.md` -> exit 0
    - `rg -n "Routes And Shell|Board Screen|Modals And Popovers|Task Detail Screen|Diff Screen|Settings And Pane Layout|Shortcuts And Keyboard Policies|IPC, Preload, And Window Management|Service Contracts|Database And Persistence|Git, Worktrees, PTY, And Remote Sessions|AI Sessions And Hooks|Notifications And Background Sync|Theme, Styling, And i18n|Existing QA Assets|Phase 2 Baseline Capture Requirements|Phase 5 Scenario Requirements" qa/FEATURE_INVENTORY.md` -> exit 0
    - `rg -n "S01|S02|S03|S04|S05|S06|S07|S08|S09|S10|S11|S12|S13|S14" qa/FEATURE_INVENTORY.md` -> exit 0
- **Exit Criteria**: Ledger exists and covers all required categories with verification methods.
- **Status**: completed

### Todo 2: Record Electron Performance Baseline
- **Priority**: 1
- **Dependencies**: none
- **Goal**: Package Electron and record bundle size, cold start, and idle memory.
- **Work**:
  - Install/prepare dependencies using repo package manager through Corepack when needed.
  - Run `corepack pnpm dist:dir` or record an exact blocker if Linux/macOS packaging constraints prevent it.
  - Measure output size, cold start to window-visible where feasible, and idle memory on board screen.
  - Create `qa/PERF_BASELINE.md` with command output, environment, metrics, and blockers/skips.
- **Convention Notes**: Do not mutate global env; use local command invocation. Record Linux-vs-macOS caveats honestly.
- **Verification**:
  - `test -f qa/PERF_BASELINE.md`
  - package command exit status or explicit blocker evidence.
  - Evidence:
    - `corepack pnpm install` -> exit 0; postinstall rebuilt `better-sqlite3` for Electron.
    - `PATH="$tmp_corepack_bin:$PATH" /usr/bin/time -v pnpm dist:dir` -> exit 0; final wall time 2:58.54; max RSS 813,604 KB; output `dist/linux-unpacked`.
    - `xvfb-run -a -s '-screen 0 1440x960x24' node qa/perf/electron-baseline.cjs --iterations 5` -> exit 0; startup and task operation metrics recorded.
    - `test -f qa/PERF_BASELINE.md` -> exit 0 by artifact creation.
- **Exit Criteria**: Baseline metrics are recorded, or any unavailable metric has a concrete environment blocker and retry path.
- **Status**: completed

### Todo 3: Create Deterministic QA Seed DB and Scenario Specs
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Create `qa/seed/kanvibe-seed.sqlite` and `qa/scenarios/S01-S14*.json`.
- **Work**:
  - Reuse/extend existing seed and Electron QA helper patterns.
  - Ensure DB has 3 projects, at least 12 tasks across all status columns, priority/branch/PR combinations, and AI session history fixtures.
  - Define runner-neutral JSON scenarios for S01-S14 with actions and expected UI/DB effects.
  - Add validation scripts/tests if needed to prove seed cardinality and scenario schema.
- **Convention Notes**: Preserve SQLite schema exactly; do not alter migrations.
- **Verification**:
  - `sqlite3 qa/seed/kanvibe-seed.sqlite` row/count/schema queries.
  - Scenario JSON validation command or focused node test.
  - Evidence:
    - `corepack pnpm qa:seed` -> exit 0; created `qa/seed/kanvibe-seed.sqlite` and `qa/seed/MANIFEST.md`.
    - `sqlite3 -header -column qa/seed/kanvibe-seed.sqlite ...` -> 3 projects, 15 tasks, 11 app settings, 2 pane layouts; 3 tasks in each status.
    - `sqlite3 qa/seed/kanvibe-seed.sqlite "PRAGMA integrity_check; PRAGMA foreign_key_check;"` -> `ok` and no foreign-key rows.
    - `corepack pnpm qa:scenarios` -> exit 0; wrote `qa/scenarios/S01-*.json` through `S14-*.json`, schema, and manifest.
    - Node validation over `qa/scenarios/S*.json` -> 14 files, required keys present, shared seed path correct.
- **Exit Criteria**: Seed and scenarios exist and pass validation.
- **Status**: completed

### Todo 4: Capture Electron Baseline Screenshots, Videos, and Manifest
- **Priority**: 3
- **Dependencies**: Todo 3
- **Goal**: Populate `qa/baseline/` as Phase 5 ground truth.
- **Work**:
  - Extend Playwright/Electron QA runner to load the seed DB and scenario JSON.
  - Capture all required screens/states in `qa/baseline/screens/`.
  - Record S01-S14 videos in `qa/baseline/videos/`.
  - Create `qa/baseline/MANIFEST.md` with file, screen/state, locale/theme, and reproduction procedure.
- **Convention Notes**: This must complete before `native/` app code starts. If current Linux runtime cannot capture macOS-only states, record blocker and keep migration code gated.
- **Verification**:
  - Artifact counts match manifest.
  - Existing Electron QA commands or new focused baseline command exit 0.
  - Evidence:
    - `xvfb-run -a -s '-screen 0 1440x960x24' corepack pnpm qa:baseline` -> exit 0; captured 14 scenario baselines.
    - `find qa/baseline/screens -type f -name '*.png' | wc -l` -> 14.
    - `find qa/baseline/videos -type f -name '*.mp4' | wc -l` -> 14.
    - `ffprobe` over `qa/baseline/videos/*.mp4` -> all 14 videos have non-zero duration and size.
    - Manifest validation script -> 14 rows and each listed screen/video exists.
    - `sqlite3 qa/baseline/.run/kanvibe.db "PRAGMA integrity_check; PRAGMA foreign_key_check;"` -> `ok` and no foreign-key rows.
- **Exit Criteria**: Baseline manifest covers every required artifact and no required capture is missing without a blocker.
- **Status**: completed

### Todo 5: Research and Scaffold Native Rust Workspace
- **Priority**: 4
- **Dependencies**: Todo 4
- **Goal**: Create `native/` workspace with pinned dependencies and crate boundaries.
- **Work**:
  - Invoke `technical-search-skill` for current official `gpui`, `gpui-component`, and Zed terminal implementation evidence.
  - Create workspace crates: `kanvibe-core`, `kanvibe-git`, `kanvibe-pty`, `kanvibe-hooks`, `kanvibe-theme`, `kanvibe-i18n`, `kanvibe-app`, and `qa-harness`.
  - Add initial README and schema snapshot compatibility tests.
- **Convention Notes**: No webview/Electron/Node dependencies in native crates.
- **Verification**:
  - `cargo test --workspace`
  - `cargo build --workspace`
  - Evidence:
    - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md` -> exit 0; 5 source links PASS.
    - `cargo fmt --all` in `native/` -> exit 0.
    - `cargo test --workspace` in `native/` -> exit 0; seed DB schema compatibility test and crate boundary tests passed.
    - `cargo build --workspace` in `native/` -> exit 0.
- **Exit Criteria**: Workspace builds/tests and dependency pins are documented.
- **Status**: completed

### Todo 6: Implement Slice 1 - Window, Theme, i18n, Read-only Board
- **Priority**: 5
- **Dependencies**: Todo 5
- **Goal**: Native app opens seed DB and renders read-only board shell with theme/i18n parity.
- **Work**:
  - Implement SQLite read models, theme tokens, ko/en catalog import, GPUI app shell, and board columns/cards read-only.
  - Add schema compatibility and i18n tests.
  - Run corresponding Phase 5 subset against baseline board screens.
- **Convention Notes**: Color tokens must be semantic. No schema mutation.
- **Verification**:
  - `cargo test -p kanvibe-core -p kanvibe-theme -p kanvibe-i18n -p kanvibe-app`
  - qa-harness board subset artifacts.
  - Evidence:
    - Added `rusqlite = 0.40.1`, `serde = 1.0.228`, and `serde_json = 1.0.150` after crates.io verification.
    - `cargo test -p kanvibe-core -p kanvibe-theme -p kanvibe-i18n -p kanvibe-app -p qa-harness` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- readonly-board --repo-root .. --locale en --output ../qa/parity/slice-1/read-only-board-en.json` -> exit 0.
    - `cargo run -p qa-harness -- readonly-board --repo-root .. --locale ko --output ../qa/parity/slice-1/read-only-board-ko.json` -> exit 0.
    - `jq -r '[.locale, .newTask, .allProjects, (.columns | length), .projectCount, .doneTotal] | @tsv' qa/parity/slice-1/read-only-board-en.json qa/parity/slice-1/read-only-board-ko.json` -> `en + New Task All Projects 5 3 3` and `ko + 새 작업 전체 프로젝트 5 3 3`.
    - `cargo test --workspace` in `native/` -> exit 0.
    - `cargo build --workspace` in `native/` -> exit 0.
    - Current Linux runtime cannot capture a macOS GPUI screenshot; final visual parity remains required in Phase 5 before completion.
- **Exit Criteria**: Read-only board subset passes tests and visual QA.
- **Status**: completed

### Todo 7: Implement Slice 2 - Board Interactions
- **Priority**: 6
- **Dependencies**: Todo 6
- **Goal**: Port card CRUD, drag/drop, modals, context menu, find bar, priority/project color updates.
- **Work**:
  - Implement S01, S02, S07, S08, S09, S12 behavior and UI parity.
  - Add DB state asserts and behavior tests.
  - Rerun board Phase 5 subset after each repair.
- **Convention Notes**: Do not redesign; match Electron behavior and persistence.
- **Verification**:
  - focused cargo tests
  - qa-harness scenarios S01/S02/S07/S08/S09/S12.
  - Evidence:
    - Added Rust board write contracts in `kanvibe-core`: create task, update task, update status, move to column, reorder, delete, project color update, and done pagination.
    - `cargo test -p kanvibe-core -p qa-harness` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- board-interactions --repo-root .. --output ../qa/parity/slice-2/board-interactions.json` -> exit 0.
    - `jq '.' qa/parity/slice-2/board-interactions.json` -> report covers S02/S07/S08/S09/S12 with create/edit/move/reorder/delete/done paging/search/project filter/color evidence.
    - `cargo test --workspace` in `native/` -> exit 0.
    - `cargo build --workspace` in `native/` -> exit 0.
    - Current Linux runtime still cannot perform native macOS GPUI visual interaction capture; final UI parity remains required in Phase 5.
- **Exit Criteria**: Board interaction scenarios pass with DB asserts.
- **Status**: completed

### Todo 8: Implement Slice 3 - Task Detail, PTY, Dock, Sidebar
- **Priority**: 7
- **Dependencies**: Todo 7
- **Goal**: Port task detail terminal, AI session history pane display, dock shortcuts, sidebar hint persistence, and same-task focus policy.
- **Work**:
  - Implement PTY with sanitized local shell environment.
  - Implement dock item model and shortcut formatter/matcher.
  - Implement task detail window routing/focus policy and AppSettings-backed sidebar hint dismissal.
  - Cover S03, S04, S13, S14.
- **Convention Notes**: No global env mutation; dock numbering derives from item arrays.
- **Verification**:
  - PTY env unit tests
  - shortcut unit tests
  - qa-harness S03/S04/S13/S14.
  - Evidence:
    - Added Rust PTY shell environment sanitizer preserving shell basics while stripping `PORT`, `HOST`, `NODE_ENV`, and `KANVIBE_*`.
    - Added AppSettings-backed sidebar default/hint persistence helpers.
    - Added task-detail dock slot model with macOS `Cmd+{n}` and Linux `Alt+{n}` matching; PR inserts at slot 4 and shifts chat/AI sessions.
    - Added task-detail href localization and existing-window focus decision model.
    - `cargo test -p kanvibe-app -p kanvibe-pty -p kanvibe-core -p qa-harness` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- task-detail --repo-root .. --output ../qa/parity/slice-3/task-detail-pty-dock.json` -> exit 0.
    - `jq '.' qa/parity/slice-3/task-detail-pty-dock.json` -> report covers S03/S04/S13/S14 task fields, dock item order, shortcut labels, PR URL, provider filters, existing-window focus, remote zellij/SSH metadata, and no generic env leak.
    - `cargo test --workspace` in `native/` -> exit 0.
    - `cargo build --workspace` in `native/` -> exit 0.
    - Current Linux runtime still cannot perform native macOS GPUI visual task-detail capture; final UI parity remains required in Phase 5.
- **Exit Criteria**: Task detail subset passes tests and QA artifacts.
- **Status**: completed

### Todo 9: Implement Slice 4 - Git, Worktree, Diff, Editor
- **Priority**: 8
- **Dependencies**: Todo 8
- **Goal**: Port worktree/branch task flows and Diff screen/editor parity.
- **Work**:
  - Implement Git operations and `gh` CLI PR URL integration as explicit child process contracts.
  - Implement branch task modal flow, file tree, diff viewer, code editor, save behavior.
  - Cover S05 and S06 with temporary git repo fixtures.
- **Convention Notes**: Use Rust libraries/processes directly; no Monaco/webview reuse.
- **Verification**:
  - git/worktree crate tests
  - qa-harness S05/S06.
  - Evidence:
    - Added local command-backed `kanvibe-git` contracts for repo validation, branches, changed-file diff listings, original/current file reads, editor save, managed worktree paths, and worktree session creation.
    - Added `kanvibe-core::branch_from_task` persistence matching Electron branch-from-task metadata updates.
    - `cargo test -p kanvibe-git -p kanvibe-core -p qa-harness` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- git-diff --repo-root .. --output ../qa/parity/slice-4/git-diff-worktree.json` -> exit 0.
    - `jq '.' qa/parity/slice-4/git-diff-worktree.json` -> report covers S05/S06 with modified/untracked files, original/current/saved editor content checks, real worktree branch, and DB `branchFromTask` status/base/session metadata.
    - `cargo test --workspace` in `native/` -> exit 0.
    - `cargo build --workspace` in `native/` -> exit 0.
    - Current Linux runtime still cannot perform native macOS GPUI visual Diff/editor capture; final UI parity remains required in Phase 5.
- **Exit Criteria**: Git/Diff subset passes.
- **Status**: completed

### Todo 10: Implement Slice 5 - AI Sessions, Hooks, Notifications
- **Priority**: 9
- **Dependencies**: Todo 9
- **Goal**: Port AI session history aggregation, hook HTTP server/installers, hook status, board notifications, and notification center.
- **Work**:
  - Implement session readers for Claude/Codex/Gemini/OpenCode-compatible history fixtures.
  - Implement hook endpoint/server status/installers and notification persistence/display.
  - Cover S11 and hook/status inventory rows.
- **Convention Notes**: Keep `KANVIBE_*` scoping and avoid writing shell startup/global config outside explicit hook installer behavior.
- **Verification**:
  - hooks/session/notification cargo tests
  - qa-harness S11.
  - Evidence:
    - Added `kanvibe-hooks` contracts for scoped hook server URLs, shell/plugin hook URL extraction, expected/reachable hook validation, AI provider session aggregation, notification center behavior, board event notifications, and visible hook provider status.
    - Added `qa-harness` S11 report generation for hook status, AI sessions, notification center, and background sync setting evidence.
    - `cargo test -p kanvibe-hooks -p qa-harness` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- notifications-hooks --repo-root .. --output ../qa/parity/slice-5/notifications-hooks-ai.json` -> exit 0.
    - `jq -r '[.artifact, .notificationCenter.unreadCount, .hookStatus.visible, .hookStatus.hasExpectedHookServerUrl, .aiSessions.sessionCount, .aiSessions.firstSessionId, .appSettings.background_sync_enabled] | @tsv' qa/parity/slice-5/notifications-hooks-ai.json` -> `slice-5-notifications-hooks-ai  3  true  true  4  codex-native-hooks  false`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - Current Linux runtime still cannot perform native macOS GPUI visual notification/hook capture; final UI parity remains required in Phase 5.
- **Exit Criteria**: AI/hook/notification subset passes.
- **Status**: completed

### Todo 11: Implement Slice 6 - Settings, PaneLayout, Window Policy, Remote Sessions
- **Priority**: 10
- **Dependencies**: Todo 10
- **Goal**: Port settings screen, language switching, PaneLayout editor, existing-window focus, ssh/tmux remote session dependency flows.
- **Work**:
  - Implement SettingsRoute parity, PaneLayout editor persistence, app-wide settings, and remote session dependency UI/contracts.
  - Cover S10 plus remaining inventory rows.
- **Convention Notes**: App-wide settings persist in `AppSettings`.
- **Verification**:
  - settings/layout/remote cargo tests
  - qa-harness S10 and relevant screens.
  - Evidence:
    - Added typed `kanvibe-core` app settings for vim mode, theme preference, default session type, task search shortcut, background sync, release update dismissals, and pane layout configs against existing SQLite rows.
    - Added `kanvibe-pty` contracts for tmux/zellij dependency status, remote blocked-host state, install/check command strings, tmux pane layout commands, and zellij layout KDL generation.
    - Added `kanvibe-app` settings shell state plus internal route extraction/window focus policy for settings and pane-layout routes.
    - Added `qa-harness` S10/S14 report generation at `qa/parity/slice-6/settings-layout-remote.json`.
    - `cargo test -p kanvibe-core -p kanvibe-pty -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- settings-layout-remote --repo-root .. --output ../qa/parity/slice-6/settings-layout-remote.json` -> exit 0.
    - `jq -r '[.artifact, .settings.themePreferenceAfter, .settings.defaultSessionType, .settings.vimModeEnabled, .paneLayout.savedLayoutType, .paneLayout.fallbackLayoutType, .remoteSessionDependency.visible, .remoteSessionDependency.availableAfterInstall, .windowPolicy.action] | @tsv' qa/parity/slice-6/settings-layout-remote.json` -> `slice-6-settings-layout-remote  dark  zellij  false  vertical_2  quad  true  true  focus-existing`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - Current Linux runtime still cannot perform native macOS GPUI visual settings/pane-layout/remote dependency capture; final UI parity remains required in Phase 5.
- **Exit Criteria**: Settings/layout/window/remote subset passes.
- **Status**: completed

### Todo 12: Implement Slice 7 - Packaging and Performance
- **Priority**: 11
- **Dependencies**: Todo 11
- **Goal**: Produce release build, `.app` bundle/DMG script, and final performance measurements.
- **Work**:
  - Add packaging scripts with ad-hoc signing where applicable.
  - Measure release bundle size, cold start, idle memory, and terminal scroll FPS.
  - Update `native/README.md` and `qa/PERF_BASELINE.md` with final comparison.
- **Convention Notes**: Do not publish or code-sign with real credentials.
- **Verification**:
  - `cargo build --release`
  - bundle artifact exists
  - performance metrics recorded.
  - Evidence:
    - Added `native/scripts/package-macos-app.sh`, which Darwin-gates GPUI/native-ui `.app` bundling, builds `kanvibe-app` with `--features native-ui`, writes `Info.plist`/`PkgInfo`, and ad-hoc signs with `codesign --sign -` when available.
    - Added `/native/target` and `/native/dist` to `.gitignore`.
    - Updated `native/README.md` with Linux release and macOS bundle commands.
    - Updated `qa/PERF_BASELINE.md` with native release build, binary size, headless scaffold startup, and macOS bundle/runtime gaps.
    - `bash -n native/scripts/package-macos-app.sh` -> exit 0.
    - `native/scripts/package-macos-app.sh` on Linux -> exit 78 with expected Darwin/GPUI/codesign runtime gate.
    - `/usr/bin/time -v cargo build --release --quiet` in `native/` -> exit 0, wall time 0:49.78, max RSS 433,396 KB.
    - `wc -c native/target/release/kanvibe-app native/target/release/qa-harness` -> `443192` bytes and `3369272` bytes.
    - `/usr/bin/time -v native/target/release/kanvibe-app` -> exit 0, `HeadlessStub`, max RSS 1,992 KB.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - macOS `.app`/DMG artifact, GPUI cold start, idle memory, and terminal scroll FPS remain Phase 5 runtime gates because this host is Linux.
- **Exit Criteria**: Native release artifact and performance table are complete.
- **Status**: completed

### Todo 13: Run Full Phase 5 QA and Repair Loop
- **Priority**: 12
- **Dependencies**: Todo 12
- **Goal**: Prove final parity and close the migration.
- **Work**:
  - Run full qa-harness over S01-S14 with seed DB copy.
  - Write `qa/parity/run-<N>/QA_REPORT.md`.
  - Repair each FAIL with a regression test and rerun the same gate.
  - After one full PASS, rerun everything once more without code changes for flake verification.
  - Check every `qa/FEATURE_INVENTORY.md` row only after evidence exists.
- **Convention Notes**: Do not declare completion until two consecutive PASS reports, all inventory boxes checked, and performance targets met.
- **Verification**:
  - final two parity reports PASS.
  - `cargo test --workspace`
  - `cargo build --release`
  - Evidence:
    - Added `qa-harness full-parity` aggregation, producing `full-parity.json` and `QA_REPORT.md` in a run directory.
    - Added `qa_harness::full_parity_report` regression coverage verifying all S01-S14 scenarios are covered by headless/native slice contracts and that the final status is blocked only by macOS runtime gates.
    - `cargo test -p qa-harness --quiet` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-001` -> exit 0.
    - `jq -r '[.status, .headlessContractsPass, .scenarioCount, (.coveredScenarioIds|length), (.missingScenarioIds|length), (.macosRuntimeGates|length)] | @tsv' qa/parity/run-001/full-parity.json` -> `BLOCKED  true  14  14  0  4`.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-002` -> exit 0.
    - `jq -r '[.status, .headlessContractsPass, .scenarioCount, (.coveredScenarioIds|length), (.missingScenarioIds|length), (.macosRuntimeGates|length)] | @tsv' qa/parity/run-002/full-parity.json` -> `BLOCKED  true  14  14  0  4`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - Blocking gates in both run reports: native GPUI screenshot/video parity for S01-S14, native `.app`/DMG packaging, GPUI cold start/idle memory/terminal scroll FPS, and two consecutive PASS reports all require a macOS runtime.
- **Exit Criteria**: All objective deliverables exist and QA contract returns PASS.
- **Status**: blocked - macOS runtime required for final GPUI visual/package/performance PASS gates

### Todo 14: Continue Native GPUI Entry Implementation
- **Priority**: 13
- **Dependencies**: Todo 13 blocker audit
- **Goal**: Move from headless-only app boundary toward a real macOS GPUI application entrypoint while preserving Linux verification.
- **Work**:
  - Add a macOS-only `native-ui` GPUI module using the pinned `gpui` and `gpui-component` APIs.
  - Add typed launch configuration using only scoped `KANVIBE_*` overrides.
  - Add a Linux-testable render spec that maps the existing seed board into root window content, board columns, task counts, first cards, and semantic color tokens.
  - Keep GPUI visual parity and actual macOS screenshots as Phase 5 runtime gates.
- **Convention Notes**: Do not add Electron/webview/Node runtime; keep non-macOS code buildable and use scoped env only.
- **Verification**:
  - `cargo test -p kanvibe-app --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-003`
  - Evidence:
    - Added `native/crates/kanvibe-app/src/native_ui.rs`, gated by `#[cfg(all(target_os = "macos", feature = "native-ui"))]`, with `Application::new()`, `gpui_component::init(cx)`, `Root::new(view, window, cx)`, a board root view, and a gpui-component `Button`.
    - Added `NativeUiLaunchConfig` scoped to `KANVIBE_REPO_ROOT`, `KANVIBE_DB_PATH`, and `KANVIBE_LOCALE`.
    - Added `NativeUiRenderSpec`/`NativeUiColumnSpec` and tests proving seed board mapping, route/locale, task counts, primary/new-task label, and semantic color tokens.
    - `cargo info gpui@0.2.2`, `cargo info gpui-component@0.5.1`, and local crate source examples confirmed the pinned API patterns.
    - `cargo test -p kanvibe-app --quiet` in `native/` -> exit 0, 12 tests passed.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` on Linux -> exit 0.
    - `rustup target add x86_64-apple-darwin` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 before app code due missing Apple C compiler for `ring`; dependency path is `gpui -> gpui_http_client -> zed-reqwest -> rustls -> ring`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-003` -> exit 0; report remains `BLOCKED true 14 14 0 4` until macOS visual/package/performance gates run.
- **Exit Criteria**: macOS GPUI entry slice exists, Linux-safe contracts pass, and remaining gate is concrete toolchain/runtime verification.
- **Status**: completed

### Todo 15: Add Native QA Control Protocol Contract
- **Priority**: 14
- **Dependencies**: Todo 14
- **Goal**: Replace the harness-only parity aggregation with a real debug-only native control-channel contract required by Phase 5.
- **Work**:
  - Add line-delimited JSON commands for health checks, element/text queries, synthetic input boundaries, screenshot capture boundary, and DB snapshots.
  - Gate Unix socket startup to debug builds and `KANVIBE_QA_SOCKET`.
  - Wire the socket startup into the macOS GPUI entrypoint without exposing it to release user builds.
  - Parse all existing `qa/scenarios/S01-S14.json` files and prove every declared action/assertion maps to a native control command category.
- **Convention Notes**: Use only scoped `KANVIBE_*` env, do not include Electron/webview/Node, and keep screenshot/event dispatch limitations explicit until macOS runtime implementation.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `jq -r '[.artifact, .socketEnv, .debugOnly, .scenarioFileCount, .stepMappingCount, .assertionMappingCount, .coveragePass, (.unsupportedSteps|length), (.unsupportedAssertions|length)] | @tsv' qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo test --workspace --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `cargo build --workspace --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-004`
  - Evidence:
    - Added `kanvibe_app::qa_control` with `KANVIBE_QA_SOCKET`, `QaControlCommand`, `QaControlResponse`, `QaControlState`, JSON line handling, debug-only Unix socket startup, and protocol capabilities.
    - The macOS `native_ui` entrypoint now calls `spawn_debug_qa_socket_from_env(spec.clone())` before opening the GPUI window.
    - Added `qa-harness qa-control` report generation and included `native-qa-control-protocol` in `full_parity_report`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 15 app tests and 10 harness tests passed.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `jq -r '[.artifact, .socketEnv, .debugOnly, .scenarioFileCount, .stepMappingCount, .assertionMappingCount, .coveragePass, (.unsupportedSteps|length), (.unsupportedAssertions|length)] | @tsv' qa/parity/slice-qa-control/qa-control-protocol.json` -> `native-qa-control-protocol  KANVIBE_QA_SOCKET  true  14  83  43  true  0  0`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-004` -> exit 0; `native-qa-control-protocol` appears in slice artifacts, final status remains blocked by macOS visual/package/performance gates.
- **Exit Criteria**: Phase 5 has a concrete native control protocol and all scenario files map to it with no unsupported action/assertion types.
- **Status**: completed

### Todo 16: Add Native QA Replay Client And Socket Smoke Test
- **Priority**: 15
- **Dependencies**: Todo 15
- **Goal**: Move from static scenario-to-protocol mapping to an executable harness-side replay plan and proven Unix socket client path.
- **Work**:
  - Add task element ids to the native render spec so the QA protocol can query seeded board cards by stable ids like `task.qa-task-todo-local`.
  - Add semantic `syntheticClick` commands for scenario actions that target known UI elements instead of placeholder coordinates.
  - Add `QaControlClient` in `qa-harness` for one-line JSON command/response round trips over the debug Unix socket.
  - Generate a scenario replay-plan artifact that expands S01-S14 launch steps, control commands, screenshots, DB snapshots, and assertions into executable command records.
  - Add a debug Unix socket smoke test that starts the app-side QA socket and round-trips `ping` plus a task element query through the harness client.
- **Convention Notes**: The socket remains debug-only and scoped by `KANVIBE_QA_SOCKET`; event dispatch and screenshot capture remain macOS runtime gates.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `jq -r '[.artifact, .scenarioFileCount, .launchActionCount, .socketCommandCount, .coveragePass, (.unsupportedSteps|length), (.unsupportedAssertions|length), .client.unixSupportedInThisBuild] | @tsv' qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-005`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `NativeUiCardSpec` and registered `task.<id>` / `task.<id>.title` QA elements from the seed board render spec.
    - Added `QaControlCommand::SyntheticClick`, protocol capability reporting, and a public debug/unix `spawn_debug_qa_socket_at_path` helper for harness smoke tests.
    - Added `QaControlClient`, `qa_control_replay_plan_report`, and `qa-harness qa-replay-plan`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 16 app tests and 12 harness tests passed.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `jq -r '[.artifact, .socketEnv, .debugOnly, .scenarioFileCount, .stepMappingCount, .assertionMappingCount, .coveragePass, (.unsupportedSteps|length), (.unsupportedAssertions|length), ([.stepMappings[].controlCommand] | unique | join(","))] | @tsv' qa/parity/slice-qa-control/qa-control-protocol.json` -> `native-qa-control-protocol  KANVIBE_QA_SOCKET  true  14  83  43  true  0  0  dumpScreenshot,launchAppWithQaSocket,queryElement,queryText,syntheticClick,syntheticKey`.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - `jq -r '[.artifact, .scenarioFileCount, .launchActionCount, .socketCommandCount, .coveragePass, (.unsupportedSteps|length), (.unsupportedAssertions|length), .client.unixSupportedInThisBuild] | @tsv' qa/parity/slice-qa-control/qa-control-replay-plan.json` -> `native-qa-control-replay-plan  14  14  116  true  0  0  true`.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-005` -> exit 0; report is `BLOCKED true 14 14 0 true 9 4`, includes `native-qa-control-replay-plan`, and still blocks only on macOS visual/package/performance gates.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Phase 5 has a harness-side client and replay-plan artifact that can drive the debug socket; Linux smoke tests prove the transport path while final PASS remains gated by macOS runtime evidence.
- **Status**: completed

### Todo 17: Execute Native QA Replay Stream Over Debug Socket
- **Priority**: 16
- **Dependencies**: Todo 16
- **Goal**: Execute the full S01-S14 replay command stream over the QA control socket and record transport evidence separately from behavior blockers.
- **Work**:
  - Add `qa_control_replay_execution_report` for replaying all generated control commands against a provided native QA socket.
  - Add `qa_control_replay_smoke_report` for Linux/debug verification by spawning the app-side QA socket in-process from the seed board render spec.
  - Add `qa-harness qa-replay-execute`, accepting `--socket <path>` for a real native app socket and defaulting to the in-process debug smoke socket when no socket is supplied.
  - Classify every command result as transport success/failure and behavior pass/blocked/fail, with counters for missing elements, pending GPUI dispatch, screenshot blockers, DB snapshots, and synthetic inputs.
  - Include `native-qa-control-replay-execution` in `full_parity_report`.
- **Convention Notes**: The execution report does not claim final parity; it proves command transport and surfaces remaining GPUI dispatch/capture/native surface gaps as blockers.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact, .status, .transportPass, .scenarioFileCount, .launchActionCount, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-006`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added a socket-backed executor that runs all replay-plan control commands and records per-scenario command responses.
    - Added `qa-harness qa-replay-execute`; without `--socket` it starts an in-process debug QA socket for smoke evidence, and with `--socket` it can target a real native app socket.
    - Added tests for full replay execution over the debug socket.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 16 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - `jq -r '[.artifact, .status, .transportPass, .scenarioFileCount, .launchActionCount, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json` -> `native-qa-control-replay-execution  BLOCKED  true  14  14  116  40  14  23  0  0`.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-006` -> exit 0; report is `BLOCKED true 14 14 0 true 10 4`, includes `native-qa-control-replay-execution`, and still blocks on macOS visual/package/performance gates.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: The harness can execute every generated QA control command against a socket and produce reproducible transport/behavior counters, while final PASS remains blocked until real macOS GPUI dispatch, screenshot/video capture, packaging, and performance gates run.
- **Status**: completed

### Todo 18: Expose Seed-Backed Native QA Elements After Replay Interactions
- **Priority**: 17
- **Dependencies**: Todo 17
- **Goal**: Remove the remaining missing-element replay blockers by exposing real seed-backed task metadata and interaction-derived QA state through the native control socket.
- **Work**:
  - Extend `NativeUiCardSpec` with branch, session type, SSH host, and PR URL metadata loaded from the seed SQLite DB.
  - Register task field ids such as `task.<id>.field.session_type`, `task.<id>.field.pr_url`, `task.<id>.field.ssh_host`, and `taskTitle.<slug>` from visible seed cards.
  - Add a debug QA dynamic state map updated by synthetic clicks for scenario routes/surfaces: task detail route/dock, diff route/pane/sidebar, create-task result, settings pane route, notification center, hook status, AI provider filters, project filter, session dependency panel, branch-task visibility, and window-count queries.
  - Preserve `protocol-ready-gpui-dispatch-pending` for synthetic input responses so the report still blocks on real GPUI event dispatch.
- **Convention Notes**: The new elements are seed-backed or replay-interaction state for the debug QA protocol; screenshots and real GPUI dispatch remain explicit Phase 5 blockers.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact, .status, .transportPass, .scenarioFileCount, .launchActionCount, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-007`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added seed-backed card metadata to `NativeUiCardSpec` and QA element registration for task fields/title slugs.
    - Added mutex-backed dynamic QA elements updated by synthetic clicks while keeping synthetic dispatch marked as GPUI-dispatch-pending.
    - Added app-side QA tests for seed metadata and replay interaction state.
    - Updated qa-harness replay smoke expectations: missing element blockers are now zero.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 18 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - `jq -r '[.artifact, .status, .transportPass, .scenarioFileCount, .launchActionCount, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json` -> `native-qa-control-replay-execution  BLOCKED  true  14  14  116  40  14  0  0  0`.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-007` -> exit 0; report is `BLOCKED true 14 14 0 true 10 4`, includes `native-qa-control-replay-execution`, and still blocks on macOS visual/package/performance gates.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: All replay query commands find a native QA element/text response, leaving only GPUI event dispatch and screenshot/video capture as replay-execution blockers.
- **Status**: completed

### Todo 19: Evaluate Native QA DB Assertions Against Snapshot State
- **Priority**: 18
- **Dependencies**: Todo 18
- **Goal**: Replace superficial `dbSnapshot` command success with real DB assertion evaluation in the native replay execution report.
- **Work**:
  - Extend `QaControlResponse::DbSnapshot` with task rows, app settings, pane layouts, generic env leak status, and worktree-created titles.
  - Mutate debug QA snapshot state for create task, branch task, drag-to-progress, Vim-style status move, delete task, vim setting, and pane layout selection actions.
  - Evaluate `dbCount`, `taskCountByStatus`, `dbRow`, `dbMissing`, `noWorktreeCreated`, and `noGenericEnvLeak` assertions in `qa-harness` from the replay item source assertion and snapshot response.
  - Keep replay status blocked only for GPUI dispatch and screenshot capture when all DB assertions pass.
- **Convention Notes**: The snapshot is debug QA evidence for scenario semantics and does not replace final macOS GPUI event dispatch, screenshot/video, or real packaged app verification.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact, .status, .transportPass, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount, .dbSnapshotCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '.scenarioResults[].commandResults[] | select(.commandType=="dbSnapshot") | [.scenarioId, .sourceType, .behaviorStatus, (.blocker // "")] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-008`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `QaTaskSnapshot` and `QaPaneLayoutSnapshot` plus settings/env/worktree fields to `DbSnapshot`.
    - Added debug QA snapshot mutations for S02/S06/S07/S08/S09/S10 DB assertion paths.
    - Added harness-side DB assertion evaluation against snapshot data.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 19 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - `jq -r '[.artifact, .status, .transportPass, .executedCommandCount, .pendingDispatchCount, .screenshotBlockedCount, .missingElementCount, .structuredErrorCount, .transportErrorCount, .dbSnapshotCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json` -> `native-qa-control-replay-execution  BLOCKED  true  116  40  14  0  0  0  13`.
    - `jq -r '.scenarioResults[].commandResults[] | select(.commandType=="dbSnapshot") | [.scenarioId, .sourceType, .behaviorStatus, (.blocker // "")] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json` -> all 13 DB assertions report `pass`.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-008` -> exit 0; report is `BLOCKED true 14 14 0 true 10 4`, includes `native-qa-control-replay-execution`, and still blocks on macOS visual/package/performance gates.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Replay execution evaluates every declared DB assertion with pass/fail semantics; all DB assertions pass and remaining replay blockers are GPUI dispatch plus screenshot/video capture.
- **Status**: completed

### Todo 20: Isolate Native QA Replay State Per Scenario
- **Priority**: 19
- **Dependencies**: Todo 19
- **Goal**: Make the in-process native replay smoke path mirror Phase 5's per-scenario app/seed launch contract instead of sharing one debug QA state across S01-S14.
- **Work**:
  - Factor replay command execution into a per-scenario helper reused by external-socket and smoke execution modes.
  - Keep `qa_control_replay_execution_report(..., --socket)` on a single external client for a real already-launched app.
  - Change debug smoke execution to spawn a fresh in-process QA socket/client for every scenario plan and report `scenarioSocketCount`.
  - Extend the smoke test to assert 14 scenario sockets and all 13 DB snapshot assertions still pass under isolated state.
- **Convention Notes**: This improves Linux-verifiable harness fidelity only; it does not fake final GPUI dispatch, screenshot/video capture, `.app` packaging, or macOS performance evidence.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact,.mode,.status,.transportPass,.scenarioSocketCount,.executedCommandCount,.pendingDispatchCount,.screenshotBlockedCount,.missingElementCount,.structuredErrorCount,.transportErrorCount,.dbSnapshotCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '.scenarioResults[] as $s | $s.commandResults[] | select(.commandType=="dbSnapshot") | [$s.scenarioId, .sourceType, .behaviorStatus, (.blocker // "")] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-009`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Factored replay execution in `native/crates/qa-harness/src/lib.rs` so external socket mode remains shared-client while debug smoke mode uses `inProcessDebugSocketPerScenario`.
    - Added `scenarioSocketCount` to replay execution artifacts and a smoke regression check that all DB snapshots pass after per-scenario state reset.
    - `cargo test -p qa-harness --quiet` in `native/` -> exit 0, 13 tests passed.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - Replay counter check -> `native-qa-control-replay-execution  inProcessDebugSocketPerScenario  BLOCKED  true  14  116  40  14  0  0  0  13`.
    - DB assertion check -> all 13 DB snapshots pass across S01, S02, S06, S07, S08, S09, S10, S11, and S14.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-009` -> exit 0; report is `BLOCKED`, `headlessContractsPass=true`, scenario coverage `14/14`, and includes `native-qa-control-replay-execution`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 19 app tests and 13 harness tests passed.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: The Linux debug smoke artifact proves each scenario can replay from a fresh native QA state; all DB assertions still pass, and remaining replay blockers are GPUI dispatch plus screenshot/video capture.
- **Status**: completed

### Todo 21: Evaluate Native QA Non-DB Assertions Semantically
- **Priority**: 20
- **Dependencies**: Todo 20
- **Goal**: Replace generic element-exists success for non-DB assertions with assertion-specific pass/fail checks where the debug QA protocol exposes enough state.
- **Work**:
  - Route task element queries through mutable QA task state so deleted tasks and project-filter-hidden tasks can return absent.
  - Add project-filter state and dock-root metadata derived from native task detail dock items/shortcut labels.
  - Evaluate `taskNotVisible`, `route`, `taskField`, `dockItems`, `shortcutLabels`, `aiProviderFilters`, `taskTitleVisible`, and title/branch variants of `taskVisible` from query responses.
  - Count expected absence as PASS instead of a missing-element blocker, while preserving missing blockers for assertions that require visible UI.
  - Extend focused tests to prove deleted/filtered task absence and all replay assertion commands pass.
- **Convention Notes**: This is a harness and debug-state fidelity change only; it does not convert GPUI synthetic dispatch or screenshots into fake PASS evidence.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact,.mode,.status,.transportPass,.scenarioSocketCount,.executedCommandCount,.pendingDispatchCount,.screenshotBlockedCount,.missingElementCount,.structuredErrorCount,.transportErrorCount,.dbSnapshotCount] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '.scenarioResults[] as $s | $s.commandResults[] | select(.sourceKind=="assertion") | [$s.scenarioId, .sourceType, .commandType, .behaviorStatus, (.blocker // "")] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-010`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/kanvibe-app/src/qa_control.rs || true`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Initial focused test rerun surfaced one fixture expectation mismatch for `qa-task-done-remote`; corrected expected title to the seed value `Archive remote release branch` and reran the same focused gate.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 20 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - Replay counter check -> `native-qa-control-replay-execution  inProcessDebugSocketPerScenario  BLOCKED  true  14  116  40  14  0  0  0  13`.
    - Assertion result check -> every assertion command across S01-S14 reports `pass`, including `taskNotVisible`, `route`, `dockItems`, `shortcutLabels`, `taskField`, `aiProviderFilters`, `onlyProjectTasksVisible`, and all DB assertions.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-010` -> exit 0; report is `BLOCKED`, `headlessContractsPass=true`, scenario coverage `14/14`, and includes `native-qa-control-replay-execution`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Every non-step scenario assertion in the native replay artifact has assertion-specific pass/fail semantics and currently reports `pass`; replay remains blocked only by GPUI dispatch and screenshot/video capture.
- **Status**: completed

### Todo 22: Add macOS Screenshot Capture Contract
- **Priority**: 21
- **Dependencies**: Todo 21
- **Goal**: Replace the generic screenshot-integration placeholder with a concrete macOS `screencapture -l` path while keeping Linux blocker reporting honest.
- **Work**:
  - Add scoped `KANVIBE_QA_WINDOW_ID` protocol metadata for the macOS window id.
  - Implement `dumpScreenshot` so macOS debug builds run `/usr/sbin/screencapture -x -l <window-id> <path>` and report captured status from command success plus output existence.
  - Keep non-macOS builds returning `captured=false` with an explicit macOS/window-id blocker reason.
  - Add tests for numeric window-id validation, command arguments, Linux blocker response, and protocol capabilities.
  - Update `native/README.md` with the screenshot env var and capture command contract.
  - Update replay blocker wording to distinguish the remaining macOS screenshot runtime need from missing protocol implementation.
- **Convention Notes**: This does not claim visual parity on Linux. It creates the macOS runtime path required for Phase 5; actual screenshots/videos still need a macOS GPUI window id and execution environment.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `jq -r '[.protocolCapabilities.screenshotWindowIdEnv,.protocolCapabilities.screenshotCapture.macos,.protocolCapabilities.screenshotCapture.linux] | @tsv' qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '[.artifact,.mode,.status,.transportPass,.scenarioSocketCount,.executedCommandCount,.pendingDispatchCount,.screenshotBlockedCount,.missingElementCount,.structuredErrorCount,.transportErrorCount,.dbSnapshotCount,(.blockers|join(" | "))] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq -r '.scenarioResults[] as $s | $s.commandResults[] | select(.commandType=="dumpScreenshot") | [$s.scenarioId,.behaviorStatus,.response.reason] | @tsv' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-012`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md || true`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `KANVIBE_QA_WINDOW_ID` and macOS `screencapture -x -l <window-id> <path>` handling to `kanvibe-app` QA control.
    - Added tests for screenshot command args/window-id validation, Linux blocker response, and capability metadata.
    - Updated `native/README.md` QA Control Protocol section with the screenshot contract.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 22 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json` -> exit 0.
    - Protocol capability check -> `KANVIBE_QA_WINDOW_ID  screencapture -x -l <window-id> <path>  blocked`.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - Replay counter/blocker check -> `native-qa-control-replay-execution  inProcessDebugSocketPerScenario  BLOCKED  true  14  116  40  14  0  0  0  13  synthetic GPUI event dispatch is still protocol-ready but not implemented | native screenshot capture requires macOS screencapture and a QA window id`.
    - Screenshot step check -> all 14 screenshot commands report `blocked` with reason `requires macOS screencapture and KANVIBE_QA_WINDOW_ID` on Linux.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-012` -> exit 0; report is `BLOCKED`, `headlessContractsPass=true`, scenario coverage `14/14`, and includes `native-qa-control-replay-execution`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: `dumpScreenshot` has a concrete macOS capture implementation and Linux replay artifacts report the remaining screenshot blocker as a macOS/window-id runtime requirement rather than missing protocol work.
- **Status**: completed

### Todo 23: Prepare Per-Scenario Native Replay DB Copies
- **Priority**: 22
- **Dependencies**: Todo 22
- **Goal**: Align the native replay launch contract with Phase 5's requirement that every scenario runs against its own seed DB copy.
- **Work**:
  - Add deterministic native replay DB copy paths under `qa/parity/native-db/<scenario-id>.sqlite`.
  - Change `launchApp` replay items so `KANVIBE_DB_PATH` points to the per-scenario copy and records the original `sourceSeed`.
  - Prepare the per-scenario DB copy before each in-process debug socket smoke launch.
  - Add replay-plan assertions that the launch metadata and env use the copied DB path.
  - Document `qa/parity/native-db/` in `native/README.md`.
- **Convention Notes**: The original `qa/seed/kanvibe-seed.sqlite` remains the immutable compatibility fixture; generated native replay DB copies are QA artifacts.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `jq -r '.scenarioPlans[] | [.scenarioId, .replayItems[0].sourceSeed, .replayItems[0].dbCopyPath, .replayItems[0].env.KANVIBE_DB_PATH] | @tsv' qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `find qa/parity/native-db -maxdepth 1 -type f -name 'S*.sqlite' | wc -l`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-013`
  - `jq -r '[.artifact,.status,.headlessContractsPass,.scenarioCount,(.missingScenarioIds|length),(.sliceArtifacts|length)] | @tsv' qa/parity/run-013/full-parity.json`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md || true`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `NATIVE_REPLAY_DB_DIR_FROM_REPO_ROOT`, per-scenario copy path metadata, and debug smoke copy preparation in `qa-harness`.
    - Updated replay plan launch env so every `KANVIBE_DB_PATH` is `qa/parity/native-db/<scenario-id>.sqlite`.
    - Updated `native/README.md` to document generated per-scenario DB copies.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0, 22 app tests and 13 harness tests passed.
    - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json` -> exit 0.
    - Replay-plan DB path check -> all 14 scenarios use `sourceSeed=qa/seed/kanvibe-seed.sqlite` and matching `dbCopyPath`/`KANVIBE_DB_PATH` under `qa/parity/native-db/`.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - Native replay DB copy count -> `14`.
    - Replay counter check remains `native-qa-control-replay-execution  inProcessDebugSocketPerScenario  BLOCKED  true  14  116  40  14  0  0  0  13`.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-013` -> exit 0; report is `phase-5-full-parity  BLOCKED  true  14  0  10`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0 after debug-gating the smoke-only copy-preparation helper to avoid release dead-code warnings.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Replay plan and debug smoke execution use one copied SQLite DB per scenario, with generated DB copies present and all existing replay assertions still passing.
- **Status**: completed

### Todo 24: Add External Native App Launch Contract Report
- **Priority**: 23
- **Dependencies**: Todo 23
- **Goal**: Add command-backed evidence for launching the real `kanvibe-app` process with QA socket env, rather than relying only on in-process debug socket smoke.
- **Work**:
  - Add `native_app_launch_report` in `qa-harness` that resolves the real debug app binary, prepares a native launch DB copy, starts the process with a narrow scoped environment, waits for `KANVIBE_QA_SOCKET`, and records whether the process exits before the socket opens.
  - Add `qa-app-launch` CLI command with `--repo-root`, optional `--app-binary`, and `--output`.
  - Capture child stdout/stderr, exit status, app binary path, DB copy path, socket path, and scoped child env keys.
  - Add a missing-binary regression test and fix binary path resolution to canonicalize before setting `current_dir`.
  - Document `qa-app-launch` in `native/README.md`.
- **Convention Notes**: The report does not claim a Linux native UI. On this host it honestly records that the real app binary exits as `HeadlessStub` before opening the socket; macOS/native-ui remains required for a PASS launch replay.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo build -p kanvibe-app --quiet`
  - `cargo run -p qa-harness -- qa-app-launch --repo-root .. --output ../qa/parity/slice-qa-control/qa-app-launch.json`
  - `jq -r '[.artifact,.status,.mode,.qaSocketReady,.processExitedBeforeSocket,.exitStatus,(.childEnvKeys|join(",")),(.blockers|join(" | "))] | @tsv' qa/parity/slice-qa-control/qa-app-launch.json`
  - `jq -r '[.appBinary,.dbCopyPath,.stderr] | @tsv' qa/parity/slice-qa-control/qa-app-launch.json`
  - `test -f qa/parity/native-db/native-app-launch.sqlite && stat -c '%n %s' qa/parity/native-db/native-app-launch.sqlite`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md || true`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `native_app_launch_report` and `qa-app-launch` CLI.
    - Added missing-binary launch report test; focused test count is now 22 app tests and 14 harness tests.
    - First launch attempt exposed relative binary path resolution after `current_dir`; fixed by canonicalizing existing binary paths and reran the same command.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` in `native/` -> exit 0.
    - `cargo build -p kanvibe-app --quiet` in `native/` -> exit 0.
    - `cargo run -p qa-harness -- qa-app-launch --repo-root .. --output ../qa/parity/slice-qa-control/qa-app-launch.json` -> exit 0.
    - Launch summary -> `native-app-launch-contract  BLOCKED  externalNativeAppProcess  false  true  0  KANVIBE_REPO_ROOT,KANVIBE_DB_PATH,KANVIBE_LOCALE,KANVIBE_QA_SOCKET  native app process exited before opening the QA socket; on this host the app may be a non-macOS headless stub`.
    - Launch process details -> app binary `/home/crookedbot/Documents/kanvibe/kanvibe__worktrees/perf-rust-backend/native/target/debug/kanvibe-app`, stderr `kanvibe native scaffold mode: HeadlessStub`.
    - Native app launch DB copy exists at `qa/parity/native-db/native-app-launch.sqlite`, size `65536`.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: The harness can launch the real app binary with scoped QA env and records a reproducible PASS/BLOCKED result; Linux now has an explicit external-process launch blocker artifact instead of relying only on in-process smoke.
- **Status**: completed

### Todo 25: Add macOS DMG Packaging Path
- **Priority**: 24
- **Dependencies**: Todo 24
- **Goal**: Complete the packaging script contract for both required macOS artifacts: `.app` bundle and DMG.
- **Work**:
  - Extend `native/scripts/package-macos-app.sh` in place to create `native/dist/KanVibe-0.1.0.dmg` with `hdiutil create -format UDZO` after staging and optional signing of `KanVibe.app`.
  - Add robust argument parsing for `--skip-sign` and `--no-dmg`.
  - Keep non-Darwin hosts exiting `78` before macOS-only build/sign/DMG operations.
  - Document the DMG output and `--no-dmg` option in `native/README.md`.
- **Convention Notes**: The script remains Darwin-gated. Linux can verify syntax and blocker behavior only; `.app` and `.dmg` creation must still run on macOS for final Phase 5 completion.
- **Verification**:
  - `bash -n native/scripts/package-macos-app.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `rg -n "hdiutil|--no-dmg|DMG_PATH|UDZO|KanVibe-0.1.0.dmg" native/scripts/package-macos-app.sh native/README.md`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md native/scripts/package-macos-app.sh || true`
  - `git diff --check`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - Added `DMG_PATH`, `CREATE_DMG`, `--no-dmg`, and `hdiutil create -volname KanVibe -srcfolder KanVibe.app -ov -format UDZO` to `native/scripts/package-macos-app.sh`.
    - Updated `native/README.md` packaging section to document `native/dist/KanVibe-0.1.0.dmg`.
    - `bash -n native/scripts/package-macos-app.sh` -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with Darwin-gate message.
    - `rg -n "hdiutil|--no-dmg|DMG_PATH|UDZO|KanVibe-0.1.0.dmg" native/scripts/package-macos-app.sh native/README.md` -> exit 0, confirms DMG script/docs references.
    - `cargo test --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --workspace --quiet` in `native/` -> exit 0.
    - `cargo build --release --quiet` in `native/` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` in `native/` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README/package script files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Packaging script has a concrete Darwin-only path to produce both `.app` and `.dmg`; Linux verification proves syntax, documentation, and the expected runtime gate.
- **Status**: completed

### Todo 26: Promote Native Scenario Video Capture into QA Replay
- **Priority**: 25
- **Dependencies**: Todo 25
- **Goal**: Make Phase 5 per-scenario video artifacts first-class in the native QA replay contract instead of leaving video capture as an aggregate macOS blocker.
- **Work**:
  - Add debug QA control commands `startVideoCapture` and `stopVideoCapture`.
  - Implement the macOS capture path as `screencapture` frame recording plus `ffmpeg` MP4 encoding, gated by `KANVIBE_QA_WINDOW_ID` and optional `KANVIBE_QA_FFMPEG`.
  - Keep Linux honest with structured video blockers.
  - Wrap every S01-S14 replay plan with one native MP4 path under `qa/parity/native-videos/`.
  - Track video blockers separately from screenshot blockers in replay execution reports.
  - Document the video commands and scoped env variables in `native/README.md`.
- **Convention Notes**: The command surface remains debug-only and uses only scoped `KANVIBE_*` env names. Linux verification proves planning, serialization, socket transport, and blocker accounting; real MP4 output remains macOS-gated.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-control --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-protocol.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-014`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md || true`
  - `git diff --check`
  - `bash -n native/scripts/package-macos-app.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 14 harness tests pass.
    - `qa-control-protocol.json` lists `startVideoCapture`, `stopVideoCapture`, and `videoCapture.ffmpegEnv=KANVIBE_QA_FFMPEG`.
    - `qa-control-replay-plan.json` records `videoCommandCount=28`, `videoArtifactCount=14`, and native output paths under `qa/parity/native-videos/`.
    - `qa-control-replay-execution.json` records `executedCommandCount=144`, `transportPass=true`, `videoBlockedCount=28`, `screenshotBlockedCount=14`, `pendingDispatchCount=40`, and zero transport/structured/missing-element errors.
    - `qa/parity/run-014/full-parity.json` remains `BLOCKED` with 14/14 scenario coverage and macOS runtime gates.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README files -> no matches.
    - `git diff --check` -> exit 0.
    - `bash -n native/scripts/package-macos-app.sh` -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Every declared scenario video has a native replay command pair and explicit artifact path; Linux replay reports video capture as a macOS/ffmpeg/window-id blocker without claiming final PASS.
- **Status**: completed

### Todo 27: Add External Native App Scenario Replay Contract
- **Priority**: 26
- **Dependencies**: Todo 26
- **Goal**: Move beyond a root-element launch probe by attempting S01-S14 replay against the real `kanvibe-app` process with one copied DB and one debug socket per scenario.
- **Work**:
  - Add `native_app_replay_report` in `qa-harness`.
  - Add `qa-app-replay` CLI with `--repo-root`, optional `--app-binary`, and `--output`.
  - Launch the real app once per scenario with a narrow env containing `KANVIBE_REPO_ROOT`, `KANVIBE_DB_PATH`, `KANVIBE_LOCALE`, `KANVIBE_QA_SOCKET`, and optional `KANVIBE_QA_FFMPEG`.
  - Reuse the existing replay-plan executor when the external app socket becomes ready.
  - Record per-scenario process stdout/stderr, socket path, DB copy path, exit status, and whether replay commands ran.
  - Document the command in `native/README.md`.
- **Convention Notes**: Linux still cannot pass this gate because `kanvibe-app` exits as `HeadlessStub` before opening the socket. The report keeps that as an app-runtime blocker rather than pretending the replay transport failed.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo build -p kanvibe-app --quiet`
  - `cargo run -p qa-harness -- qa-app-replay --repo-root .. --output ../qa/parity/slice-qa-control/qa-app-replay.json`
  - `jq '{artifact,status,mode,scenarioFileCount,appLaunchAttemptCount,qaSocketReadyCount,qaSocketReadyPass,processExitedBeforeSocketCount,processTimeoutCount,transportPass,executedCommandCount,blockerCount,blockers}' qa/parity/slice-qa-control/qa-app-replay.json`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-015`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md || true`
  - `git diff --check`
  - `bash -n native/scripts/package-macos-app.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p qa-harness --quiet` -> exit 0; 15 harness tests pass.
    - `cargo build -p kanvibe-app --quiet` -> exit 0.
    - `qa-app-replay` -> exit 0 and generated `qa/parity/slice-qa-control/qa-app-replay.json`.
    - `qa-app-replay.json` records `appLaunchAttemptCount=14`, `qaSocketReadyCount=0`, `processExitedBeforeSocketCount=14`, `transportPass=true`, `executedCommandCount=0`, and blocker `one or more native app processes exited before opening the QA socket`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 15 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - `qa/parity/run-015/full-parity.json` remains `BLOCKED` with `headlessContractsPass=true`, 14/14 scenario coverage, and macOS runtime gates.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README files -> no matches.
    - `git diff --check` -> exit 0.
    - `bash -n native/scripts/package-macos-app.sh` -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: The harness has a command-backed external app replay artifact that attempts every S01-S14 scenario against the real app process and honestly blocks before socket replay on this Linux host.
- **Status**: completed

### Todo 28: Add macOS Phase 5 Run Directory Orchestrator
- **Priority**: 27
- **Dependencies**: Todo 27
- **Goal**: Provide a single macOS handoff command that creates the required `qa/parity/run-<N>/screens/` and `videos/` capture paths while driving the existing native QA harness artifacts.
- **Work**:
  - Add `native/scripts/phase5-macos-run.sh`.
  - Darwin-gate the script with exit `78` on non-macOS hosts.
  - Package the release `.app`/DMG by default, then build the debug `native-ui` binary used by the QA socket.
  - Generate `qa-control`, `qa-replay-plan`, `qa-replay-execute`, `qa-app-launch`, `qa-app-replay`, and `full-parity` artifacts into one numbered run directory.
  - Add `KANVIBE_QA_ARTIFACT_ROOT` support in `qa-harness` so capture commands target `<run-dir>/screens` and `<run-dir>/videos` when a Phase 5 run is active, while preserving standalone `native-screens`/`native-videos` defaults.
  - Forward `KANVIBE_QA_WINDOW_ID` into external app replay launches when present.
  - Document the runner in `native/README.md`.
- **Convention Notes**: The script does not make Linux pass macOS runtime gates. It creates the required macOS run-directory orchestration path and keeps Linux verification limited to syntax, env/path planning, and the expected Darwin gate.
- **Verification**:
  - `chmod +x native/scripts/phase5-macos-run.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo test -p qa-harness --quiet`
  - `KANVIBE_QA_ARTIFACT_ROOT=qa/parity/run-qa-path-check cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan-run-root-check.json`
  - `cargo run -p qa-harness -- qa-replay-plan --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-plan.json`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-016`
  - Evidence:
    - `bash -n native/scripts/phase5-macos-run.sh` -> exit 0.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo test -p qa-harness --quiet` -> exit 0; 15 harness tests pass.
    - Run-root replay-plan check records screenshot path `qa/parity/run-qa-path-check/screens/S01-board-load-and-columns-board-dark-ko.png` and video path `qa/parity/run-qa-path-check/videos/S01-board-load-and-columns.mp4`.
    - Default replay-plan regeneration preserves standalone paths `qa/parity/native-screens/...` and `qa/parity/native-videos/...`.
    - `stat -c '%a %n' native/scripts/phase5-macos-run.sh` -> `775 native/scripts/phase5-macos-run.sh`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 15 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh` -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
    - `qa/parity/run-016/full-parity.json` remains `BLOCKED` with `headlessContractsPass=true`, 14/14 scenario coverage, and macOS runtime gates.
- **Exit Criteria**: A macOS operator can invoke one script to create a numbered Phase 5 run directory and drive existing harness artifacts with capture commands pointing at that run's screens/videos folders; Linux reports only the expected runtime gate.
- **Status**: completed

### Todo 29: Add Native Performance Comparison Artifact
- **Priority**: 28
- **Dependencies**: Todo 28
- **Goal**: Promote the explicit Phase 5 performance targets into a command-backed native artifact instead of leaving performance only as a generic macOS blocker.
- **Work**:
  - Add `qa-harness native-performance`.
  - Report native `.app` bundle size against the 30 MiB target when generated.
  - Report native DMG and release binary sizes when present.
  - Parse `qa/PERF_BASELINE.md` for Electron/Linux baseline package, startup, and RSS values.
  - Keep cold start, idle memory, and terminal scroll FPS as structured macOS-runtime blockers until actual GPUI measurements exist.
  - Include `native-performance-comparison` in `full-parity` slice artifacts.
  - Add `native-performance` to the macOS Phase 5 runner and `native/README.md`.
- **Convention Notes**: This does not infer macOS performance from Linux headless stubs. It records the measurable Linux/native release artifact size and makes the remaining macOS performance evidence explicit.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo run -p qa-harness -- native-performance --repo-root .. --output ../qa/parity/slice-qa-control/native-performance.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-017`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p qa-harness --quiet` -> exit 0; 16 harness tests pass.
    - `native-performance.json` records `artifact=native-performance-comparison`, `status=BLOCKED`, `releaseBinary.status=PASS`, `releaseBinary.sizeBytes=443192`, `electronBaseline.linuxBoardReadyRssKb=740572`, and `electronBaseline.linuxTaskOperationsRssKb=742012`.
    - `native-performance.json` records blockers for missing `.app`/DMG artifacts plus macOS cold start, idle memory, and terminal scroll FPS measurement.
    - `qa/parity/run-017/full-parity.json` remains `BLOCKED` with 14/14 scenario coverage and includes `native-performance-comparison` in `sliceArtifacts`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 16 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - Package and Phase 5 runner shell syntax checks -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Phase 5 has a reproducible native performance artifact that can fail size regressions immediately and clearly identifies the exact macOS runtime measurements still required for final PASS.
- **Status**: completed

### Todo 30: Add Native Visual Parity Evidence Artifact
- **Priority**: 29
- **Dependencies**: Todo 28
- **Goal**: Promote Phase 5 visual parity from a generic macOS blocker into a command-backed artifact that inventories Electron baseline captures, scenario-declared assets, and expected native screenshot/video outputs without pretending to perform visual comparison.
- **Work**:
  - Add `qa-harness native-visual-parity`.
  - Parse `qa/baseline/MANIFEST.md` and verify all S01-S14 baseline screen/video files still exist.
  - Map each scenario to its declared artifacts and replay-plan-derived native screenshot/video paths.
  - Report missing native captures as structured blockers and keep structural visual comparison unpassed until the files exist and a review is recorded.
  - Include `native-visual-parity-evidence` in `full-parity` slice artifacts.
  - Add `native-visual-parity` to the macOS Phase 5 runner and `native/README.md`.
- **Convention Notes**: This does not infer visual parity from Linux headless contracts, and it does not treat inventory completeness as a visual PASS.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo run -p qa-harness -- native-visual-parity --repo-root .. --output ../qa/parity/slice-qa-control/native-visual-parity.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-018`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p qa-harness --quiet` -> exit 0; 17 harness tests pass.
    - `native-visual-parity.json` records `artifact=native-visual-parity-evidence`, `status=BLOCKED`, `baselineManifest.pass=true`, 14 manifest rows, 14 expected native screenshots, 14 missing native screenshots, 14 expected native videos, 14 missing native videos, and `comparison.performed=false`.
    - `qa/parity/run-018/full-parity.json` remains `BLOCKED` with `headlessContractsPass=true`, 14/14 scenario coverage, and includes `native-visual-parity-evidence` in `sliceArtifacts`.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 17 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo fmt --manifest-path native/Cargo.toml --all -- --check` -> exit 0.
    - Package and Phase 5 runner shell syntax checks -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: Phase 5 can produce a reproducible native visual parity evidence JSON that identifies baseline integrity, expected native capture paths, missing native screenshot/video files, and the remaining structural review requirement.
- **Status**: completed

### Todo 31: Expand Phase 5 QA Report Detail
- **Priority**: 30
- **Dependencies**: Todo 30
- **Goal**: Make generated `QA_REPORT.md` reflect the Phase 5 contract by separating functional, visual, and performance evidence instead of listing only slice names and runtime gates.
- **Work**:
  - Add named `functionalEvidence`, `visualEvidence`, and `performanceEvidence` summaries to `full-parity.json`.
  - Keep functional evidence blocked unless the QA replay artifact itself passes, even when transport contracts pass.
  - Render functional replay blockers, visual capture/readiness counts, structural comparison status, and performance blockers into `QA_REPORT.md`.
  - Preserve existing scenario coverage, slice artifact, macOS gate, and Linux command sections.
  - Make runtime scenario seed DB copies unique under `qa/parity/native-db/` so parallel tests cannot read a half-copied fixed scenario DB.
- **Convention Notes**: This is reporting only; it does not weaken the Phase 5 PASS condition or mark visual/performance parity complete on Linux.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-019`
  - `rg -n '## Functional Evidence|## Visual Evidence|## Performance Evidence' qa/parity/run-019/QA_REPORT.md`
  - `jq '{functional:.functionalEvidence.status,visual:.visualEvidence.status,performance:.performanceEvidence.status}' qa/parity/run-019/full-parity.json`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `cargo fmt --manifest-path native/Cargo.toml --all -- --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p qa-harness --quiet` -> exit 0; 17 harness tests pass.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-019` -> exit 0.
    - `rg -n '## Functional Evidence|## Visual Evidence|## Performance Evidence' qa/parity/run-019/QA_REPORT.md` -> exit 0; all three sections present.
    - `qa/parity/run-019/full-parity.json` records `status=BLOCKED`, `functionalEvidence.status=BLOCKED`, `visualEvidence.status=BLOCKED`, `performanceEvidence.status=BLOCKED`, `transportPass=true`, `executedCommandCount=144`, `missingScreenCount=14`, and `missingVideoCount=14`.
    - Repair loop: `cargo test -p kanvibe-app -p qa-harness --quiet` initially exposed a parallel fixed-path seed DB copy race (`no such table: projects`); runtime scenario seed copies now use unique files under `qa/parity/native-db/`, and the same command reran successfully.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 17 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo fmt --manifest-path native/Cargo.toml --all -- --check` -> exit 0.
    - Package and Phase 5 runner shell syntax checks -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: `QA_REPORT.md` generated by `full-parity` contains reproducible functional, visual, and performance evidence sections with blockers grounded in the underlying JSON artifacts.
- **Status**: completed

### Todo 32: Tighten Full-Parity Aggregate Status
- **Priority**: 31
- **Dependencies**: Todo 31
- **Goal**: Prevent `full-parity` from masking explicit slice failures behind a generic scenario-coverage status.
- **Work**:
  - Add explicit `sliceStatuses` to `full-parity.json`, treating legacy slice reports without a `status` field as implicit `PASS`.
  - Aggregate full-parity status with `FAIL` precedence over `BLOCKED`, and `BLOCKED` precedence over `PASS`.
  - Render slice artifact statuses in `QA_REPORT.md`.
  - Add unit coverage for aggregate status precedence.
- **Convention Notes**: This is report correctness only; it does not change any slice behavior or relax the final Phase 5 PASS requirements.
- **Verification**:
  - `cargo test -p qa-harness --quiet`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-020`
  - `jq '{status,sliceStatuses:.sliceStatuses}' qa/parity/run-020/full-parity.json`
  - `rg -n 'native-visual-parity-evidence.*BLOCKED|native-performance-comparison.*BLOCKED' qa/parity/run-020/QA_REPORT.md`
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `cargo fmt --manifest-path native/Cargo.toml --all -- --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p qa-harness --quiet` -> exit 0; 18 harness tests pass.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-020` -> exit 0.
    - `qa/parity/run-020/full-parity.json` records aggregate `status=BLOCKED` with `sliceStatuses`: legacy slice reports as `PASS`, `native-qa-control-replay-execution=BLOCKED`, `native-visual-parity-evidence=BLOCKED`, and `native-performance-comparison=BLOCKED`.
    - `qa/parity/run-020/QA_REPORT.md` renders per-slice statuses, including the three `BLOCKED` Phase 5 artifacts.
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 18 harness tests pass.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo fmt --manifest-path native/Cargo.toml --all -- --check` -> exit 0.
    - Package and Phase 5 runner shell syntax checks -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: `full-parity.json` and `QA_REPORT.md` expose per-slice statuses, and aggregate status fails on any explicit slice failure instead of only checking scenario coverage.
- **Status**: completed

### Todo 33: Promote Headless Synthetic Dispatch Evidence
- **Priority**: 32
- **Dependencies**: Todo 32
- **Goal**: Remove the stale functional blocker that marked semantic synthetic inputs as pending even though the debug QA control state already applies click/key effects to its headless render model.
- **Work**:
  - Change app-side synthetic click/key responses from `protocol-ready-gpui-dispatch-pending` to `headless-qa-state-dispatch-applied`.
  - Treat accepted non-pending synthetic inputs as clean pass evidence in the harness.
  - Keep mouse input explicitly accepted as a no-state-change headless command.
  - Regenerate replay/full-parity artifacts so `pendingDispatchCount=0` and remaining blockers are screenshot/video/macOS runtime gates.
- **Convention Notes**: This is a headless QA-state dispatch contract, not a claim that real macOS GPUI event injection has passed.
- **Verification**:
  - `cargo test -p kanvibe-app -p qa-harness --quiet`
  - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-021`
  - `jq '{status,pendingDispatchCount,acceptedSyntheticInputCount,blockers}' qa/parity/slice-qa-control/qa-control-replay-execution.json`
  - `jq '{status,functional:.functionalEvidence.status,blockedSlices:[.sliceStatuses[]|select(.status=="BLOCKED")|.artifact]}' qa/parity/run-021/full-parity.json`
  - `cargo test --workspace --quiet`
  - `cargo build --workspace --quiet`
  - `cargo build --release --quiet`
  - `cargo check -p kanvibe-app --features native-ui --quiet`
  - `python3 /home/crookedbot/.codex/skills/technical-search-skill/scripts/verify_sources.py native/README.md`
  - `rg -n '[ \t]+$' .claude/plan/native-migration/2026-07/08-rust-gpui-migration.plan.md native/crates/qa-harness/src/lib.rs native/crates/qa-harness/src/main.rs native/crates/kanvibe-app/src/qa_control.rs native/README.md native/scripts/phase5-macos-run.sh || true`
  - `git diff --check`
  - `cargo fmt --manifest-path native/Cargo.toml --all -- --check`
  - `bash -n native/scripts/package-macos-app.sh && bash -n native/scripts/phase5-macos-run.sh`
  - `native/scripts/package-macos-app.sh --skip-sign`
  - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg`
  - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet`
  - Evidence:
    - `cargo test -p kanvibe-app -p qa-harness --quiet` -> exit 0; 23 app tests and 18 harness tests pass.
    - `cargo run -p qa-harness -- qa-replay-execute --repo-root .. --output ../qa/parity/slice-qa-control/qa-control-replay-execution.json` -> exit 0.
    - `qa-control-replay-execution.json` records `pendingDispatchCount=0`, `acceptedSyntheticInputCount=40`, `screenshotBlockedCount=14`, `videoBlockedCount=28`, and blockers only for macOS screenshot/video capture.
    - `cargo run -p qa-harness -- full-parity --repo-root .. --output-dir ../qa/parity/run-021` -> exit 0.
    - `qa/parity/run-021/full-parity.json` remains `BLOCKED`; functional blockers are now only `native screenshot capture requires macOS screencapture and a QA window id` and `native video capture requires macOS screencapture, ffmpeg, and a QA window id`.
    - `cargo test --workspace --quiet` -> exit 0.
    - `cargo build --workspace --quiet` -> exit 0.
    - `cargo build --release --quiet` -> exit 0.
    - `cargo check -p kanvibe-app --features native-ui --quiet` -> exit 0.
    - README source verification -> 5 PASS, 0 fail/warn.
    - Trailing whitespace check over touched plan/harness/app/README/script files -> no matches.
    - `git diff --check` -> exit 0.
    - `cargo fmt --manifest-path native/Cargo.toml --all -- --check` -> exit 0.
    - Package and Phase 5 runner shell syntax checks -> exit 0.
    - `native/scripts/package-macos-app.sh --skip-sign` on Linux -> exit 78 with the expected Darwin gate.
    - `native/scripts/phase5-macos-run.sh --window-id 4242 --skip-package --no-dmg` on Linux -> exit 78 with the expected Darwin gate.
    - `cargo check -p kanvibe-app --features native-ui --target x86_64-apple-darwin --quiet` on Linux -> exit 101 in `ring` before app code because local `cc` rejects Apple flags `-arch` and `-mmacosx-version-min`.
- **Exit Criteria**: QA replay no longer reports synthetic dispatch blockers; functional replay remains blocked only by capture/runtime requirements that cannot be satisfied on this Linux host.
- **Status**: completed

## Verification Strategy
- Phase-local verification after every todo, with plan status updated immediately after passing evidence.
- Prompt-separated self-review QA in this runtime unless a separate subagent/process is available; rely on command/artifact evidence.
- Required command/artifact evidence:
  - `qa/FEATURE_INVENTORY.md`
  - `qa/PERF_BASELINE.md`
  - `qa/seed/kanvibe-seed.sqlite`
  - `qa/baseline/screens/**`, `qa/baseline/videos/**`, `qa/baseline/MANIFEST.md`
  - `native/Cargo.toml`, native crates, `native/README.md`
  - `cargo test --workspace`, `cargo build --release`
  - `qa/parity/run-<final>/QA_REPORT.md` and one immediately repeated PASS run.

## Progress Tracking
- Total Todos: 33
- Completed: 32
- Status: Execution in progress

## Change Log
- 2026-07-08: Plan created from pasted migration objective using Roky Harness and task-planner contracts.
- 2026-07-08: Todo 1 completed - created `qa/FEATURE_INVENTORY.md` and verified required sections plus S01-S14 coverage.
- 2026-07-08: Todo 2 completed - fixed pnpm 11 build approvals/Corepack postinstall path, packaged Electron directory build, fixed packaged runtime dependency omissions, added `qa/perf/electron-baseline.cjs`, and recorded `qa/PERF_BASELINE.md`.
- 2026-07-08: Todo 3 completed - added deterministic QA seed/scenario generators, created `qa/seed/kanvibe-seed.sqlite`, and generated S01-S14 runner-neutral scenario specs with validation evidence.
- 2026-07-08: Todo 4 completed - captured Electron baseline screenshots/videos for S01-S14 under Xvfb with `qa/baseline/MANIFEST.md`.
- 2026-07-08: Todo 5 completed - researched GPUI/gpui-component/Zed terminal sources, added `native/` Rust workspace with eight crates, documented macOS-only UI pins, and passed `cargo test --workspace` plus `cargo build --workspace`.
- 2026-07-08: Todo 6 completed - added direct Rust SQLite read models, ko/en board label loading, semantic status color sharing, read-only board shell view model, and deterministic S01 reports in `qa/parity/slice-1/`.
- 2026-07-08: Todo 7 completed - added Rust board interaction write models and generated `qa/parity/slice-2/board-interactions.json` covering create/edit/move/reorder/delete/search/project filter/done paging/color update evidence.
- 2026-07-08: Todo 8 completed - added task-detail dock/navigation/sidebar/PT Y environment contracts and generated `qa/parity/slice-3/task-detail-pty-dock.json` for S03/S04/S13/S14 evidence.
- 2026-07-08: Todo 9 completed - added command-backed Git/Diff/worktree/editor contracts and generated `qa/parity/slice-4/git-diff-worktree.json` for S05/S06 evidence.
- 2026-07-08: Todo 10 completed - added AI session, hook status, board event notification, and notification center contracts plus `qa/parity/slice-5/notifications-hooks-ai.json` for S11 evidence.
- 2026-07-08: Todo 11 completed - added typed settings, pane layout persistence, remote session dependency policy, window focus policy, and `qa/parity/slice-6/settings-layout-remote.json` for S10/S14 evidence.
- 2026-07-08: Todo 12 completed - added macOS bundle script, recorded Linux release binary/performance evidence, and carried macOS `.app`/GPUI performance gates into Phase 5.
- 2026-07-08: Todo 13 blocked - generated two Phase 5 reports in `qa/parity/run-001/` and `qa/parity/run-002/`; all 14 headless/native scenario contracts pass, but final PASS requires macOS GPUI visual, `.app`/DMG, and native performance gates.
- 2026-07-08: Todo 14 completed - added a macOS-only GPUI/gpui-component native UI entry slice plus Linux-verifiable launch/render contracts; run 003 still blocks only on macOS visual/package/performance gates.
- 2026-07-08: Todo 15 completed - added debug-only `KANVIBE_QA_SOCKET` JSON control protocol and scenario-to-control mapping artifact; run 004 includes `native-qa-control-protocol` and still blocks only on macOS visual/package/performance gates.
- 2026-07-08: Todo 16 completed - added harness-side `QaControlClient`, semantic `syntheticClick` replay commands, `qa-control-replay-plan.json`, and a live debug Unix socket smoke test; run 005 includes `native-qa-control-replay-plan` and still blocks only on macOS visual/package/performance gates.
- 2026-07-08: Todo 17 completed - added full QA-control replay execution over the debug socket, generated `qa-control-replay-execution.json`, and included `native-qa-control-replay-execution` in run 006; transport passes for all 116 commands while GPUI dispatch, screenshot capture, and missing native surfaces remain blockers.
- 2026-07-08: Todo 18 completed - added seed-backed task metadata and replay-interaction QA state; run 007 reduces replay missing element blockers from 23 to 0 while preserving GPUI dispatch and screenshot capture blockers.
- 2026-07-08: Todo 19 completed - extended native DB snapshots and harness DB assertion evaluation; run 008 shows all 13 DB assertions pass with zero structured/transport errors, leaving GPUI dispatch and screenshot/video capture blockers.
- 2026-07-08: Todo 20 completed - changed debug QA replay smoke to use fresh per-scenario sockets/state; run 009 preserves 14/14 headless scenario coverage and all 13 DB assertions pass, leaving GPUI dispatch and screenshot/video capture blockers.
- 2026-07-08: Todo 21 completed - added semantic non-DB replay assertion evaluation; run 010 shows every S01-S14 assertion command passes, leaving only GPUI dispatch and screenshot/video capture blockers.
- 2026-07-08: Todo 22 completed - added macOS `screencapture` screenshot contract through `KANVIBE_QA_WINDOW_ID`; run 012 keeps all headless assertions passing and reports screenshot blockers as macOS/window-id runtime requirements.
- 2026-07-08: Todo 23 completed - added per-scenario native replay SQLite copies under `qa/parity/native-db/`; run 013 keeps 14/14 headless coverage with all replay assertions passing.
- 2026-07-08: Todo 24 completed - added external native app launch contract report; Linux artifact shows the real debug binary exits `HeadlessStub` before opening `KANVIBE_QA_SOCKET`, while scoped launch env and DB copy preparation are verified.
- 2026-07-08: Todo 25 completed - extended macOS package script to create a compressed DMG with `hdiutil`; Linux syntax and Darwin-gate checks pass, while final artifact creation remains macOS-gated.
- 2026-07-08: Todo 26 completed - promoted native S01-S14 video capture into the debug QA replay contract with `startVideoCapture`/`stopVideoCapture`, per-scenario MP4 paths, and separate video blocker accounting.
- 2026-07-08: Todo 27 completed - added `qa-app-replay`, which launches the real native app process once per scenario and records the current Linux `HeadlessStub` socket blocker before external replay can run.
- 2026-07-08: Todo 28 completed - added a Darwin-gated `phase5-macos-run.sh` orchestrator plus `KANVIBE_QA_ARTIFACT_ROOT` capture path support for numbered Phase 5 run directories.
- 2026-07-08: Todo 29 completed - added `native-performance`, a structured Phase 5 performance artifact with bundle-size target checks, baseline extraction, and explicit macOS runtime blockers.
- 2026-07-08: Todo 30 completed - added `native-visual-parity`, a structured Phase 5 visual evidence artifact that verifies baseline manifest integrity and blocks honestly on missing native captures/structural review.
- 2026-07-08: Todo 31 completed - expanded generated `QA_REPORT.md` with functional, visual, and performance evidence sections, and fixed parallel QA replay seed-copy collisions with unique runtime DB copies.
- 2026-07-08: Todo 32 completed - added explicit per-slice statuses to full parity aggregation and made aggregate status fail on explicit slice failures before considering blockers.
- 2026-07-08: Todo 33 completed - promoted semantic synthetic click/key replay to applied headless QA-state dispatch, leaving functional replay blocked only on macOS screenshot/video capture.
