# Issue #310 — GPUI terminal fork QA

Date: 2026-07-29
Scope: Todo 4 terminal scrollback, selection, clipboard, and app dependency integration

## Outcome

Portable implementation gates are green. Real macOS S03/S04/S13/S14 evidence
remains mandatory and this report does not approve Electron removal.

## Implemented contract

| Area | Evidence |
| --- | --- |
| Supply chain | Local `gpui-terminal 0.1.0-kanvibe.1` path dependency, upstream source commit/provenance, and intact MIT/Apache license files |
| Scrollback | Fractional trackpad accumulation, bounded row deltas, display-offset rendering, scroll indicator, and bottom restoration on keyboard/paste input |
| Terminal modes | SGR click/drag/hover/wheel reports; alternate screen falls back to bounded cursor sequences; Shift forces local selection/scrollback |
| Selection | Alacritty-native simple/semantic/line selection with scrollback-aware coordinates and render highlight |
| Clipboard | Cmd+C copies a non-empty terminal selection; Cmd+V writes clipboard text and honors bracketed-paste mode |
| Integration | `kanvibe-app` macOS `native-ui` resolves the local fork; macOS CI runs fork tests before unsigned bundle packaging |
| Runtime evidence | Every Phase 5 run receives a non-overwriting terminal checklist covering S03/S04/S13/S14 and required artifact references |

## Executed gates

| Gate | Result |
| --- | --- |
| Fork `cargo fmt --check` | PASS |
| Fork `cargo clippy --all-targets -- -D warnings` | PASS |
| Fork `cargo check --locked --tests` | PASS |
| Root `cargo fmt --all -- --check` | PASS |
| Root workspace all-feature/all-target Clippy | PASS |
| Root workspace tests | PASS — 155 tests |
| Root workspace release build | PASS |
| Root and fork locked metadata/lockfile resolution | PASS |
| CI YAML parse | PASS |
| Packaging/Phase 5 shell syntax | PASS |
| Linux package probe | PASS — expected Darwin-only exit 78 |
| `git diff --check` | PASS |

## Platform gates not executable here

- Fork `cargo test` compiles all Rust code but the Linux linker lacks
  `libxkbcommon` and `libxkbcommon-x11`. This is a host system dependency
  blocker, not a Rust compile failure. The executable fork suite is therefore
  an explicit `macos-latest` CI step.
- Full `aarch64-apple-darwin` GPUI cross-check reaches `ring` and stops because
  this Linux host has no Apple clang/macOS SDK. The native macOS CI build is the
  authoritative compile/package gate.
- Pointer direction/speed, selection visuals, Cmd+C/V, tmux/nvim mouse behavior,
  resize, focus, and process lifecycle still require two clean real-macOS
  S03/S04/S13/S14 runs with artifacts.

## Release decision

The terminal fork is ready for macOS CI and hands-on Phase 5 validation.
Issue #310 is not closed, default rollout is not approved, and Electron remains
the rollback path until the real-macOS, signing/notarization, release/update,
stabilization, and rollback gates pass.
