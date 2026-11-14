# KanVibe AI Session History Electron QA Report

- Run ID: `perf-chat-ui-1`
- Branch: `perf/kanvibe-mobile-performance`
- Commit: `a0720586`
- Scope: Electron UI QA for responsive chat/history navigation at desktop, 768px and 390px; manual history loading, provider icon rail OR filtering, message-level provider icons, Markdown rendering, Mermaid SVG rendering, sanitizer behavior, body-text chat search across Claude/Codex/OpenCode/Gemini history, session detail rendering, and role filter narrowing
- Status: **FAIL**

## Checks

- ✅ **Actual Codex headless CLI request persists readable history (non-blocking when unavailable)** — skipped (non-blocking): Codex auth.json is missing; agent is not logged in
- ✅ **Seed isolated git project, fake HOME AI history, and KanVibe task** — task=5ad2bf80-7ed3-4972-8fcf-24488f5e955a; repo=/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fixtures/ai-session-repo; worktree=/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fixtures/ai-session-repo__worktrees/manual-ai-history; fakeHome=/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home; fixtures=/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home/.claude/projects/-home-crookedbot-Documents-github-crookedbot-rookedsysc-kanvibe--worktrees-perf-kanvibe-mobile-performance-qa-output-perf-chat-ui-1-fixtures-ai-session-repo/claude-manual-ai-history.jsonl,/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home/.codex/sessions/2026/codex-manual-ai-history.jsonl,/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home/.gemini/tmp/manual-ai-history-project/chats/session-2026-01-01T00-10-gemini.json,/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home/.local/share/opencode/opencode.db
- ❌ **Opening AI chat view shows the AI session panel without selecting a conversation** — locator.click: Timeout 10000ms exceeded.
Call log:
[2m  - waiting for getByRole('button', { name: '히스토리', exact: true })[22m

    at /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:693:69
    at async check (/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:582:22)
    at async main (/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:690:5)
- ❌ **AI session history Electron QA flow** — locator.click: Timeout 10000ms exceeded.
Call log:
[2m  - waiting for getByRole('button', { name: '히스토리', exact: true })[22m

    at /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:693:69
    at async check (/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:582:22)
    at async main (/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa/electron/flows/ai-session-history.cjs:690:5)
- ✅ **Playwright trace artifact written** — /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/diagnostics/playwright-trace.zip
- ✅ **CDP diagnostics artifact written** — /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/diagnostics/cdp-diagnostics.json

## Console / Runtime Errors

- warning: %cElectron Security Warning (Insecure Content-Security-Policy) font-weight: bold; This renderer process has either no Content Security
  Policy set or a policy with "unsafe-eval" enabled. This exposes users of
  this app to unnecessary security risks.

For more information and help, consult
https://electronjs.org/docs/tutorial/security.
This warning will not show up
once the app is packaged.

## Evidence

- task-detail-before-ai-history-load: `/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/screenshots/01-task-detail-before-ai-history-load.png`
- Video: `/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/run.mp4`

## Diagnostics

- Playwright trace: `/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/diagnostics/playwright-trace.zip`
- CDP diagnostics: `/home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/diagnostics/cdp-diagnostics.json`
- CDP counters: {"requests":15,"responses":15,"failedRequests":0,"exceptions":0,"logEntries":0}
- Playwright trace captures actions, screenshots, DOM snapshots, console, and network timeline.

## Notes

- Electron renderer recording: /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/videos/36ff8441650a9e8feb01aef94ad166fd.webm
- Actual X11 screen recording captured with ffmpeg.
- Fake HOME for provider history fixtures: /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fake-home
- Fixture repo/worktree: /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fixtures/ai-session-repo / /home/crookedbot/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/perf-kanvibe-mobile-performance/qa-output/perf-chat-ui-1/fixtures/ai-session-repo__worktrees/manual-ai-history
- Actual Codex headless request skipped without failing QA: Codex auth.json is missing; agent is not logged in
- QA uses isolated KANVIBE_APP_DATA_DIR and fake HOME under the run directory, not the user's app data or real AI history.

