import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { buildSeedDatabase } from "../src/lib/sqliteSchema";

const outputPath = path.join(process.cwd(), "qa", "seed", "kanvibe-seed.sqlite");
const fixedNow = "2026-07-08T00:00:00.000Z";

interface ProjectSeed {
  id: string;
  name: string;
  repoPath: string;
  defaultBranch: string;
  sshHost: string | null;
  isWorktree: number;
  color: string;
}

interface TaskSeed {
  id: string;
  title: string;
  description: string;
  status: string;
  branchName: string | null;
  worktreePath: string | null;
  sessionType: string | null;
  sessionName: string | null;
  sshHost: string | null;
  agentType: string | null;
  projectId: string | null;
  baseBranch: string | null;
  prUrl: string | null;
  priority: string | null;
  displayOrder: number;
}

const projects: ProjectSeed[] = [
  {
    id: "qa-project-kanvibe",
    name: "KanVibe App",
    repoPath: "/tmp/kanvibe-qa/repos/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: 0,
    color: "#0064FF",
  },
  {
    id: "qa-project-api",
    name: "Remote API",
    repoPath: "/srv/kanvibe-api",
    defaultBranch: "develop",
    sshHost: "qa-remote",
    isWorktree: 0,
    color: "#00A870",
  },
  {
    id: "qa-project-docs-worktree",
    name: "Docs Worktree",
    repoPath: "/tmp/kanvibe-qa/repos/docs__worktrees/feat-native-docs",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: 1,
    color: "#8B5CF6",
  },
];

