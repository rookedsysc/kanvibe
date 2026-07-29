# macOS terminal runtime evidence

This checklist supplements S03/S04/S13/S14. A run is not PASS until every
item has an attached screenshot, video timestamp, or log reference.

- Run ID:
- Git commit:
- `.app` version:
- macOS version:
- Mac model / architecture:
- Display scale:
- Operator:
- Date:

## S03 — local terminal and dock

- [ ] Launch the packaged app, open `qa-task-progress-terminal`, and select Terminal.
- [ ] Print at least 200 numbered rows; wheel/trackpad up shows older rows and down shows newer rows in the expected direction.
- [ ] Slow trackpad motion advances smoothly without losing fractional movement; fast motion remains bounded and usable.
- [ ] The scroll indicator appears above bottom, tracks position, and disappears at bottom.
- [ ] Typing one character while scrolled up returns to bottom and sends that character exactly once.
- [ ] Drag selects characters; double-click expands to a word; triple-click expands to a line.
- [ ] Selection remains aligned after scrolling into history and after resizing the window.
- [ ] Cmd+C copies selected terminal text; Cmd+C with no selection sends no printable input; Ctrl+C still sends interrupt.
- [ ] Cmd+V pastes plain text exactly once.
- [ ] In an application with bracketed paste enabled, Cmd+V is bracketed and embedded escape bytes cannot terminate the wrapper.
- [ ] Terminal resize updates PTY columns/rows; stop/restart exits and reconnects without a leaked child.

Evidence:

- Screenshot:
- Video timestamps:
- PTY/process log:
- Clipboard fixture/result:

## S04 — dock switching

- [ ] Switching Terminal → Pull Request → AI Sessions → Terminal preserves the live terminal and expected scroll position.
- [ ] Dock shortcuts are handled before terminal input and do not write their printable key into the PTY.
- [ ] Returning from each dock restores terminal focus only after overlays are closed.

Evidence:

- Screenshot:
- Video timestamps:
- PTY log:

## S13 — focus and overlays

- [ ] Reopening the same task focuses the existing window without creating a duplicate.
- [ ] Open and close each terminal-adjacent overlay with pointer and Escape; terminal input resumes without an extra click.
- [ ] Escape intended for tmux/nvim reaches the terminal when no KanVibe modal is open.
- [ ] Delete confirmation participates in the modal stack and restores focus only after the stack is empty.

Evidence:

- Window-count log:
- Video timestamps:

## S14 — tmux/nvim and remote mode reporting

- [ ] In local tmux, wheel direction/speed matches Terminal.app closely and does not multiply a single row into an excessive jump.
- [ ] In nvim mouse mode, click, drag, hover where supported, and wheel events reach nvim; Shift+drag forces local text selection.
- [ ] In alternate screen without mouse reporting, wheel navigation uses bounded up/down cursor input.
- [ ] Repeat mouse, selection, clipboard, resize, stop/restart, and focus checks over the configured SSH/zellij fixture.
- [ ] Remote child environment contains no generic `PORT`, `HOST`, or `NODE_ENV` leak.
- [ ] Disconnect and retry surface an error and recover without a leaked local SSH/PTY child.

Evidence:

- Local tmux/nvim video timestamps:
- Remote zellij/SSH video timestamps:
- Remote environment/process logs:

## Run verdict

- [ ] All evidence paths exist inside this run directory.
- [ ] `evidence-manifest.json` identifies this run, source commit, scenario digest, app/macOS/hardware/operator metadata, and non-empty S01-S14 screen/video files.
- [ ] `verify-phase5-run.sh --run <this-run>` exits 0; for rollout approval, `--previous-run <prior-run>` also exits 0.
- [ ] No FAIL or uninvestigated flake remains.
- [ ] This is one of two consecutive runs against the same source and scenario definitions.

Verdict: `PENDING`

Notes:
