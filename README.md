<div align="center">

# KanVibe

**AI Agent Task Management Kanban Board**

A desktop Kanban board for managing AI coding agent (Claude Code, Gemini CLI, Codex CLI, etc.) tasks in real-time.
Monitor tmux/zellij sessions with an embedded terminal while tracking task progress on a drag & drop Kanban board.
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

| Dependency | Version | Required | Install |
|------------|---------|----------|---------|
| [Node.js](https://nodejs.org/) | >= 22 | Yes | `brew install node` |
| [pnpm](https://pnpm.io/) | latest | Yes | `corepack enable && corepack prepare pnpm@latest --activate` |
| [git](https://git-scm.com/) | latest | Yes | `brew install git` |
| [tmux](https://github.com/tmux/tmux) | latest | Yes | `brew install tmux` |
| [gh](https://cli.github.com/) | latest | Yes | `brew install gh` (requires `gh auth login`) |
| [zellij](https://github.com/zellij-org/zellij) | latest | No | `brew install zellij` |

---

## Quick Start (Development)

```bash
pnpm install
pnpm dev
```

This starts the Next.js dev server on port 8888, then launches the Electron window connected to it. DevTools open automatically.

The SQLite database is created at the Electron `userData` path with `(development)` suffix to keep it separate from production data.

---

## Build & Package

### Build only (no installer)

```bash
pnpm build
```

This runs two steps:
1. `next build` — exports the Next.js app as static HTML to `out/`
2. `node scripts/build-main.mjs` — bundles the Electron main/preload TypeScript via esbuild to `electron/`

### Package as installer

```bash
pnpm pack
```

Runs `pnpm build` then `electron-builder` to produce platform-specific installers. Output goes to `dist/`.

| Platform | Formats | Notes |
|----------|---------|-------|
| macOS | `.dmg`, `.zip` | Hardened runtime enabled |
| Windows | `.exe` (NSIS) | — |
| Linux | `.AppImage`, `.deb` | — |

> To build for the current platform only, run `pnpm pack` as-is. Cross-platform builds may require additional configuration.

---

## Project Structure

```
main/                   # Electron main process
├── background.ts       # App entry point (window creation, IPC setup)
├── preload.ts          # Preload script (contextBridge)
├── database.ts         # SQLite (better-sqlite3) + TypeORM setup
├── updater.ts          # electron-updater auto-update
├── helpers/
│   └── create-window.ts
└── ipc/                # IPC handlers (kanban, project, diff, hooks, terminal, etc.)
src/                    # Next.js renderer (App Router)
├── app/                # Pages & layouts
├── components/         # React components
├── entities/           # TypeORM entities
├── migrations/         # SQLite migrations
└── lib/                # Shared utilities
scripts/
└── build-main.mjs      # esbuild bundler for Electron main process
electron-builder.yml     # electron-builder config
```

### Database

SQLite (via `better-sqlite3`) is used as the embedded database. No external database server is needed.

- **Development**: `<userData> (development)/kanvibe.dev.sqlite`
- **Production**: `<userData>/kanvibe.sqlite`

WAL mode is enabled for concurrent read performance. Migrations run automatically on app start.

### Ports

| Port | Service | Description |
|------|---------|-------------|
| `8888` | Next.js dev server | Renderer in development mode |
| `4885` | Hooks HTTP server | AI agent hook endpoints (`POST /api/hooks/start`, `POST /api/hooks/status`) |
| `4884` | Terminal WebSocket | xterm.js terminal connections |

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
- Done column pagination
- IPC-based real-time updates

### Git Worktree Integration
- Automatic git worktree creation when a branch-based task is created
- Worktree scanning: existing branches are auto-registered as TODO tasks
- Automatic cleanup (branch + worktree + session) when task moves to DONE

### Terminal Sessions (tmux / zellij)
- **tmux** and **zellij** are both supported as terminal multiplexers
- Embedded terminal via xterm.js + WebSocket
- SSH remote terminal support (reads `~/.ssh/config`)
- Nerd Font rendering support

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

#### Codex CLI (Partial Support)
```
agent-turn-complete (agent done) → REVIEW
```

> Codex CLI currently only supports the `agent-turn-complete` notification event via the `notify` config. PROGRESS and PENDING transitions are not yet available. OpenAI is [actively designing a hooks system](https://github.com/openai/codex/discussions/2150) — full support will be added when it ships.

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
| Codex CLI | `.codex/hooks/` | `.codex/config.toml` |
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
| `/api/hooks/status` | POST | Update task status by `branchName` + `projectName` |

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
| Desktop | Electron 40 |
| Renderer | Next.js 16 (App Router) + React 19 + TypeScript |
| Database | SQLite (better-sqlite3) + TypeORM |
| Styling | Tailwind CSS v4 |
| Terminal | xterm.js + WebSocket + node-pty |
| SSH | ssh2 (Node.js) |
| Drag & Drop | @hello-pangea/dnd |
| i18n | next-intl |
| Build | esbuild + electron-builder |
| Auto Update | electron-updater (GitHub Releases) |

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
