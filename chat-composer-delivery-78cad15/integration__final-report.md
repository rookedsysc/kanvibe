# Independent composer integration QA

**PASS for desktop/CLI/history behavior.** No product code edits or commits by QA. Mobile, repository CI and full diff quality verdicts belong to their assigned roles.

Source: `45c404119d73e59b23bbdc981696a89676661cc1`; **720 tracked Node inputs**, SHA-256 `45394beab54a47b8c94542c4efb9a0236de576f974c7ec940d1f5e75dc930d9d`. Root and actual runtime both match with zero differences after the documented owned port/helper-location overrides. Full-tree fingerprints differ because mobile-only work continued; consumed Node inputs are unchanged. See `source-integrity-verified.json` and `source-binding.json`.

| Gate | Result | Executed count | Evidence |
|---|---|---:|---|
| pane baseline | PASS / exit0 | 7 runner checks | `desktop-verified/pane-chat-result.json` |
| history baseline | PASS / exit0 | 5 runner checks | `desktop-verified/desktop-ui-result.json` |
| composer intake | PASS / exit0 | 11 runner checks | `desktop-intake-verified/result.json` |
| route/unmount quota | PASS / exit0 | 2 runner checks | `quota-navigation-verified/result.json` |
| authorized media/history | PASS / exit0 | 4 runner checks | `history-verified/result.json` |
| real native CLI | PASS / exit0 | 15 native cases | `native-verified/result.json` |
| real ended image history | PASS / exit0 | 2 runner checks | `native-ended-history-verified/result.json` |

Actual Codex0.167.2 and installed Claude2.1.323 each submitted skill, image-only, captioned image, file-only, filenames with spaces, file suggestions and plain text through the app. **15/15 native cases passed**, including the literal image-source text regression. Every Send produced exactly one primary localhost model request; Codex title requests are counted separately. Both providers sent the exact owned PNG bytes. Four real image sessions remained discoverable after CLI exit, each with two messages and one image attachment. The Claude internal provenance bubble is absent; literal human text remains.

C1 navigation staged ten unsent images, left the actual task route, verified all ten files were discarded, reopened the same live CLI and staged another image successfully. Real filesystem upload failure retained the draft/thumbnail, recovered on Retry, and restored the owned staging directory to0700.

Curated visual evidence: `desktop-intake-verified/chooser-preview.png`, `desktop-intake-verified/searchable-skills.png`, `desktop-intake-verified/transient-upload-retry-success.png`; `quota-navigation-verified/upload-after-reopen.png`; `native-verified/claude-file-only/after-send.png`; `native-ended-history-verified/real-ended-claude-caption.png`; `history-verified/ended-image-history.png`. Screenshots were opened and inspected. The local Orca/Paseo checklist was compared as source references; their apps were not run.

Chooser scope: the real attachment button emitted a filechooser event and accepted an owned on-disk file. Native dialog pixels remain unverified because the local Orca executable is unavailable. Paste/drop exercised the actual renderer handlers and staging with untrusted DOM events; the OS clipboard was neither read nor changed.

All provider replies came from owned localhost fixtures. This proves image/skill delivery and native submission, not model understanding or tool execution. No paid model calls, user app replacement, user database access or OS clipboard mutation occurred.

Initial failures and red/green investigations remain in `findings.md`, `native-text/`, `native-final/`, `claude-file-boundary/` and `native-ended-history/`. Special-mode skill discovery failures and fast automation setup races are distinguished from product findings.

Final source rebind: commit `78cad15a5dbaf99460f6f674517712cc22edb89e` has zero differences across the same720 consumed Node inputs.

Recording: `desktop-verified/video/960a5f18d31db31f3f0bc13c0e01cf1f.webm` is finalized, duration797.760s; no remux was needed (`verified-recording-probe.json`). After the completed gates, the owned app was found with zero windows and closed listeners; its initiator is not established. Exact owned-process recovery preserved AppData, certificate, pairing, task and pane IDs (`desktop-verified/recovery-result.json`). The continuation recordings under `desktop-verified/video-resumed/` are finalized too. All six retained recordings decode fully with exit0.

Cleanup is complete (`cleanup-summary.json`, full `cleanup.json`): helper/app/provider stopped by exact ownership, ports20364/20374/54777/54778/55395 released, three owned source copies and eight isolated model homes removed.151 session JSONLs were archived under `session-archive/`; owned AppData and fixture audits are preserved. User app PID47303 still listens on9736. No device/emulator actions were performed by this role.
