# Independent composer correctness review

**PASS — assigned source correctness review; C1-LIFETIME closed for desktop and mobile.** Specification conformance and implementation quality: PASS for this review slice. Review execution: isolated reviewer. No remaining verified finding in this review. No product source, apps, devices, ports, or external services changed.

The original verified bug is preserved in `report-initial-request-changes.md`; the earlier desktop-only closure is in `report-desktop-pass-mobile-pending.md`. Root separately owns the native Claude process-detection repair, app/device integration, exhaustive hunk mapping and final full CI. This report does not replace those delivery gates.

## C1-LIFETIME repair verification

| Scenario | Repaired desktop: real hook + real store | Repaired mobile: notifier + HTTP boundary fixture |
| --- | --- | --- |
| Ten ready uploads → dispose → finish cleanup → reopen same lease → attach one | Ten old IDs removed, fresh upload READY | All ten DELETEs, reopened draft stages draft11 |
| Upload ACK arrives after disposal | Existing cleanup path preserved | Captured old connection/client cleans ID; new draft remains empty |
| Dispose while send is pending | Zero delete calls before outcome | Zero DELETEs before outcome for accepted false/true |
| Backend retains file, then ACK arrives | Map entry removed after ACK; actual file remains readable with contents `hello` | After success, deferred DELETE preserves fixture's retained file |
| Deactivate hidden pane | Existing target draft map retained | No cleanup; text and ready ID retained |

Desktop exact isolated command: `/Users/rookedsysc/.nvm/versions/node/v24.22.1/bin/node node_modules/vitest/vitest.mjs run src/desktop/renderer/components/__tests__/finalLifecycle.test.tsx`. `lifecycle-repaired-red.log`: two failures, exit1 against old hook. `lifecycle-repaired-green.log`: **2/2 PASS, exit0** against repaired hook. `lifecycle-repaired.test.tsx` records the independent regression. The renderer adapter delegates to the real attachment store/discard implementation; retained file fixture was removed only after read verification.

Mobile exact isolated command: `/Users/rookedsysc/fvm/versions/3.47.2/bin/flutter test --no-pub test/src/features/pane_chat/pane_chat_composer_test.dart --name 'dispose|late upload ACK|hidden pane deactivation' --reporter json`. `mobile-lifetime-independent-green.log`: **5/5 PASS, exit0**, run in the reviewer's own source-only candidate. Independently inspected the five assertions and per-build `_ComposerLifetime`: captured client/connection, staged/pending ID ownership, late ACK cleanup, and old lifetime disposed flag protect Riverpod notifier reuse. Implementation evidence `../mobile/lifetime-red.log` records three lifecycle regressions failing before repair; `../mobile/lifetime-rebuild-green.log` records21/21 composer tests passing. Mobile fixture retention is not represented as an independent real-filesystem result; that boundary is independently proven by the desktop real-store test above.

`candidate.json` records reviewed source hashes; `repair-current-source-match.json` compares them with current workspace files. The original candidate retains baseline provider implementations; desktop retest changed only the hook/test, and mobile retest refreshed the frozen composer repair files/test plus package configuration. No shared app or device runtime was used.

Cleanup is asynchronous and best effort. The first repaired desktop probe waited only for delete invocation, then raced pending filesystem cleanup; `lifecycle-repaired-initial-probe.log` preserves it. The final test waits until actual server map cleanup completes before asserting reclaimed quota. App termination, failed cleanup RPCs, and permanent orphan-file garbage collection are outside this closure; there is no claim of a server attachment TTL.

## Preserved earlier review evidence

No additional source blocker was found in captured-target acknowledgements, connection/lease ownership, native callback epochs, provider framing, request receipt/retry handling, repaired C5 missing-cwd authorization/nonblocking regular-file checks, bounded authenticated lazy media, or stale suggestions. State/component ownership matches existing frontend/mobile inventories. No additional architecture, design or comment finding is asserted.

Feature79542ac and dev89e0c89 are both ancestors of reviewed merge00a79b6 (checks exit0). `merge-resolution.diff` preserves composer/upstream connection tests, both Herdr tab-bar/pane-size APIs, and lifecycle mocks. No lost-side merge finding. Earlier focused isolated run: attachment service6 + catalog6 + pane service24 + composer14 + attachment renderer5 + image history10 + mobile HTTP166 = **231 PASS, seven files, exit0** (`focused-tests.log`). Existing green C5 and merge audits are unchanged.
