# Final independent mobile C1-LIFETIME closure

**PASS — C1-LIFETIME closed on mobile; no remaining finding.** Specification conformance and implementation quality: PASS for the assigned lifecycle repair. Review execution: isolated reviewer. Current source is byte-for-byte identical to the previously independently tested repair (`source-binding.json`). No new Flutter command was run during the active shared build; no product source/device/app/port was modified.

| Required behavior | Verified current ownership path | Evidence |
| --- | --- | --- |
| Ten ready uploads release quota on disposal and same lease can reopen | Each build captures client, connection, task/token and its own staged IDs; onDispose cleans staged IDs minus pending-send IDs | all ten DELETEs; staged set empty; reopened draft stages draft11 |
| Late upload ACK cannot affect a recreated notifier | upload captures its build's lifetime before awaiting; old disposed flag remains true even when Riverpod reuses the notifier; late ID is cleaned through captured owner | late ACK DELETE issued; recreated draft remains empty |
| Pending send keeps its file until result | captured IDs enter pending set before request; disposal excludes those IDs; completion removes pending marks then cleans through the disposed lifetime | no DELETE before either outcome; one DELETE after each accepted/refused outcome |
| Accepted CLI files remain readable | server retain-before-write policy makes discard remove the staged map entry without deleting retained file; mobile cleanup uses that same DELETE contract | mobile accepted/refused fixture semantics; prior independent desktop real-store test reads retained file after ACK |
| Hidden pane draft survives deactivation | setActive(false) cancels suggestions/intake; staged IDs and text are retained until actual lifetime disposal | hidden draft text and staged ID retained; no DELETE |

Recorded narrow implementation evidence: `mobile/lifetime-red.log`18 passed/3 failed before repair; `mobile/lifetime-rebuild-green.log`21 passed/0 failed after repair. Previously executed independent isolated Flutter3.47.2 command: `flutter test --no-pub test/src/features/pane_chat/pane_chat_composer_test.dart --name 'dispose|late upload ACK|hidden pane deactivation' --reporter json` — **5/5 PASS, exit0**, logged at `final-review/mobile-lifetime-independent-green.log`. Both current code/test hashes match that exact independent run. Machine-readable count reconciliation is in `evidence-counts.json`.

Cleanup is asynchronous and best effort when RPCs fail. This closure covers ordinary disposal, delayed acknowledgements and pane inactivity; it does not claim permanent orphan-file garbage collection or cleanup after process termination. No server attachment TTL is assumed. Native Android global-drop diagnosis is separate and temporary diagnostic lines were excluded, as requested. Root owns final full gates and device handoff.
