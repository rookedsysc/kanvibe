<div align="center">

# KanVibe

**AI Agent Task Management Kanban Board**

A web-based terminal Kanban board for managing AI coding agent (Claude Code, Gemini CLI, Codex CLI, etc.) tasks in real-time.
Monitor tmux/zellij sessions directly in your browser while tracking task progress on a drag & drop Kanban board.
Automatically track task status via [AI Agent Hooks](#ai-agent-hooks---automatic-status-tracking) — no manual updates needed.

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/rookedsysc)

> Buying me a coffee is nice, but honestly? A contribution would make my day even more. :)

[KO](./docs/README.ko.md) | [ZH](./docs/README.zh.md)

</div>

<div align="center">

[![▶ Watch Demo on YouTube](https://img.youtube.com/vi/8JTrvd3T_Z0/maxresdefault.jpg)](https://www.youtube.com/watch?v=8JTrvd3T_Z0)

**▶ [Watch Demo on YouTube](https://www.youtube.com/watch?v=8JTrvd3T_Z0)**

<table>
  <tr>
    <td width="50%"><img src="./docs/images/detail-page.png" alt="Kanban Board" width="100%"></td>
    <td width="50%"><img src="./docs/images/detail-page.png" alt="Task Detail & Terminal" width="100%"></td>
  </tr>
</table>

</div>

---

## Prerequisites

The `kanvibe` CLI script automatically checks and installs missing dependencies. You can also install them manually:

| Dependency | Version | Required | Install |
|------------|---------|----------|---------|
| [Node.js](https://nodejs.org/) | 24.x | Yes | `brew install node@24` |
| [pnpm](https://pnpm.io/) | latest | Yes | `corepack enable && corepack prepare pnpm@latest --activate` |
| [git](https://git-scm.com/) | latest | Yes | `brew install git` |
| [tmux](https://github.com/tmux/tmux) | latest | Yes | `brew install tmux` |
| [gh](https://cli.github.com/) | latest | Yes | `brew install gh` (requires `gh auth login`) |
| [zellij](https://github.com/zellij-org/zellij) | latest | No | `brew install zellij` |

> KanVibe now uses an embedded SQLite database. Docker is no longer required.

---

## Quick Start

### 1. Configure Environment

```bash
cp .env.example .env
```

| Variable | Description | Default |
|----------|-------------|---------|
| `PORT` | Web server port | `4885` |
| `KANVIBE_USER` | Login username | `admin` |
| `KANVIBE_PASSWORD` | Login password | `changeme` (change this!) |

### 2. Run

```bash
bash kanvibe.sh start          # Interactive mode selection (foreground/background)
bash kanvibe.sh start --fg     # Foreground (output to terminal, Ctrl+C to stop)
bash kanvibe.sh start --bg     # Background (server keeps running after terminal closes)
```

This command checks dependencies (with i18n install prompts), installs packages, prepares the embedded SQLite database, builds, and launches the server.

```bash
bash kanvibe.sh stop
```

Stops the KanVibe server.

Open `http://localhost:4885` in your browser.

### Desktop App Build (DMG / Homebrew-ready)

```bash
pnpm dist
```

This builds the renderer bundle, compiles the desktop main process into `build/main`, generates the bundled seed database, and packages a desktop app through Electron Builder.

- macOS output: `dist/*.dmg`, `dist/*.zip`
- Homebrew distribution: use the generated DMG artifact in a custom Homebrew Cask tap
- Homebrew template: `distribution/homebrew/kanvibe.rb.template`

### Native Module Recovery

If macOS shows a `better-sqlite3` `NODE_MODULE_VERSION` mismatch, KanVibe now verifies the binding before startup and attempts an automatic rebuild when possible.

- browser/server runtime: `pnpm rebuild better-sqlite3`
- desktop runtime: `pnpm exec electron-rebuild -f --build-from-source --only better-sqlite3`

For local desktop runs, `pnpm start` and `pnpm dev` already perform this compatibility check before Electron launches.

Always run these commands under Node 24.x.

---

## Usage Flow

### 1. Register a Project

Search for your local git repository using the **fzf-style folder search** in project settings. KanVibe scans the directory and automatically detects existing worktree branches.

### 2. Create Tasks

Add a TODO task from the Kanban board. When creating a task with a branch name, KanVibe automatically:
- Creates a **git worktree** for the branch
- Spawns a **tmux window** or **zellij tab** for the session
- Links the terminal session to the task

### 3. Work with the Kanban Board

Tasks are managed through 5 statuses: **TODO** → **PROGRESS** → **PENDING** → **REVIEW** → **DONE**

Change statuses via drag & drop, or let [AI Agent Hooks](#ai-agent-hooks---automatic-status-tracking) transition them automatically. When a task moves to **DONE**, its branch, worktree, and terminal session are **automatically deleted**.

### 4. Select Pane Layouts

Each task's terminal page supports multiple pane layouts:

| Layout | Description |
|--------|-------------|
| **Single** | One full-screen pane |
| **Horizontal 2** | Two panes side by side |
| **Vertical 2** | Two panes stacked |
| **Left + Right TB** | Left pane + right top/bottom split |
| **Left TB + Right** | Left top/bottom split + right pane |
| **Quad** | Four equal panes |

Each pane can run a custom command (e.g., `vim`, `htop`, `lazygit`, test runner, etc.). Configure layouts globally or per-project from the settings dialog.

---

## Features

### Kanban Board
- 5-status task management (TODO / PROGRESS / PENDING / REVIEW / DONE)
- Custom task ordering with drag & drop
- Multi-project filtering
- Board page find with `Cmd/Ctrl+F` for visible project/task text
- Global quick search for task detail pages by branch or project name
- Done column pagination
- Real-time WebSocket updates

### Git Worktree Integration
- Automatic git worktree creation when a branch-based task is created
- Worktree scanning: existing branches are auto-registered as TODO tasks
- Automatic cleanup (branch + worktree + session) when task moves to DONE

### Terminal Sessions (tmux / zellij)
- **tmux** and **zellij** are both supported as terminal multiplexers
- Browser-based terminal via xterm.js + WebSocket
- SSH remote terminal support (reads `~/.ssh/config`)
- Non-interactive remote SSH commands reuse an app-local ControlMaster socket pool under `~/.kanvibe` with bounded per-host concurrency
- Remote terminal attach executes tmux/zellij directly over SSH; trusted X11 forwarding (`ssh -Y`) is requested only when local `DISPLAY`, remote `X11Forwarding`, and `xauth` are available
- Nerd Font rendering support

### Quick Task Search
- Open a global search dialog from the configured keyboard shortcut
- Search existing task detail pages by branch name or project name
- Distinguish local tasks from SSH remote tasks directly in search results
- Configure the shortcut in **Project Settings** → **Detail Page**

### Keyboard Shortcuts

| Shortcut | Scope | Action |
|----------|-------|--------|
| `Cmd/Ctrl+F` | Board | Open page find for visible project/task text |
| `Cmd/Ctrl+Shift+O` | Global | Open quick task search by branch or project name (default, configurable) |
| `Cmd/Ctrl+Shift+P` | Board | Open the project filter dropdown |
| `Cmd/Ctrl+Shift+I` | Board | Open the notifications dropdown |
| `Cmd+[` / `Cmd+]` (macOS), `Alt+[` / `Alt+]` (Linux) | Global | Navigate back/forward through app history; back falls back to board home when there is no previous page |
| `Cmd+1/2/3` (macOS), `Alt+1/2/3` (Linux) | Task detail | Activate the numbered detail dock items: info, status/hooks, and AI chat. These shortcuts are intercepted before terminal input |
| `Cmd+4` (macOS), `Alt+4` (Linux) | Task detail | Open the task PR in the browser when a PR exists; otherwise the shortcut belongs to the fourth numbered dock item when present |
| `Cmd/Ctrl+N` | Quick task search | Create a new branch TODO from the currently highlighted task |
| `↑ / ↓ / Enter / Shift+Enter / Esc` | Quick task search | Move selection, open task, open task in a new window, close dialog |
| `↑ / ↓ / Enter / Esc` | Project filter dropdown | Move selection, toggle project filter, close dropdown |
| `↑ / ↓ / Enter / Esc` | Notifications dropdown | Move selection, open notification target, close dropdown |

Task detail dock numbering excludes the back-to-board button and follows the visible dock item order. If a task has a PR URL, PR takes slot 4 and later dock items shift to 5+; without a PR, the next dock item uses slot 4.

### AI Agent Hooks - Automatic Status Tracking
KanVibe integrates with **Claude Code Hooks**, **Gemini CLI Hooks**, **Codex CLI**, and **OpenCode** to automatically track task status. Tasks are managed through 5 statuses:

| Status | Description |
|--------|-------------|
| **TODO** | Initial state when a task is created |
| **PROGRESS** | AI is actively working on the task |
| **PENDING** | AI asked a follow-up question; waiting for user response (Claude Code only) |
| **REVIEW** | AI has finished; awaiting review |
| **DONE** | Task complete — branch, worktree, and terminal session are **automatically deleted** |

#### Claude Code
```
User sends prompt          → PROGRESS
AI asks question (AskUser) → PENDING
User answers               → PROGRESS
AI finishes response       → REVIEW
```

#### Gemini CLI
```
BeforeAgent (user prompt)  → PROGRESS
AfterAgent (agent done)    → REVIEW
```

> Gemini CLI does not have an equivalent to Claude Code's `AskUserQuestion`, so the PENDING state is not available.

#### Codex CLI
```
UserPromptSubmit                → PROGRESS
PermissionRequest (Bash only)  → PENDING
PreToolUse (Bash only)         → PROGRESS
Stop                           → REVIEW
```

KanVibe now uses Codex's current lifecycle hooks model with `.codex/hooks.json` plus `[features].codex_hooks = true` in `.codex/config.toml`, following the latest official docs:

- https://developers.openai.com/codex/hooks
- https://developers.openai.com/codex/config-reference

> Codex's current `PermissionRequest` and `PreToolUse` matchers are Bash-scoped, so `PENDING` represents approval waits rather than every kind of conversational follow-up question.

#### OpenCode
```
User sends message (message.updated, role=user) → PROGRESS
AI asks a question (question.asked)             → PENDING
User answers question (question.replied)        → PROGRESS
Session idle (session.idle)                     → REVIEW
```

OpenCode uses its own [plugin system](https://opencode.ai/docs/plugins/) instead of shell-script hooks. KanVibe generates a TypeScript plugin at `.opencode/plugins/kanvibe-plugin.ts` that subscribes to OpenCode's native event hooks (`message.updated`, `question.asked`, `question.replied`, and `session.idle`) via the `@opencode-ai/plugin` SDK. This means status updates are handled in-process without spawning external shell commands.

All agent hooks are **auto-installed** when you register a project through KanVibe's directory scan or create a task with a worktree. You can also install them individually from the task detail page.

| Agent | Hook Directory | Config File |
|-------|---------------|-------------|
| Claude Code | `.claude/hooks/` | `.claude/settings.json` |
| Gemini CLI | `.gemini/hooks/` | `.gemini/settings.json` |
| Codex CLI | `.codex/hooks/` | `.codex/config.toml`, `.codex/hooks.json` |
| OpenCode | `.opencode/plugins/` | Plugin auto-discovery |

#### Browser Notifications

Task status changes via AI Agent Hooks trigger **browser notifications** with project, branch, and status. **Click to jump directly to the task detail page.**

- **Real-time alerts** — Instant notifications for task status changes
- **Background mode** — Notifications work even when KanVibe is not focused
- **Smart navigation** — Click notification → task detail page (with correct language)
- **Configurable** — Enable/disable per project and filter by status (PROGRESS, PENDING, REVIEW, DONE)

Setup: Browser will prompt for permission on first visit. Configure filters in **Project Settings** → **Notifications**.

#### Hook API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/hooks/start` | POST | Create a new task |
| `/api/hooks/status` | POST | Update task status by `branchName` + `projectName`; if the target is missing, KanVibe still sends a browser notification and returns `404` without changing status |

### GitHub-style Diff View

Review code changes directly in the browser with a GitHub-style diff viewer. Click the **Diff** badge on the task detail page to see all modified files compared to the base branch.

<table>
  <tr>
    <td width="30%"><img src="./docs/images/diff-view-button.png" alt="Diff Badge on Task Detail" width="100%"></td>
    <td width="70%"><img src="./docs/images/diff-view.png" alt="Diff View Page" width="100%"></td>
  </tr>
</table>

- File tree sidebar with changed file count
- Inline diff viewer powered by Monaco Editor
- Edit mode for quick fixes directly in the browser
- Viewed file tracking with checkboxes

### Pane Layout Editor
- 6 layout presets (Single, Horizontal 2, Vertical 2, Left+Right TB, Left TB+Right, Quad)
- Per-pane custom command configuration
- Global and per-project layout settings

### Internationalization (i18n)
- Supported languages: Korean (ko), English (en), Chinese (zh)
- Powered by next-intl

---

## Tech Stack

| Category | Technology |
|----------|------------|
| Frontend/Backend | Next.js 16 (App Router) + React 19 + TypeScript |
| Database | SQLite + TypeORM + better-sqlite3 |
| Styling | Tailwind CSS v4 |
| Terminal | xterm.js + WebSocket + node-pty |
| SSH | system ssh binary |
| Drag & Drop | @hello-pangea/dnd |
| i18n | next-intl |
| Desktop Packaging | Electron + Electron Builder |

---

## License

This project is licensed under the **AGPL-3.0**. You are free to use, modify, and extend it for open-source purposes. Commercial SaaS distribution is not permitted under this license. See [LICENSE](./LICENSE) for details.

---

## Contributing

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) for guidelines.

---

## Inspired By

- [workmux](https://github.com/raine/workmux) — tmux workspace manager
- [vibe-kanban](https://github.com/BloopAI/vibe-kanban) — AI-powered Kanban board
