#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */

const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(process.cwd(), "qa", "scenarios");
const seedPath = "qa/seed/kanvibe-seed.sqlite";

const schema = {
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kanvibe.local/qa/scenario.schema.json",
  "title": "KanVibe QA Scenario",
  "type": "object",
  "required": ["id", "title", "seed", "coverage", "steps", "assertions", "artifacts"],
  "properties": {
    "id": { "type": "string", "pattern": "^S(0[1-9]|1[0-4])-[a-z0-9-]+$" },
    "title": { "type": "string" },
    "seed": { "type": "string" },
    "coverage": { "type": "array", "items": { "type": "string" } },
    "steps": { "type": "array", "items": { "type": "object" } },
    "assertions": { "type": "array", "items": { "type": "object" } },
    "artifacts": {
      "type": "object",
      "required": ["screens", "videos"],
      "properties": {
        "screens": { "type": "array", "items": { "type": "string" } },
        "videos": { "type": "array", "items": { "type": "string" } }
      }
    }
  }
};

const baseLaunch = [
  { action: "launchApp", seed: seedPath, locale: "ko", theme: "dark", viewport: { width: 1440, height: 960 } },
  { action: "waitForBoardReady" },
];

const scenarios = [
  {
    id: "S01-board-load-and-columns",
    title: "Board loads all status columns and seeded cards",
    coverage: ["board", "columns", "seed-db", "project-colors", "priority-badges"],
    steps: [
      ...baseLaunch,
      { action: "captureScreen", name: "board-dark-ko" },
    ],
    assertions: [
      { type: "columns", statuses: ["todo", "progress", "pending", "review", "done"] },
      { type: "taskCountByStatus", expected: { todo: 3, progress: 3, pending: 3, review: 3, done: 3 } },
      { type: "taskVisible", taskId: "qa-task-todo-local", title: "Draft native board shell" },
      { type: "dbCount", table: "projects", expected: 3 },
      { type: "dbCount", table: "kanban_tasks", expected: 15 },
    ],
    artifacts: { screens: ["screens/S01-board-dark-ko.png"], videos: ["videos/S01-board-load-and-columns.mp4"] },
  },
  {
    id: "S02-create-task-modal",
    title: "Create task modal writes a todo task without worktree side effects",
    coverage: ["create-task-modal", "kanban.createTask", "db-write", "board-refresh"],
    steps: [
      ...baseLaunch,
      { action: "openCreateTaskModal" },
      { action: "fillTaskForm", title: "QA created task", description: "Created by S02", priority: "medium" },
      { action: "submitCreateTask" },
      { action: "captureScreen", name: "create-task-result" },
    ],
    assertions: [
      { type: "taskVisible", title: "QA created task", status: "todo" },
      { type: "dbRow", table: "kanban_tasks", where: { title: "QA created task" }, expected: { status: "todo", priority: "medium" } },
      { type: "noWorktreeCreated", title: "QA created task" },
    ],
    artifacts: { screens: ["screens/S02-create-task-result.png"], videos: ["videos/S02-create-task-modal.mp4"] },
  },
  {
    id: "S03-task-detail-terminal-dock",
    title: "Task detail shows dock, metadata, and terminal attachment state",
    coverage: ["task-detail", "dock-shortcuts", "terminal-panel", "tmux-session"],
    steps: [
      ...baseLaunch,
      { action: "openTaskDetail", taskId: "qa-task-progress-terminal" },
      { action: "selectDockItem", item: "terminal" },
      { action: "captureScreen", name: "task-detail-terminal" },
    ],
    assertions: [
      { type: "route", pattern: "/task/qa-task-progress-terminal" },
      { type: "dockItems", expected: ["overview", "status", "terminal", "chat", "aiSessions"] },
      { type: "shortcutLabels", platform: "macos", expectedPrefix: "Cmd+" },
      { type: "taskField", taskId: "qa-task-progress-terminal", field: "session_type", expected: "tmux" },
    ],
    artifacts: { screens: ["screens/S03-task-detail-terminal.png"], videos: ["videos/S03-task-detail-terminal-dock.mp4"] },
  },
  {
    id: "S04-task-detail-pr-and-ai-history",
    title: "Task detail PR slot and AI session history are visible",
    coverage: ["task-detail", "pull-request", "ai-session-history", "dock-slot-order"],
    steps: [
      ...baseLaunch,
      { action: "openTaskDetail", taskId: "qa-task-review-diff" },
      { action: "selectDockItem", item: "pullRequest" },
      { action: "selectDockItem", item: "aiSessions" },
      { action: "captureScreen", name: "task-detail-pr-ai" },
    ],
    assertions: [
      { type: "taskField", taskId: "qa-task-review-diff", field: "pr_url", expectedContains: "/pull/302" },
      { type: "dockItems", expected: ["overview", "status", "terminal", "pullRequest", "chat", "aiSessions"] },
      { type: "aiProviderFilters", expected: ["claude", "codex", "gemini", "opencode"] },
    ],
    artifacts: { screens: ["screens/S04-task-detail-pr-ai.png"], videos: ["videos/S04-task-detail-pr-and-ai-history.mp4"] },
  },
  {
    id: "S05-diff-route-file-tree",
    title: "Diff route shows changed files and file diff pane",
    coverage: ["diff-route", "git-diff", "file-tree", "monaco-parity"],
    steps: [
      ...baseLaunch,
      { action: "openDiffRoute", taskId: "qa-task-review-diff" },
      { action: "selectFirstChangedFile" },
      { action: "captureScreen", name: "diff-route-file-tree" },
    ],
    assertions: [
      { type: "route", pattern: "/diff/qa-task-review-diff" },
      { type: "diffSidebarVisible" },
      { type: "diffPaneVisible" },
      { type: "externalToolBlockerAllowed", tools: ["git"] },
    ],
    artifacts: { screens: ["screens/S05-diff-route-file-tree.png"], videos: ["videos/S05-diff-route-file-tree.mp4"] },
  },
  {
    id: "S06-git-worktree-branch-flow",
    title: "Branch-from-task flow creates branch metadata and preserves base branch",
    coverage: ["branch-task-modal", "git-worktree", "kanban.branchFromTask", "db-write"],
    steps: [
      ...baseLaunch,
      { action: "openTaskContextMenu", taskId: "qa-task-todo-local" },
      { action: "chooseContextAction", actionName: "branchFromTask" },
      { action: "fillBranchTaskForm", projectId: "qa-project-kanvibe", branchName: "qa/branch-from-task", baseBranch: "main", sessionType: "tmux" },
      { action: "submitBranchTask" },
      { action: "captureScreen", name: "branch-task-result" },
    ],
    assertions: [
      { type: "dbRow", table: "kanban_tasks", where: { branch_name: "qa/branch-from-task" }, expected: { status: "progress", base_branch: "main", session_type: "tmux" } },
      { type: "taskVisible", branchName: "qa/branch-from-task", status: "progress" },
    ],
    artifacts: { screens: ["screens/S06-branch-task-result.png"], videos: ["videos/S06-git-worktree-branch-flow.mp4"] },
  },
  {
    id: "S07-board-drag-drop-status",
    title: "Dragging a card updates status and display order",
    coverage: ["drag-drop", "kanban.updateTaskStatus", "display-order", "board-refresh"],
    steps: [
      ...baseLaunch,
      { action: "dragTaskToColumn", taskId: "qa-task-todo-local", toStatus: "progress", position: 0 },
      { action: "captureScreen", name: "drag-drop-progress" },
    ],
    assertions: [
      { type: "taskVisible", taskId: "qa-task-todo-local", status: "progress" },
      { type: "dbRow", table: "kanban_tasks", where: { id: "qa-task-todo-local" }, expected: { status: "progress" } },
    ],
    artifacts: { screens: ["screens/S07-drag-drop-progress.png"], videos: ["videos/S07-board-drag-drop-status.mp4"] },
  },
  {
    id: "S08-context-menu-status-and-delete",
    title: "Context menu changes status and delete cleans task row",
    coverage: ["context-menu", "delete-task", "done-confirm", "resource-cleanup"],
    steps: [
      ...baseLaunch,
      { action: "openTaskContextMenu", taskId: "qa-task-pending-no-branch" },
      { action: "chooseContextAction", actionName: "moveToReview" },
      { action: "openTaskContextMenu", taskId: "qa-task-pending-no-branch" },
      { action: "chooseContextAction", actionName: "deleteTask" },
      { action: "confirmDialog" },
      { action: "captureScreen", name: "context-delete-result" },
    ],
    assertions: [
      { type: "taskNotVisible", taskId: "qa-task-pending-no-branch" },
      { type: "dbMissing", table: "kanban_tasks", where: { id: "qa-task-pending-no-branch" } },
    ],
    artifacts: { screens: ["screens/S08-context-delete-result.png"], videos: ["videos/S08-context-menu-status-and-delete.mp4"] },
  },
  {
    id: "S09-vim-keyboard-shortcuts-search",
    title: "Vim commands and global task search keyboard flows work",
    coverage: ["vim-mode", "task-search", "keyboard-shortcuts", "page-find"],
    steps: [
      ...baseLaunch,
      { action: "focusTask", taskId: "qa-task-todo-local" },
      { action: "pressKeys", keys: [":"] },
      { action: "typeText", text: "move review" },
      { action: "pressKeys", keys: ["Enter"] },
      { action: "pressShortcut", command: "taskSearchDefault" },
      { action: "typeText", text: "diff renderer" },
      { action: "captureScreen", name: "keyboard-search" },
    ],
    assertions: [
      { type: "taskVisible", taskId: "qa-task-todo-local", status: "review" },
      { type: "searchResultVisible", taskId: "qa-task-review-diff" },
      { type: "dbRow", table: "kanban_tasks", where: { id: "qa-task-todo-local" }, expected: { status: "review" } },
    ],
    artifacts: { screens: ["screens/S09-keyboard-search.png"], videos: ["videos/S09-vim-keyboard-shortcuts-search.mp4"] },
  },
  {
    id: "S10-settings-pane-layout",
    title: "Settings and pane layout persistence round-trip",
    coverage: ["settings", "app-settings", "pane-layout", "theme", "language"],
    steps: [
      ...baseLaunch,
      { action: "openSettings" },
      { action: "setAppSetting", key: "vim_mode_enabled", value: "false" },
      { action: "openPaneLayoutRoute", projectId: "qa-project-kanvibe" },
      { action: "selectPaneLayout", layoutType: "vertical_2" },
      { action: "captureScreen", name: "settings-pane-layout" },
    ],
    assertions: [
      { type: "dbRow", table: "app_settings", where: { key: "vim_mode_enabled" }, expected: { value: "false" } },
      { type: "dbRow", table: "pane_layout_configs", where: { project_id: "qa-project-kanvibe" }, expected: { layout_type: "vertical_2" } },
      { type: "route", pattern: "/pane-layout" },
    ],
    artifacts: { screens: ["screens/S10-settings-pane-layout.png"], videos: ["videos/S10-settings-pane-layout.mp4"] },
  },
  {
    id: "S11-notifications-hooks-background-sync",
    title: "Notifications, hook status, and background sync settings are visible",
    coverage: ["notifications", "hooks", "background-sync", "release-update-dialog"],
    steps: [
      ...baseLaunch,
      { action: "openNotificationCenter" },
      { action: "openTaskDetail", taskId: "qa-task-review-ai-history" },
      { action: "selectDockItem", item: "hooks" },
      { action: "captureScreen", name: "notifications-hooks" },
    ],
    assertions: [
      { type: "notificationCenterVisible" },
      { type: "hookStatusVisible", taskId: "qa-task-review-ai-history" },
      { type: "dbRow", table: "app_settings", where: { key: "background_sync_enabled" }, expected: { value: "false" } },
    ],
    artifacts: { screens: ["screens/S11-notifications-hooks.png"], videos: ["videos/S11-notifications-hooks-background-sync.mp4"] },
  },
  {
    id: "S12-project-filter-and-done-pagination",
    title: "Project filter narrows board and Done column count remains accurate",
    coverage: ["project-filter", "done-column", "load-more-done", "project-colors"],
    steps: [
      ...baseLaunch,
      { action: "openProjectFilter" },
      { action: "selectProjects", projectIds: ["qa-project-api"] },
      { action: "captureScreen", name: "project-filter-api" },
    ],
    assertions: [
      { type: "onlyProjectTasksVisible", projectId: "qa-project-api" },
      { type: "taskVisible", taskId: "qa-task-done-remote", status: "done" },
      { type: "taskNotVisible", taskId: "qa-task-todo-local" },
    ],
    artifacts: { screens: ["screens/S12-project-filter-api.png"], videos: ["videos/S12-project-filter-and-done-pagination.mp4"] },
  },
  {
    id: "S13-task-detail-existing-window-focus",
    title: "Opening an already-open task detail focuses the existing route",
    coverage: ["window-policy", "task-navigation", "desktop-preload", "shortcut-routing"],
    steps: [
      ...baseLaunch,
      { action: "openTaskDetail", taskId: "qa-task-progress-pr" },
      { action: "recordWindowCount", name: "beforeSecondOpen" },
      { action: "openTaskDetail", taskId: "qa-task-progress-pr" },
      { action: "captureScreen", name: "existing-window-focus" },
    ],
    assertions: [
      { type: "windowCountUnchanged", baseline: "beforeSecondOpen" },
      { type: "route", pattern: "/task/qa-task-progress-pr" },
      { type: "taskTitleVisible", title: "Wire PR status card" },
    ],
    artifacts: { screens: ["screens/S13-existing-window-focus.png"], videos: ["videos/S13-task-detail-existing-window-focus.mp4"] },
  },
  {
    id: "S14-remote-session-dependencies",
    title: "Remote session dependency UI reports SSH-scoped tmux/zellij checks",
    coverage: ["remote-session", "ssh-host", "tmux", "zellij", "dependency-install"],
    steps: [
      ...baseLaunch,
      { action: "openTaskDetail", taskId: "qa-task-todo-remote" },
      { action: "selectDockItem", item: "terminal" },
      { action: "openSessionDependencyPanel" },
      { action: "captureScreen", name: "remote-session-dependencies" },
    ],
    assertions: [
      { type: "taskField", taskId: "qa-task-todo-remote", field: "ssh_host", expected: "qa-remote" },
      { type: "sessionDependencyVisible", sessionType: "zellij", sshHost: "qa-remote" },
      { type: "noGenericEnvLeak", forbiddenKeys: ["PORT", "HOST", "NODE_ENV"] },
    ],
    artifacts: { screens: ["screens/S14-remote-session-dependencies.png"], videos: ["videos/S14-remote-session-dependencies.mp4"] },
  },
];

