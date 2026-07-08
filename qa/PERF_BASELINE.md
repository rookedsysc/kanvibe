# Electron Performance Baseline

Captured: 2026-07-08 UTC

Scope: Phase 1 Electron baseline for the Rust + GPUI migration. This capture was run on Linux, so `electron-builder --dir` produced `dist/linux-unpacked`. A macOS `.app`/DMG baseline still needs to be captured on macOS before macOS parity signoff.

## Environment

- Host: `Linux crookedbot 7.0.2-6-pve x86_64`
- Node: `v24.16.0`
- pnpm: `11.6.0`
- Packaged Electron: `32.3.3`
- Packaged app runtime: Electron Node `20.18.1`
- Display for startup run: `xvfb-run -a -s '-screen 0 1440x960x24'`

## Package Build

Command:

```sh
tmp_corepack_bin=$(mktemp -d)
corepack enable --install-directory "$tmp_corepack_bin"
PATH="$tmp_corepack_bin:$PATH" /usr/bin/time -v pnpm dist:dir
```

Notes:

- A temporary Corepack shim was used because the package scripts call a literal `pnpm` binary recursively.
- `corepack pnpm install` passed before packaging, including the postinstall `better-sqlite3` Electron rebuild.
- `pnpm-workspace.yaml` records pnpm 11 build approvals.
- `packageManager: pnpm@11.6.0` lets Electron Builder detect pnpm from project metadata.
- `dunder-proto` and `ms` are explicit production dependencies so Electron Builder includes runtime dependencies required by `get-proto` and `debug` in `app.asar`.

Final successful package build:

| Metric | Value |
| --- | ---: |
| Exit status | 0 |
| Wall time | 2:58.54 |
| User time | 34.82 s |
| System time | 8.85 s |
| Max resident set | 813,604 KB |
| Renderer build | 690 modules, 17.55 s |

Largest renderer chunks:

| Chunk | Size | Gzip |
| --- | ---: | ---: |
| `index-CDklwkz5.js` | 845.52 KB | 251.65 KB |
| `xterm-B-qIQCd3.js` | 329.31 KB | 83.04 KB |
| `TaskDetailRoute-DdLuwEh0.js` | 72.10 KB | 21.95 KB |

Artifact sizes:

| Artifact | Size |
| --- | ---: |
| `dist/linux-unpacked` | 425,578,021 bytes |
| `dist/linux-unpacked/kanvibe` | 185,784,416 bytes |
| `dist/linux-unpacked/resources/app.asar` | 81,394,656 bytes |
| `build/renderer` | 1.4 MB |
| `build/main` | 470 KB |
| `resources/database/app.seed.db` | 57,344 bytes |

## Startup And Runtime

Command:

```sh
xvfb-run -a -s '-screen 0 1440x960x24' node qa/perf/electron-baseline.cjs --iterations 5
```

Result: PASS

Output JSON: `qa/perf-output/electron-baseline-2026-07-08T02-12-54-669Z/electron-baseline.json` (ignored generated output)

Startup:

| Metric | Value |
| --- | ---: |
| First page observed | 3,692.34 ms |
| Board ready | 6,245.35 ms |

Memory, summed across the Electron process tree:

| Point | Process count | RSS |
| --- | ---: | ---: |
| After board ready | 6 | 740,572 KB / 723.21 MB |
| After task operations | 6 | 742,012 KB / 724.62 MB |

Task operation latency:

| Operation | Count | Avg | Median | P95 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| `kanban.getTasksByStatus.initial` | 1 | 4.75 ms | 4.75 ms | 4.75 ms | 4.75 ms |
| `kanban.createTask.noWorktree` | 5 | 3.51 ms | 1.92 ms | 10.05 ms | 10.05 ms |
| `kanban.updateTaskStatus.progress` | 5 | 2.40 ms | 1.95 ms | 4.45 ms | 4.45 ms |
| `kanban.getTasksByStatus.afterUpdate` | 5 | 1.61 ms | 1.61 ms | 1.99 ms | 1.99 ms |
| `kanban.deleteTask.noWorktree` | 5 | 1.95 ms | 1.69 ms | 3.08 ms | 3.08 ms |

## Validation

- `dist/linux-unpacked/resources/app.asar` contains `node_modules/dunder-proto`, `node_modules/get-proto`, and `node_modules/get-intrinsic`.
- Successful run diagnostics show `main:startup`, `renderer:load-complete`, board IPC calls, and measured task IPC calls.
- No `main:unhandled-rejection`, `main:uncaught-exception`, or `renderer:did-fail-load` was observed in the successful run diagnostics.

## Native Rust Release Check

Captured: 2026-07-08 UTC

Scope: Phase 4/Todo 12 native release build check on Linux. This verifies portable Rust contracts and the release binary, but it is not the final macOS GPUI package/startup measurement because `kanvibe-app` runs as `HeadlessStub` on non-macOS targets unless the macOS `native-ui` feature is enabled.

Release build command:

```sh
/usr/bin/time -v cargo build --release --quiet
```

Run from: `native/`

| Metric | Value |
| --- | ---: |
| Exit status | 0 |
| Wall time | 0:49.78 |
| User time | 53.70 s |
| System time | 3.29 s |
| Max resident set | 433,396 KB |

Release artifact sizes:

| Artifact | Size |
| --- | ---: |
| `native/target/release/kanvibe-app` | 443,192 bytes |
| `native/target/release/qa-harness` | 3,369,272 bytes |
| `native/target/release` | 73,103,783 bytes |

Release scaffold startup command:

```sh
/usr/bin/time -v native/target/release/kanvibe-app
```

Result:

| Metric | Value |
| --- | ---: |
| Exit status | 0 |
| Mode | `HeadlessStub` |
| Elapsed wall time | 0:00.00 |
| Max resident set | 1,992 KB |

macOS bundle script:

```sh
native/scripts/package-macos-app.sh
```

Linux verification result: exit 78 with the expected message that `.app` bundling requires Darwin because GPUI/native-ui and codesign are macOS runtime gates. On macOS, the script builds `kanvibe-app` with `--features native-ui`, creates `native/dist/KanVibe.app`, writes `Info.plist`/`PkgInfo`, and ad-hoc signs with `codesign --sign -` when available.

## Open Baseline Gaps

- macOS packaging, startup, memory, and task-operation numbers are not captured in this Linux environment.
- The current numbers are Electron/Linux directory-build numbers, not native Rust/GPUI targets.
- Native macOS `.app`/DMG packaging and GPUI cold-start, idle memory, and terminal scroll FPS still require a macOS runtime.
