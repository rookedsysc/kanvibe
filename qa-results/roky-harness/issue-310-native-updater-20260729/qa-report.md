# Roky Harness QA — Issue #310 native updater

## Verdict

`PASS` for the portable updater implementation and fail-closed release
contracts. A real Developer ID-signed published update, successful replacement,
forced health-timeout rollback, and unchanged user-data evidence remain
`BLOCKED` on this non-macOS host. Electron removal is still prohibited.

## Acceptance coverage

| Requirement | Result | Evidence |
| --- | --- | --- |
| Trusted release selection | PASS | Exact stable tag/page and one exact `KanVibe-<version>.dmg` URL/name/state/size/SHA-256 are required; ambiguous or malformed assets leave Install disabled |
| Transport integrity | PASS | Streaming download enforces the declared byte count and GitHub SHA-256 before the partial file is committed |
| Publisher identity | PASS (contract) | DMG and candidate require Developer ID authority and the running app's TeamIdentifier |
| Apple distribution policy | PASS (contract) | Fixed-argument `codesign`, `stapler`, and `spctl` checks cover DMG, mounted app, and staged app |
| Product identity | PASS (contract) | Candidate requires `com.kanvibe.desktop` and the selected release version |
| Safe replacement | PASS | Signed helper waits for the old PID, uses same-directory bundle renames, and journals every recoverable transition |
| Failed-update rollback | PASS | Launch failure or missing version/nonce health acknowledgement removes the candidate, restores the backup, and relaunches the old app |
| Release artifact | PASS (contract) | Packaging signs the nested helper inside-out, notarizes/staples app and DMG, emits the final DMG `.sha256`, and the independent verifier checks checksum/team/helper/app/DMG |
| Architecture/provenance | PASS (contract) | Release packaging requires clean committed source, records/verifies the 40-character commit, and creates arm64+x86_64 app/helper binaries in one universal DMG |
| Crash diagnostics | PASS | Fatal errors and panics append bounded structured version/commit/PID diagnostics with a captured backtrace |
| Real release evidence | BLOCKED | `qa/checklists/updater-macos.md` requires signed success and forced rollback with database snapshots on macOS |

## Command evidence

| Command | Result |
| --- | --- |
| Official-source verification | PASS — 7/7 Apple/GitHub URLs |
| Portable GPUI + macOS-updater path type-check | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --workspace --all-features` | PASS — 163 tests |
| `cargo build --workspace --release --all-features` | PASS |
| Local terminal-fork release build | PASS |
| Packaging/release/Phase 5 shell syntax | PASS |
| Linux packaging and release-verifier probes | PASS — expected Darwin-only exit 78 |
| `git diff --check` | PASS |

## Review notes

- Release notes remain inert GPUI text; no release markdown or HTML is executed.
- The helper is a separate signed binary in `Contents/Helpers`, so the running
  app never overwrites its own bundle.
- The health token is generated from `/dev/urandom`, confined to the journal
  and launch arguments, and accepted only from the expected installed bundle
  and expected version.
- Journal persistence failure is itself rollback-triggering; a candidate is
  never considered committed until the matching health acknowledgement.
- Scoped QA variables can suppress the acknowledgement and shorten the bounded
  wait to produce deterministic rollback evidence. They do not bypass any
  digest, signature, notarization, Gatekeeper, team, bundle, or version check.

## Remaining real-macOS gates

- Build and verify the release with real Developer ID/notary credentials.
- Publish the immutable DMG/checksum asset and confirm GitHub's digest metadata.
- Complete both halves of `updater-runtime-checklist.md` with before/after
  database evidence and no orphan bundle/helper/health paths.
- Complete two consecutive S01–S14 Phase 5 PASS runs against the same source
  and scenario digest.
- Preserve Electron through native stabilization and the verified rollback
  window required by Issue #310.
