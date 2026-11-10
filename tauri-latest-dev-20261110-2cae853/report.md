# Latest dev Tauri port QA

Base: `82fe04d2ba82de026fc8cefc0cdd801501d26b96`; version1.5.3. SourceSHA256 `32f83c0c3d8965f239d490c387b6eb456801e5a060ade920aed2731ce90266e7`.

Rust262 / JavaScript831 / native45 passed. 112 files / 517 hunks /0unmapped.

## Commands

| Gate | Command | Exit | Stable |
| --- | --- | --- | --- |
| final-build | pnpm build | 0 | True |
| final-check | pnpm check | 0 | True |
| final-clippy | pnpm check:rust | 0 | True |
| final-debug-build | pnpm qa:tauri:prepare | 0 | True |
| final-docs-build | pnpm build | 0 | True |
| final-docs-install | pnpm install --frozen-lockfile | 0 | True |
| final-docs-routes | node scripts/verify-metadata-routes.mjs | 0 | True |
| final-docs-submit | node scripts/submit-sitemap.mjs | 0 | True |
| final-install | pnpm install --frozen-lockfile | 0 | True |
| final-js | pnpm test | 0 | True |
| final-lint | pnpm lint | 0 | True |
| final-native-basic | node qa/tauri/flows/parity.cjs qa-results/roky-harness/2026-11-10-latest-dev/native-basic3 | 0 | True |
| final-native-close | node qa/tauri/flows/terminal-tab-close-confirm.cjs qa-results/roky-harness/2026-11-10-latest-dev/native-close2 | 0 | True |
| final-native-latest | node qa/tauri/flows/latest-dev-parity.cjs qa-results/roky-harness/2026-11-10-latest-dev/native-latest | 0 | True |
| final-native-mobile | node qa/tauri/flows/mobile-stream-parity.cjs qa-results/roky-harness/2026-11-10-latest-dev/native-mobile | 0 | True |
| final-native-workspace | node qa/tauri/flows/task-workspace-view.cjs qa-results/roky-harness/2026-11-10-latest-dev/native-workspace | 0 | True |
| final-rust | pnpm test:rust | 0 | True |

## Native checks

- basic: 13
- close: 10
- mobile: 8
- workspace: 5
- latest: 9

## Limits

- Hidden WebView: document.hidden=true, hasFocus=false, requestAnimationFrame did not fire; workspace canvas foreground input unverified.
- Dialog decisions injected at IPC; OS dialog buttons not operated (orca unavailable).
- Real remote SSH, Linux/zellij runtime, OS sleep/wake and Flutter/Maestro device unverified.
- Sitemap submission skipped without credentials.
- ESLint exits0 with12 inherited warnings.

## Evidence

All results use isolated fixture databases. Source hash before/after every gate matches. Native debug binary SHA256 matches the rebuilt binary. Independent A/C and B source reviews have no unresolved findings; see independent-review.md and independent-cli-review.md. Added/renamed37 Rust tests all appear in the successful full test output;1 old name replaced, net36 new tests. Repair history retained in repair-log.md.

Plans, QA reports and application fixture data stay local. Reusable QA scripts and production changes are committed. Evidence upload must whitelist screenshots/report only, never app-data, tokens, certificate private keys or databases.
