<div align="center">

# KanVibe

**AI Agent Task Management Kanban Board**

A web-based terminal Kanban board for managing AI coding agent (Claude Code, etc.) tasks in real-time.
Monitor tmux/zellij sessions directly in your browser while tracking task progress on a drag & drop Kanban board.

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/rookedsysc)

> Buying me a coffee is nice, but honestly? A contribution would make my day even more. :)

[KO](./docs/README.ko.md) | [ZH](./docs/README.zh.md)

</div>

---

## Quick Start

### 1. Configure Environment

```bash
cp .env.example .env
```

| Variable | Description | Default |
|----------|-------------|---------|
| `PORT` | Web server port | `4885` |
| `DB_PORT` | PostgreSQL port | `4886` |
| `KANVIBE_USER` | Login username | `admin` |
| `KANVIBE_PASSWORD` | Login password | `changeme` (change this!) |

### 2. Run

```bash
bash start.sh
```

This single command handles everything: dependency installation, PostgreSQL startup, database migration, build, and server launch.

Open `http://localhost:4885` in your browser.

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

Drag and drop tasks across 4 columns: **TODO** → **PROGRESS** → **REVIEW** → **DONE**

When a task moves to **DONE**, its branch, worktree, and terminal session are automatically cleaned up.

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
- 4-column drag & drop board (TODO / PROGRESS / REVIEW / DONE)
- Custom task ordering with drag & drop
- Multi-project filtering
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
- Nerd Font rendering support

### Claude Code Hooks - Automatic Status Tracking
KanVibe integrates with **Claude Code Hooks** to automatically track task status:

```
User sends prompt     → Task moves to PROGRESS
AI asks a question    → Task moves to PENDING
User answers          → Task moves to PROGRESS
AI finishes response  → Task moves to REVIEW
```

Hooks are **auto-installed** when you register a project through KanVibe's directory scan. Hook scripts are placed in your project's `.claude/hooks/` directory.

#### Hook API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/hooks/start` | POST | Create a new task |
| `/api/hooks/status` | POST | Update task status by `branchName` + `projectName` |

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
| Database | PostgreSQL 16 + TypeORM |
| Styling | Tailwind CSS v4 |
| Terminal | xterm.js + WebSocket + node-pty |
| SSH | ssh2 (Node.js) |
| Drag & Drop | @hello-pangea/dnd |
| i18n | next-intl |
| Container | Docker Compose |

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