const tasks: TaskSeed[] = [
  {
    id: "qa-task-todo-local",
    title: "Draft native board shell",
    description: "Todo local task with branch and tmux metadata.",
    status: "todo",
    branchName: "feat/native-board-shell",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feat-native-board-shell",
    sessionType: "tmux",
    sessionName: "kanvibe-native-board-shell",
    sshHost: null,
    agentType: "codex",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: null,
    priority: "high",
    displayOrder: 0,
  },
  {
    id: "qa-task-todo-remote",
    title: "Validate remote hook installer",
    description: "Todo remote task with SSH host and zellij session.",
    status: "todo",
    branchName: "fix/remote-hooks",
    worktreePath: "/srv/kanvibe-api__worktrees/fix-remote-hooks",
    sessionType: "zellij",
    sessionName: "api-fix-remote-hooks",
    sshHost: "qa-remote",
    agentType: "claude",
    projectId: "qa-project-api",
    baseBranch: "develop",
    prUrl: null,
    priority: "medium",
    displayOrder: 1,
  },
  {
    id: "qa-task-todo-unassigned",
    title: "Triage unassigned idea",
    description: "Todo task without project, branch, session, or priority.",
    status: "todo",
    branchName: null,
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    projectId: null,
    baseBranch: null,
    prUrl: null,
    priority: null,
    displayOrder: 2,
  },
  {
    id: "qa-task-progress-terminal",
    title: "Port terminal dock",
    description: "Progress task with active tmux session.",
    status: "progress",
    branchName: "feat/native-terminal",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feat-native-terminal",
    sessionType: "tmux",
    sessionName: "kanvibe-native-terminal",
    sshHost: null,
    agentType: "codex",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: null,
    priority: "high",
    displayOrder: 0,
  },
  {
    id: "qa-task-progress-pr",
    title: "Wire PR status card",
    description: "Progress task with PR URL for task detail dock slot coverage.",
    status: "progress",
    branchName: "feat/pr-status-card",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feat-pr-status-card",
    sessionType: "tmux",
    sessionName: "kanvibe-pr-status-card",
    sshHost: null,
    agentType: "claude",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: "https://github.com/rookedsysc/kanvibe/pull/301",
    priority: "medium",
    displayOrder: 1,
  },
  {
    id: "qa-task-progress-docs",
    title: "Refresh docs screenshots",
    description: "Progress docs worktree task.",
    status: "progress",
    branchName: "feat/native-docs",
    worktreePath: "/tmp/kanvibe-qa/repos/docs__worktrees/feat-native-docs",
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: "gemini",
    projectId: "qa-project-docs-worktree",
    baseBranch: "main",
    prUrl: null,
    priority: "low",
    displayOrder: 2,
  },
  {
    id: "qa-task-pending-review",
    title: "Await design parity review",
    description: "Pending task with high priority.",
    status: "pending",
    branchName: "chore/design-parity",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/chore-design-parity",
    sessionType: "tmux",
    sessionName: "kanvibe-design-parity",
    sshHost: null,
    agentType: "opencode",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: null,
    priority: "high",
    displayOrder: 0,
  },
  {
    id: "qa-task-pending-remote",
    title: "Remote session dependency check",
    description: "Pending remote task for tmux/zellij dependency UI.",
    status: "pending",
    branchName: "chore/remote-session-deps",
    worktreePath: "/srv/kanvibe-api__worktrees/chore-remote-session-deps",
    sessionType: "tmux",
    sessionName: "api-remote-session-deps",
    sshHost: "qa-remote",
    agentType: "codex",
    projectId: "qa-project-api",
    baseBranch: "develop",
    prUrl: null,
    priority: "medium",
    displayOrder: 1,
  },
  {
    id: "qa-task-pending-no-branch",
    title: "Confirm copy updates",
    description: "Pending task without a branch.",
    status: "pending",
    branchName: null,
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    projectId: "qa-project-docs-worktree",
    baseBranch: null,
    prUrl: null,
    priority: "low",
    displayOrder: 2,
  },
  {
    id: "qa-task-review-diff",
    title: "Review diff renderer",
    description: "Review task for file tree and Monaco diff parity.",
    status: "review",
    branchName: "feat/native-diff",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feat-native-diff",
    sessionType: "tmux",
    sessionName: "kanvibe-native-diff",
    sshHost: null,
    agentType: "claude",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: "https://github.com/rookedsysc/kanvibe/pull/302",
    priority: "high",
    displayOrder: 0,
  },
  {
    id: "qa-task-review-ai-history",
    title: "Review AI session history",
    description: "Review task backed by file-based AI session fixtures.",
    status: "review",
    branchName: "feat/ai-session-history",
    worktreePath: "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feat-ai-session-history",
    sessionType: "zellij",
    sessionName: "kanvibe-ai-session-history",
    sshHost: null,
    agentType: "codex",
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: null,
    priority: "medium",
    displayOrder: 1,
  },
  {
    id: "qa-task-review-remote-pr",
    title: "Review remote PR sync",
    description: "Remote review task with merged PR URL fixture.",
    status: "review",
    branchName: "feat/remote-pr-sync",
    worktreePath: "/srv/kanvibe-api__worktrees/feat-remote-pr-sync",
    sessionType: "tmux",
    sessionName: "api-remote-pr-sync",
    sshHost: "qa-remote",
    agentType: "gemini",
    projectId: "qa-project-api",
    baseBranch: "develop",
    prUrl: "https://github.com/rookedsysc/kanvibe-api/pull/88",
    priority: "medium",
    displayOrder: 2,
  },
  {
    id: "qa-task-done-migrated",
    title: "Document migrated shortcuts",
    description: "Done task for completed shortcut documentation.",
    status: "done",
    branchName: "docs/shortcut-ledger",
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: "claude",
    projectId: "qa-project-docs-worktree",
    baseBranch: "main",
    prUrl: "https://github.com/rookedsysc/kanvibe-docs/pull/12",
    priority: "low",
    displayOrder: 0,
  },
  {
    id: "qa-task-done-cleanup",
    title: "Clean stale worktree",
    description: "Done task with no live resources.",
    status: "done",
    branchName: "chore/cleanup-worktree",
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    projectId: "qa-project-kanvibe",
    baseBranch: "main",
    prUrl: null,
    priority: "medium",
    displayOrder: 1,
  },
  {
    id: "qa-task-done-remote",
    title: "Archive remote release branch",
    description: "Done remote task used for done-column pagination assertions.",
    status: "done",
    branchName: "release/archive-remote",
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: "qa-remote",
    agentType: "codex",
    projectId: "qa-project-api",
    baseBranch: "develop",
    prUrl: "https://github.com/rookedsysc/kanvibe-api/pull/77",
    priority: "high",
    displayOrder: 2,
  },
];

const settings = [
  ["sidebar_default_collapsed", "false"],
  ["sidebar_hint_dismissed", "false"],
  ["done_alert_dismissed", "false"],
  ["notification_enabled", "true"],
  ["notification_statuses", JSON.stringify(["progress", "pending", "review"])],
  ["background_sync_enabled", "false"],
  ["background_sync_interval_ms", "300000"],
  ["default_session_type", "tmux"],
  ["task_search_shortcut", "Mod+Shift+O"],
  ["vim_mode_enabled", "true"],
  ["theme_preference", "dark"],
];