function scenarioFileName(scenario) {
  return `${scenario.id}.json`;
}

function validateScenarioIds() {
  const expected = Array.from({ length: 14 }, (_, index) => `S${String(index + 1).padStart(2, "0")}`);
  const actual = scenarios.map((scenario) => scenario.id.slice(0, 3));

  for (const id of expected) {
    if (!actual.includes(id)) {
      throw new Error(`Missing scenario ${id}`);
    }
  }
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function writeManifest() {
  const lines = [
    "# QA Scenario Manifest",
    "",
    `Generated by: \`pnpm qa:scenarios\``,
    `Seed: \`${seedPath}\``,
    "",
    "| Scenario | Title | Screens | Videos |",
    "| --- | --- | --- | --- |",
  ];

  for (const scenario of scenarios) {
    lines.push(`| \`${scenario.id}\` | ${scenario.title} | ${scenario.artifacts.screens.length} | ${scenario.artifacts.videos.length} |`);
  }

  lines.push("");
  fs.writeFileSync(path.join(outputDir, "MANIFEST.md"), lines.join("\n"), "utf8");
}

validateScenarioIds();
fs.mkdirSync(outputDir, { recursive: true });
writeJson(path.join(outputDir, "scenario.schema.json"), schema);

for (const scenario of scenarios) {
  writeJson(path.join(outputDir, scenarioFileName({ ...scenario, seed: seedPath })), {
    "$schema": "./scenario.schema.json",
    seed: seedPath,
    ...scenario,
  });
}

writeManifest();
console.log(`[qa:scenarios] Wrote ${scenarios.length} scenarios to ${outputDir}`);
