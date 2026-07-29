# Roky Harness QA — Issue #310 provider hook recovery

## Verdict

`PASS` for provider status policy, local installation, bounded SSH callback
health, production Task Detail source contract, and S11 replay. Real packaged
local/SSH provider execution remains part of the macOS Phase 5 gate.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Four-provider status | PASS | Claude, Codex, Gemini, and OpenCode each require their complete generated file set |
| Callback correctness | PASS | Generated single- or double-quoted shell URLs and OpenCode plugin URLs are compared with local/SSH expected endpoints |
| Callback health | PASS | Local status probes the native listener; SSH status uses a parsed IP/port typed boundary with 5-second/64 KiB limits |
| Preservation-safe repair | PASS | Existing render/install path preserves supported settings and target fan-out before rewriting generated files |
| Production UI | PASS (source contract) | Hooks dock item and provider rows expose not-installed, URL-mismatch, unreachable, and ready states plus recheck/install-repair controls |
| UI responsiveness | PASS (source contract) | All local/SSH file reads, health checks, and writes run outside the GPUI thread |
| S11 replay | PASS | Check → install → recheck and eventual four-provider-ready assertion execute over the QA socket |
| Packaged runtime | BLOCKED | Real Claude/Codex/Gemini/OpenCode callbacks, local and SSH screenshots/video, and persisted status transitions require macOS |

## Command evidence

| Command | Result |
| --- | --- |
| Focused RED compile test | FAIL as expected — provider inspection APIs absent |
| Focused provider inspection test | PASS |
| Focused local app service test | PASS |
| Focused bounded SSH health test | PASS |
| `cargo test --workspace --all-features` | PASS — 170 workspace tests |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo build -p kanvibe-app --release` | PASS |
| S11/S14 JSON parse and `git diff --check` | PASS |

## Roky review

- Status inspection is read-only and requires every provider file; partial
  installations cannot be reported as ready.
- Remote health does not expose an arbitrary shell surface: the callback host
  is parsed as `IpAddr` and the port is a `u16`.
- The existing preservation-safe renderer remains the only write path.
- The project ledger advances from `SERVICE` to `RUNTIME`; it does not claim
  `CONNECTED` before real packaged provider fixtures pass.
- The context-loader skill's repository-local principle files were unavailable,
  so existing Rust architecture, Electron behavior, and Roky acceptance rules
  were used as the fallback.