function insertProjects(database: Database.Database): void {
  const statement = database.prepare(`
    INSERT INTO projects (id, name, repo_path, default_branch, ssh_host, is_worktree, color, created_at)
    VALUES (@id, @name, @repoPath, @defaultBranch, @sshHost, @isWorktree, @color, @createdAt)
  `);

  for (const project of projects) {
    statement.run({ ...project, createdAt: fixedNow });
  }
}

function insertTasks(database: Database.Database): void {
  const statement = database.prepare(`
    INSERT INTO kanban_tasks (
      id, title, description, status, branch_name, worktree_path, session_type, session_name,
      ssh_host, agent_type, project_id, base_branch, pr_url, priority, display_order,
      created_at, updated_at
    )
    VALUES (
      @id, @title, @description, @status, @branchName, @worktreePath, @sessionType, @sessionName,
      @sshHost, @agentType, @projectId, @baseBranch, @prUrl, @priority, @displayOrder,
      @createdAt, @updatedAt
    )
  `);

  for (const task of tasks) {
    statement.run({ ...task, createdAt: fixedNow, updatedAt: fixedNow });
  }
}

function insertSettings(database: Database.Database): void {
  const statement = database.prepare(`
    INSERT INTO app_settings (id, key, value, created_at, updated_at)
    VALUES (@id, @key, @value, @createdAt, @updatedAt)
  `);

  for (const [index, [key, value]] of settings.entries()) {
    statement.run({
      id: `qa-setting-${String(index + 1).padStart(2, "0")}`,
      key,
      value,
      createdAt: fixedNow,
      updatedAt: fixedNow,
    });
  }
}

function insertPaneLayouts(database: Database.Database): void {
  const statement = database.prepare(`
    INSERT INTO pane_layout_configs (id, layout_type, panes, project_id, is_global, created_at, updated_at)
    VALUES (@id, @layoutType, @panes, @projectId, @isGlobal, @createdAt, @updatedAt)
  `);

  const rows = [
    {
      id: "qa-layout-global-quad",
      layoutType: "quad",
      panes: JSON.stringify([
        { position: 0, command: "pnpm test" },
        { position: 1, command: "pnpm check" },
        { position: 2, command: "git status --short" },
        { position: 3, command: "pnpm qa:electron" },
      ]),
      projectId: null,
      isGlobal: 1,
    },
    {
      id: "qa-layout-kanvibe-horizontal",
      layoutType: "horizontal_2",
      panes: JSON.stringify([
        { position: 0, command: "pnpm desktop:dev" },
        { position: 1, command: "pnpm test -- --watch" },
      ]),
      projectId: "qa-project-kanvibe",
      isGlobal: 0,
    },
  ];

  for (const row of rows) {
    statement.run({ ...row, createdAt: fixedNow, updatedAt: fixedNow });
  }
}

function writeManifest(): void {
  const manifestPath = path.join(path.dirname(outputPath), "MANIFEST.md");
  const lines = [
    "# QA Seed Manifest",
    "",
    `Generated by: \`pnpm qa:seed\``,
    `Database: \`${path.relative(process.cwd(), outputPath)}\``,
    "",
    "Contents:",
    "",
    `- Projects: ${projects.length}`,
    `- Tasks: ${tasks.length} total, 3 in each status column`,
    `- App settings: ${settings.length}`,
    "- Pane layouts: 2",
    "",
    "Status coverage:",
    "",
    "- `todo`: `qa-task-todo-local`, `qa-task-todo-remote`, `qa-task-todo-unassigned`",
    "- `progress`: `qa-task-progress-terminal`, `qa-task-progress-pr`, `qa-task-progress-docs`",
    "- `pending`: `qa-task-pending-review`, `qa-task-pending-remote`, `qa-task-pending-no-branch`",
    "- `review`: `qa-task-review-diff`, `qa-task-review-ai-history`, `qa-task-review-remote-pr`",
    "- `done`: `qa-task-done-migrated`, `qa-task-done-cleanup`, `qa-task-done-remote`",
    "",
  ];

  fs.writeFileSync(manifestPath, lines.join("\n"), "utf8");
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
buildSeedDatabase(outputPath);

const database = new Database(outputPath);
try {
  database.pragma("foreign_keys = ON");
  database.transaction(() => {
    insertProjects(database);
    insertTasks(database);
    insertSettings(database);
    insertPaneLayouts(database);
  })();
} finally {
  database.close();
}

writeManifest();
console.log(`[qa:seed] Seed database ready: ${outputPath}`);
