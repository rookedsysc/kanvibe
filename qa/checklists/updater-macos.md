# macOS native updater release evidence

This checklist is a separate Issue #310 release gate. Portable tests and an
unsigned app cannot satisfy it. Run once for a successful signed update and
once with the scoped health-timeout fixture to prove restoration of the prior
signed app and unchanged user data.

- Run ID:
- Source commit:
- From app version:
- To app version:
- Release URL:
- GitHub asset ID:
- GitHub asset digest:
- macOS version:
- Mac model / architecture:
- Operator:
- Date:

## Published release asset

- [ ] `KanVibe-<to-version>.dmg` and its `.sha256` are published on the exact stable GitHub release tag.
- [ ] GitHub reports the DMG asset as `uploaded`, with the expected nonzero size and `sha256:` digest.
- [ ] `native/scripts/verify-macos-release.sh` exits 0 for the downloaded DMG, checksum, and independently packaged app.
- [ ] The verifier records one matching non-ad-hoc Developer ID TeamIdentifier for the DMG, mounted app, and `KanVibeUpdater`.
- [ ] `stapler validate` and Gatekeeper accept the DMG and mounted app on the QA Mac.

Evidence:

- Release/API metadata:
- Release verifier log:
- Gatekeeper log:

## Successful update

- [ ] Install the signed from-version app into `/Applications` and launch it normally.
- [ ] Record the userData database path, byte checksum, schema, project/task counts, settings, and notification counts before update.
- [ ] The release dialog enables Install only for the exact DMG asset; release notes render as inert text.
- [ ] Install downloads without blocking the GPUI window and rejects no valid digest/signature/identity gate.
- [ ] The old app exits, the helper launches the to-version app, and that app opens the existing board.
- [ ] The terminal update journal reaches `committed`; its expected version matches the running bundle.
- [ ] No `.KanVibe.rollback-*.app`, `.KanVibe.update-*.app`, or `.healthy` marker remains beside the installed app.
- [ ] Database checksum or expected migration snapshot, schema, row counts, settings, and notifications prove no loss.
- [ ] `codesign`, `stapler`, and Gatekeeper still accept the installed to-version app and nested helper.

Evidence:

- Screen/video:
- Update journal:
- Before/after database report:
- Installed-app verifier log:

## Forced failed-update rollback

Launch the from-version app from a controlled QA shell with
`KANVIBE_QA_FORCE_UPDATE_HEALTH_TIMEOUT=1` and
`KANVIBE_QA_UPDATE_HEALTH_TIMEOUT_MS=1000`. These variables only suppress the
new app's health acknowledgement and shorten the helper's bounded wait; they do
not bypass download, signature, notarization, identity, or staging checks.

- [ ] Start from the same signed from-version app and a fresh copy of the successful-update database fixture.
- [ ] Record the same pre-update bundle/database metadata.
- [ ] Trigger Install and observe the signed to-version candidate launch.
- [ ] The candidate sends no health acknowledgement; the helper removes it after the bounded timeout.
- [ ] The helper restores and relaunches the exact from-version Developer ID-signed app.
- [ ] The terminal update journal reaches `rolledBack`.
- [ ] No backup/staged/health path remains beside the restored app.
- [ ] Bundle ID, version, TeamIdentifier, notarization, and Gatekeeper identify the restored from-version app.
- [ ] Database checksum/schema/rows/settings/notifications are unchanged from the pre-update snapshot.
- [ ] A subsequent normal launch without the QA variables succeeds.

Evidence:

- Screen/video:
- Rolled-back journal:
- Before/after database report:
- Restored-app verifier log:

## Verdict

- [ ] Both successful update and forced rollback evidence paths exist.
- [ ] No signing/notarization/Gatekeeper warning is uninvestigated.
- [ ] No data-loss, orphan app bundle, helper, or health marker remains.
- [ ] This checklist is referenced by the matching Phase 5 evidence manifest.

Verdict: `PENDING`

Notes:
