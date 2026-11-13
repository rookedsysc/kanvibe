# Chat composer delivery QA

Commit: `78cad15a5dbaf99460f6f674517712cc22edb89e` · branch `feat/chat-ui` · PR443 · merged base `fa5b414bf8c54b756bd5006610b69563c3c2c98c`.

Status: PASS for implemented desktop/Android behavior; iOS runtime remains explicitly unrunnable..

## Delivered scope

C1: task/pane/process lease 소유 첨부 stage, 미리보기·삭제·재시도, 초안 보존, 요청 중복 방지, 화면 종료/늦은 응답/전송 중 정리. C2: 실제 CLI 이미지 입력과 일반 파일 참조, 로컬/SSH private staging. C3: 현재 호스트의 스킬·명령·파일 검색, `$`/`/`/`@` 자동완성과 키보드/터치 선택. C4: Codex 스킬 및 Claude 파일 참조 자동완성 토큰 경계를 닫아 한 번의 Send로 전송. C5: 이미지 전용/캡션 히스토리, 인증된 lazy media, 삭제된 파일 상태, Claude 내부 이미지 provenance 제외. C6: 모바일 사진·파일·클립보드·OS 드롭 intake와 같은 composer/history 계약.

## Mechanical gates

Node24.15.0/pnpm10.17.0. `pnpm check` exit0; `pnpm test` exit0, 219files/3,123tests; `pnpm build` exit0; feature39files ESLint exit0. Flutter3.47.2: `flutter analyze --no-pub` exit0; `flutter test --reporter json` exit0, 534non-hidden/non-skipped tests; `flutter build apk --profile --no-pub` exit0; owned18Dartfiles format exit0. All commands recorded HEAD45c4041 + fingerprintfa502646; pushed78cad15 has exactly that diff, verified in `commit-gate-binding.json`. Before each gate source manifest matched the candidate; package/lock manifests exactly matched prepared ABI-specific dependencies.

GitHub Type Check & Test and external Workers Builds both SUCCESS on78cad15. External Workers Builds cannot be reproduced locally; its actual remote result is recorded in `github-ci.json`.

## Independent runtime and source review

Desktop: pane7 +history5 +composer11 +quota2 +auth/media4 +native15 +ended-image-history2 checks PASS. Installed Codex0.167.2 and Claude2.1.323, real PTY/native CLI, localhost response server. Every Send yields exactly one primary request; separate Codex title requests are excluded. Four actual native image sessions preserve exact bytes and remain readable after CLI exit. Both Claude image histories contain two messages/one image, no provenance bubble. Literal human image-source text remains. No paid model calls.

Independent C1 desktop real-store two regressions RED→GREEN; mobile five lifecycle checks PASS. Independent C5 scope/FIFO findings repaired RED→GREEN; native companion13 checks PASS. One earlier whole-suite shortcut assertion failed once; targeted85test file, independent50 repetitions and final uninstrumented full suite passed. Cause was not established; no speculative shortcut source edit was made. All logs remain local.

Branch diff:126files/392hunks mapped, current composer delivery68files; previous approved pane/chat/design hunks link to their earlier plans/reviews. Zero unmapped, gate-enabling or tooling-enforced-format hunks. Provider/store ownership and component boundaries are recorded in frontend/mobile audits. Runtime artifacts and plans remain local; untracked Orca/Paseo clones and taste artifacts are preserved.

## Android device evidence

Owned Android emulator5590: actual OS photo/file picker with failure/preview/retry/remove/caption ACK (`final-picker-file`); ten actual clipboard uploads → leave route → server count0 → reopen same lease → new upload1 → remove0 (`final-lifetime-overlay-settled`); actual clipboard image-only Send/ACK; four C3 logical cases (skill button, typed `$`, command native Enter, file with spaces); ended image history with both decoded images and missing-image retry (`final-history-row`,15s); genuine OS GLOBAL_URI_READ Service-overlay drop → READY → exact server bytes/count1 → caption ACKA (`final-overlay-drop`,36s). All required behavior passed, with recordings.

`final-completions` and `final-completion-key` are retained failed setup/IME-assumption runs, not labeled passing whole flows: the first had already passed the skill-button/typed-dollar assertions before a soft-IME newline was mistaken for a hardware selection action; the hidden-keyboard attempt popped the route. The correct focused native-key continuation and @ selection passed (`final-completion-native-key`). Initial history tap selected the identically named search input; the actual session button flow passed. No speculative keyboard/native listener product patch was retained.

All tracked mobile source bytes in the earlier photo/file/lifetime recordings exactly match the final commit (`mobile/final-evidence-source-reuse.json`). The final full-tree gate fingerprint matches the pushed commit via `commit-gate-binding.json`; current native/drop runs bind directly to78cad15.

QA-only failures are retained. The helper Activity input sink covered the full task and prevented Window Manager from routing DROP into KanVibe; a speculative FlutterView listener relocation was removed. The production Kotlin bridge matches its original implementation. The isolated desktop fixture also began quitting with zero windows and closed listeners; exact owned process recovery preserved AppData, certificate/pairing and task IDs. Initiator was not established. These incidents do not establish product defects. Final Android flows use a healthy isolated fixture and genuine OS payloads.

## Limits

Android target is the owned emulator5590. iOS full app runtime/build is UNRUNNABLE here: Xcode26.0.1 vs FirebaseSDK13 requirement26.2+; Swift bridge typecheck and project syntax checks passed. Windows/Linux desktop UI and a real remote SSH host were not executed. Desktop chooser button/filechooser and file bytes were exercised; native dialog pixels were not verified. Desktop paste/drop used actual renderer handlers with untrusted DOM events, without reading or changing the OS clipboard. Native CLI local responses prove delivery, not vision understanding or model tool execution. HEIC/HEIF originals are rejected with an explicit format error; iOS compatible picker conversion was checked in plugin source, not device runtime. Cleanup of abandoned staging is best effort; sent files remain available to the CLI/history.

Evidence is local under this directory; selected reports/screenshots/recordings are appended to the existing QA asset branch for the PR. Authenticated GitHub blob/path verification is required after upload. GitHub inline private-media rendering is not verified from a signed-in browser.

Final isolated desktop cleanup: PASS. Owned helper/provider/runtime processes stopped, owned ports released, source copies/model homes removed after session archival; user app9736/PID47303 remained live and unchanged. Mobile cleanup is recorded separately.
