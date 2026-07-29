# Roky Harness QA — Issue #310 session dependency

## Verdict

`PASS` for the portable Rust service, bounded local/SSH transport, production
Task Detail source contract, and S14 replay contract. Real packaged macOS local
and SSH behavior remains `BLOCKED` until the Phase 5 evidence run.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Single tmux/zellij policy | PASS | `kanvibe-session` owns check/install commands; `kanvibe-pty` compatibility-re-exports them |
| Bounded local execution | PASS | Typed `SessionType` API uses a 10-second check, 300-second install, and 4 MiB output ceiling |
| Bounded SSH execution | PASS | Typed remote API reuses validated host, multiplexed control path, retry, timeout, output, and transport classification |
| Non-interactive install | PASS | Root or `sudo -n`; Homebrew/cargo and supported Linux package managers; post-install availability check |
| Cache and remote blocking | PASS | Successful checks cache for 60 seconds; non-transport remote install failures block retry until restart |
| Production Task Detail UI | PASS (source contract) | Check, Install, retry-check, loading, missing, available, error, and restart-blocked states are connected off the UI thread |
| Terminal precondition | PASS | Both DB session binding and PTY spawn reject a missing selected dependency |
| S14 replay contract | PASS | Check → install → retry actions and eventual `available` assertion execute over the QA socket; total replay command count is 151 |
| Packaged runtime | BLOCKED | Real local and `qa-remote` SSH checks/install/retry, terminal attachment, screenshot, and video require macOS |

## Command evidence

| Command | Result |
| --- | --- |
| Focused RED compile test | FAIL as expected — typed remote dependency method absent |
| Focused remote boundary test after implementation | PASS |
| `cargo test --workspace --all-features` | PASS — 167 workspace tests |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `./kanvibe-native check` | PASS |
| `./kanvibe-native test` | PASS |
| `./kanvibe-native build` | PASS |
| `git diff --check` and S14 JSON parse | PASS |
| Apple-target cross-check | BLOCKED before app type-check — Linux C compiler rejects Apple `-arch` and deployment flags in `ring`/SQLite |
| `./kanvibe-native phase5` | Expected exit 78 — Darwin required |
| `./kanvibe-native verify-phase5 --run qa/parity/run-023` | Expected exit 1 — macOS reports, checklists, manifest, and S01–S14 evidence are absent |

## Roky review

- No arbitrary public shell-command surface was added; callers can request only
  a typed tmux or zellij check/install operation.
- The UI never runs installation on the GPUI thread.
- A missing dependency cannot partially persist terminal session identity.
- Electron remains available as the explicit legacy fallback because Issue #310
  removal gates are not yet satisfied.
- The context-loader skill's referenced repository-local principle files were
  unavailable, so existing Rust architecture, Electron behavior, and Roky
  acceptance rules were used as the fallback.
