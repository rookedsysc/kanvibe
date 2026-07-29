# Upstream provenance

This crate is a KanVibe-maintained fork of `gpui-terminal 0.1.0`.

- Upstream repository: <https://github.com/zortax/gpui-terminal>
- Published crate source commit: `c4e69d239f3d4d8871755b2253155154ee160501`
- Audited upstream main: `51f0292938876c8da3de03f0139088591e3be518`
- Original license: MIT OR Apache-2.0
- Imported source: the crates.io `gpui-terminal-0.1.0` archive

The fork exists because the published `TerminalView` leaves mouse selection and
scrollback event handlers as placeholders and exposes no app-level state accessor
that can implement those behaviors correctly. Upstream scrolling PR #2 remains
open with requested changes and does not implement selection:
<https://github.com/zortax/gpui-terminal/pull/2>.

Keep upstream copyright and license files intact. KanVibe-specific behavior,
tests, and release notes belong in this directory rather than in copied variants.

## KanVibe delta

- scrollback-aware viewport/cursor rendering and a compact position indicator
- fractional trackpad scrolling with bounded mouse/alternate-screen reports
- bottom restoration when the user types or pastes
- Alacritty-native character, semantic-word, and line selections
- selection highlighting, SGR click/drag/hover reports, and Shift override
- GPUI clipboard copy/paste with bracketed-paste handling

The standalone lockfile is committed so the macOS fork test job resolves the
same dependency graph. Real pointer, clipboard, tmux/nvim, resize, and focus
behavior remains part of Issue #310's two-run macOS Phase 5 gate.
