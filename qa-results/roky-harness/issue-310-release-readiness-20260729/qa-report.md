# Roky Harness QA — Issue #310 macOS release readiness

## Verdict

`PASS` for portable release-script readiness. Actual Developer ID signing,
Apple notarization, Gatekeeper, clean-machine installation, published update,
Homebrew transition, stabilization, and rollback remain `BLOCKED` until their
macOS/external release gates run. This report does not authorize Electron
removal.

## Implemented gates

| Gate | Behavior |
| --- | --- |
| Explicit release mode | `package-macos-app.sh --release` rejects unsigned/no-DMG execution |
| Credential boundary | Signing identity and notarytool keychain profile are explicit flags or narrowly named environment variables |
| App signing | Developer ID, hardened runtime, secure timestamp, deep/strict verification |
| App notarization | ZIP submission with `--wait`, accepted-status validation, stapling, and Gatekeeper execute assessment |
| DMG signing/notarization | Developer ID signature, accepted-status validation, stapling, and Gatekeeper open assessment |
| Independent verification | `verify-macos-release.sh` rejects ad-hoc signatures, verifies Team ID/hardened runtime/tickets/Gatekeeper, mounts the DMG read-only, and re-verifies its embedded app |
| Portable regression | QA harness source-contract test requires every signing/notary/staple/Gatekeeper/mounted-app boundary |

## Command evidence

| Command | Result |
| --- | --- |
| `bash -n native/scripts/package-macos-app.sh` | PASS |
| `bash -n native/scripts/verify-macos-release.sh` | PASS |
| Release verifier `--help` | PASS |
| Linux release verifier probe | PASS — expected Darwin-only exit 78 |
| `macos_release_scripts_fail_closed_around_signing_and_notarization` | PASS |
| `git diff --check` | PASS |

## Remaining external/runtime gates

- Import the real Developer ID certificate and create the notarytool keychain profile.
- Execute release packaging and independent verification on macOS.
- Publish immutable DMG/checksum assets, install cleanly, update from the
  previous public version without data loss, and exercise rollback.
- Validate the Homebrew cask transition and stabilization window before
  changing product defaults or deleting Electron.
