# Roky Harness QA — Issue #310 native command transition

## Verdict

`PASS` for the portable Node-free root command surface, explicit Electron
baseline namespace, and CI contract. The final switch of generic desktop
defaults and Electron deletion remain `BLOCKED` by real-macOS Phase 5,
signing/notarization, updater, stabilization, and rollback gates.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Root native development | PASS | `./kanvibe-native dev` executes `kanvibe-app` through Cargo |
| Root native build | PASS | `./kanvibe-native build` creates the all-feature release app binary |
| Root native test/check | PASS | Workspace tests plus formatting/all-feature Clippy run without Node; terminal execution tests remain macOS-only |
| Root native packaging | PASS (contract) | `package`, `phase5`, and `verify-phase5` dispatch to the fail-closed native scripts |
| No product Node bootstrap | PASS | The native launcher contains no Node, pnpm, Electron, or webview invocation |
| Legacy isolation | PASS (readiness) | `legacy:electron:*` aliases and the `Legacy Electron baseline` CI job make retained baseline usage explicit |
| macOS bundle provenance | PASS (contract) | CI injects `${{ github.sha }}` and verifies `KanVibeBuildCommit` plus executable `KanVibeUpdater` |
| Final default switch/removal | BLOCKED | Generic package-manager desktop defaults deliberately remain Electron until the external gates pass |

## Command evidence

| Command | Result |
| --- | --- |
| `bash -n kanvibe-native` | PASS |
| `./kanvibe-native help` | PASS |
| JSON and workflow YAML parsing | PASS |
| Focused root-command source-contract test | PASS |
| `./kanvibe-native check` | PASS |
| `./kanvibe-native test` | PASS — 165 workspace tests |
| `./kanvibe-native build` | PASS |
| `git diff --check` | PASS |

## Remaining external gates

- Complete two consecutive real-event S01–S14 macOS PASS runs against the same
  source/scenario/app version.
- Build, sign, notarize, staple, install, update, and force updater rollback
  using real credentials and immutable release assets.
- Record one native stabilization release and verified Electron/data rollback.
- Only then repoint generic desktop defaults and remove the Electron product
  runtime according to `qa/ELECTRON_REMOVAL_LEDGER.md`.
