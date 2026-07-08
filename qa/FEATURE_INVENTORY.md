# KanVibe Feature Inventory

This is the parity ledger for the Rust + GPUI migration. Keep every checkbox unchecked until Phase 5 has command-backed evidence for the native app against the Electron baseline.

## Inventory Rules

- Source baseline: Electron app under `electron/`, `src/desktop/`, `src/components/`, `src/entities/`, `src/lib/`, `src/migrations/`, `messages/`, and existing `qa/electron/`.
- Verification method values:
  - Screenshot comparison: Electron baseline PNG/video compared to native structural parity.
  - Behavior scenario: `qa/scenarios/S01-S14*.json` replayed by both Electron and native runners.
  - DB assert: SQLite query/result check against the seed DB copy.
  - Unit/integration test: focused Vitest or Cargo test.
  - Artifact inspection: manifest, logs, package output, generated files, or diagnostics.
- Required locales: `ko` and `en`. `zh` exists in the Electron message catalog and should be recorded as a follow-up unless native scope expands.
- Required target: macOS first. Linux runtime observations are allowed only as caveats and do not replace macOS evidence.

## Routes And Shell

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Locale redirect from `#/` to default locale | `src/desktop/renderer/App.tsx` | Behavior scenario + unit test |
| [ ] | Locale shell for `/:locale` with `NextIntlClientProvider` | `src/desktop/renderer/App.tsx`, `messages/ko.json`, `messages/en.json` | Screenshot comparison + unit/integration test |
| [ ] | Board route `/:locale` | `src/desktop/renderer/routes/BoardRoute.tsx` | Screenshot comparison + scenarios S01, S02, S07, S08, S09, S12 |
| [ ] | Settings route `/:locale/settings` | `src/desktop/renderer/routes/SettingsRoute.tsx` | Screenshot comparison + scenario S10 + DB assert |
| [ ] | Pane layout route `/:locale/pane-layout` | `src/desktop/renderer/routes/PaneLayoutRoute.tsx` | Screenshot comparison + DB assert + unit/integration test |
| [ ] | Task detail route `/:locale/task/:id` | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + scenarios S03, S04, S13, S14 |
| [ ] | Diff route `/:locale/task/:id/diff` | `src/desktop/renderer/routes/DiffRoute.tsx` | Screenshot comparison + scenario S05 |
| [ ] | Not found route `/:locale/*` | `src/desktop/renderer/routes/NotFoundRoute.tsx` | Screenshot comparison + behavior scenario |
| [ ] | Lazy route loading fallback | `src/desktop/renderer/App.tsx` | Screenshot comparison + unit test |
| [ ] | Initial loading timeout fallback for board/task/detail/diff/settings/pane layout | `src/desktop/renderer/utils/loadingTimeout.ts`, route files | Unit/integration test + artifact inspection |
| [ ] | Renderer bootstrap and error logging | `src/desktop/renderer/main.tsx`, `electron/preload.js`, `electron/main.js` | Unit/integration test + artifact inspection |

## Board Screen

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Board skeleton with five column placeholders | `src/desktop/renderer/routes/BoardRoute.tsx` | Screenshot comparison |
| [ ] | Board header in normal and macOS titlebar layouts | `src/components/Board.tsx` | Screenshot comparison |
| [ ] | Project multi-filter dropdown with project chips and worktree project inclusion | `src/components/Board.tsx`, `src/components/ProjectSelector.tsx` | Screenshot comparison + behavior scenario + unit test |
| [ ] | New task button opens create task modal | `src/components/Board.tsx`, `src/components/CreateTaskModal.tsx` | Screenshot comparison + scenario S01 |
| [ ] | Settings button navigates to settings route | `src/components/Board.tsx` | Behavior scenario |
| [ ] | Notification center button and unread badge | `src/components/NotificationCenterButton.tsx` | Screenshot comparison + scenario S11 |
| [ ] | Page find bar open/close, native find call, and highlighted results | `src/components/BoardPageFindBar.tsx` | Screenshot comparison + scenario S07 + unit test |
| [ ] | Five status columns: todo, progress, pending, review, done | `src/components/Column.tsx`, `src/entities/KanbanTask.ts` | Screenshot comparison + DB assert |
| [ ] | Done column pagination/load more | `src/components/Column.tsx`, `src/desktop/main/services/kanbanService.ts` | Behavior scenario + DB assert |
| [ ] | Task card project color stripe and deterministic project color fallback | `src/components/TaskCard.tsx`, `src/lib/projectColor.ts` | Screenshot comparison + unit test |
| [ ] | Task card priority, branch/base branch, PR, project, agent tags | `src/components/TaskCard.tsx` | Screenshot comparison + seed DB assert |
| [ ] | Task card hover/focus states | `src/components/TaskCard.tsx` | Screenshot comparison |
| [ ] | Task card click navigates via same-task focus policy | `src/components/TaskCard.tsx`, `src/desktop/renderer/utils/taskNavigation.ts` | Scenario S13 + unit test |
| [ ] | Shift+Enter opens task detail in a new window | `src/components/TaskCard.tsx`, `src/components/Board.tsx` | Behavior scenario + unit test |
| [ ] | Shift+F10 opens task context menu from keyboard | `src/components/TaskCard.tsx`, `src/components/Board.tsx` | Behavior scenario + unit test |
| [ ] | Arrow and Vim-style task card focus navigation | `src/components/TaskCard.tsx`, `src/components/Board.tsx` | Unit test + behavior scenario |
| [ ] | Drag/drop reorder within a column | `src/components/Board.tsx`, `src/desktop/main/services/kanbanService.ts` | Scenario S02 + DB assert |
| [ ] | Drag/drop move across columns | `src/components/Board.tsx`, `src/desktop/main/services/kanbanService.ts` | Scenario S02 + DB assert |
| [ ] | Move to done with cleanable resources opens Done confirmation | `src/components/Board.tsx`, `src/components/DoneConfirmDialog.tsx` | Scenario S09 + DB assert |
| [ ] | Board context menu opens at pointer/keyboard position | `src/components/TaskContextMenu.tsx`, `src/components/Board.tsx` | Screenshot comparison + unit test |
| [ ] | Context menu branch/create-branch-todo action changes by branch existence | `src/components/TaskContextMenu.tsx` | Screenshot comparison + scenario S06 |
| [ ] | Context menu status submenu with radio behavior and keyboard navigation | `src/components/TaskContextMenu.tsx` | Screenshot comparison + unit test |
| [ ] | Context menu delete action with browser confirm | `src/components/TaskContextMenu.tsx`, `src/components/Board.tsx` | Scenario S12 + DB assert |
| [ ] | Vim command palette `:` / `:move <status>` with Tab autocomplete | `src/components/Board.tsx` | Behavior scenario + unit test |
| [ ] | Vim `n` opens create task modal when enabled | `src/components/Board.tsx`, `src/desktop/main/services/appSettingsService.ts` | Behavior scenario + unit test |
| [ ] | Vim `dd` task deletion sequence | `src/components/Board.tsx` | Behavior scenario + DB assert |
| [ ] | Board command provider opens notification center, project filter, create task, quick search | `src/desktop/renderer/components/BoardCommandProvider.tsx` | Unit/integration test |

## Modals And Popovers

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | CreateTask modal focus trap, Escape close, Enter submit | `src/components/CreateTaskModal.tsx` | Screenshot comparison + scenario S01 + unit test |
| [ ] | CreateTask project selector excludes worktree projects | `src/components/CreateTaskModal.tsx` | Unit test + DB assert |
| [ ] | CreateTask base branch autocomplete and default branch handling | `src/components/CreateTaskModal.tsx`, `src/components/BranchSearchInput.tsx` | Scenario S01/S06 + unit test |
| [ ] | CreateTask optional description, session type, ssh host, priority | `src/components/CreateTaskModal.tsx`, `src/components/PrioritySelector.tsx` | Scenario S01 + DB assert |
| [ ] | CreateTask navigates to task detail after creation | `src/components/CreateTaskModal.tsx` | Scenario S01 |
| [ ] | BranchTask modal project/base/new branch/session flow | `src/components/BranchTaskModal.tsx` | Screenshot comparison + scenario S06 + DB assert |
| [ ] | ProjectBranchTasks modal grouped project tasks | `src/components/ProjectBranchTasksModal.tsx` | Screenshot comparison + behavior scenario |
| [ ] | DoneConfirm dialog with "don't ask again" persistence | `src/components/DoneConfirmDialog.tsx`, `src/desktop/main/services/appSettingsService.ts` | Screenshot comparison + scenario S09 + DB assert |
| [ ] | HooksStatusCard task panel for Claude/Gemini/Codex/OpenCode | `src/components/HooksStatusCard.tsx` | Screenshot comparison + unit/integration test |
| [ ] | HooksStatusDialog manual/install/reinstall states | `src/components/HooksStatusDialog.tsx` | Screenshot comparison + behavior scenario |
| [ ] | ProjectSettings modal variant | `src/components/ProjectSettings.tsx` | Screenshot comparison + DB assert |
| [ ] | ProjectSettings page variant with side navigation | `src/desktop/renderer/routes/SettingsRoute.tsx`, `src/components/ProjectSettings.tsx` | Screenshot comparison + scenario S10 |
| [ ] | Notification center popover empty, unread, keyboard, mark-all-read, activation | `src/components/NotificationCenterButton.tsx` | Screenshot comparison + scenario S11 |
| [ ] | Notification missing-task confirmation modal | `src/components/NotificationCenterButton.tsx` | Behavior scenario + screenshot comparison |
| [ ] | Background sync review dialog merged PR selection and update-to-done action | `src/desktop/renderer/components/BackgroundSyncReviewDialog.tsx` | Screenshot comparison + DB assert |
| [ ] | Board event alert for hook install failure | `src/desktop/renderer/components/BoardEventAlert.tsx` | Screenshot comparison + behavior scenario |
| [ ] | Release update dialog with sanitized markdown and dismiss-version checkbox | `src/desktop/renderer/components/ReleaseUpdateDialog.tsx` | Screenshot comparison + unit/integration test |
| [ ] | Task quick search dialog and task navigation | `src/desktop/renderer/components/TaskQuickSearchDialog.tsx` | Screenshot comparison + behavior scenario + unit test |

## Task Detail Screen

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Task-not-found state | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + behavior scenario |
| [ ] | Detail dock layout with back-to-board button | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison |
| [ ] | Dock item 1 overview panel | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/components/TaskDetailTitleCard.tsx`, `src/components/TaskDetailInfoCard.tsx` | Screenshot comparison + scenario S04 |
| [ ] | Dock item 2 status/hooks panel | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/components/HooksStatusCard.tsx` | Screenshot comparison + scenario S04 |
| [ ] | Dock item 3 AI chat view | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + scenario S04 |
| [ ] | Conditional PR dock slot inserted at 4 when PR URL exists | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Scenario S04 + unit test |
| [ ] | Dock shortcut labels derive from item array order | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/desktop/shared/keyboardShortcut.ts` | Unit test + scenario S04 |
| [ ] | Dock shortcut capture happens before terminal input | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Unit test + scenario S04 |
| [ ] | Page back/forward shortcuts capture before terminal input | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Unit test |
| [ ] | Overview panel title/description editing | `src/components/TaskDetailTitleCard.tsx` | Screenshot comparison + DB assert |
| [ ] | Overview panel status badge, branch/base, diff link, priority, project color | `src/components/TaskDetailInfoCard.tsx`, `src/components/PriorityEditor.tsx`, `src/components/ProjectColorEditor.tsx` | Screenshot comparison + DB assert |
| [ ] | Status panel status transitions and delete action | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Behavior scenario + DB assert |
| [ ] | Terminal active view for tasks with session | `src/desktop/renderer/components/TerminalLoader.tsx`, `src/desktop/renderer/components/Terminal.tsx` | Screenshot comparison + scenario S03 |
| [ ] | Connect terminal form for project tasks without session | `src/components/ConnectTerminalForm.tsx` | Screenshot comparison + behavior scenario |
| [ ] | No-terminal empty state for tasks without project | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison |
| [ ] | AI session list, provider filters, search, role filters, pagination | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/lib/aiSessions/**` | Screenshot comparison + behavior scenario |
| [ ] | AI session message rendering and scroll to latest | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + scenario |
| [ ] | Sidebar hint/default overview panel display | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/components/CollapsibleSidebar.tsx` | Screenshot comparison + scenario S14 |
| [ ] | Sidebar hint dismissal persists through `AppSettings` | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/desktop/main/services/appSettingsService.ts` | Scenario S14 + DB assert |
| [ ] | Task detail route cache normalization strips legacy global settings | `src/desktop/renderer/routes/TaskDetailRoute.tsx`, `src/desktop/renderer/utils/routeCache.ts` | Unit test |
| [ ] | Same task re-entry focuses existing window instead of opening duplicate | `src/desktop/renderer/utils/taskNavigation.ts`, `electron/main.js` | Scenario S13 + unit test |

## Diff Screen

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Diff route loading, task-not-found, no-branch, no-worktree states | `src/desktop/renderer/routes/DiffRoute.tsx` | Screenshot comparison + behavior scenario |
| [ ] | Diff header with base/branch chips and file count | `src/desktop/renderer/routes/DiffRoute.tsx` | Screenshot comparison |
| [ ] | File tree with folders, status labels, change dots, viewed state | `src/components/DiffFileTree.tsx` | Screenshot comparison + unit test |
| [ ] | Resizable file tree sidebar | `src/desktop/renderer/components/DiffPageClient.tsx` | Behavior scenario |
| [ ] | Diff mode using Monaco side-by-side viewer | `src/components/DiffMonacoViewer.tsx` | Screenshot comparison + scenario S05 |
| [ ] | Edit mode using Monaco editor, save button states, Cmd/Ctrl+S | `src/components/DiffFileEditor.tsx` | Scenario S05 + unit/integration test |
| [ ] | Deleted file hides edit mode | `src/desktop/renderer/components/DiffPageClient.tsx` | Screenshot comparison + unit test |
| [ ] | File content APIs: original, modified, save | `src/desktop/main/services/diffService.ts` | Scenario S05 + integration test |

## Settings And Pane Layout

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Appearance theme preference: system/dark/light | `src/components/ProjectSettings.tsx`, `src/desktop/renderer/utils/theme.ts` | Screenshot comparison + scenario S10 + DB assert |
| [ ] | Detail sidebar default collapsed setting | `src/components/ProjectSettings.tsx`, `src/desktop/main/services/appSettingsService.ts` | DB assert + unit test |
| [ ] | Default session type setting: tmux/zellij | `src/components/ProjectSettings.tsx`, `src/desktop/main/services/appSettingsService.ts` | DB assert + unit test |
| [ ] | Notification enable toggle and status filters | `src/components/ProjectSettings.tsx`, `src/desktop/main/services/appSettingsService.ts` | Screenshot comparison + DB assert |
| [ ] | Background sync enabled and interval validation (1-1440 minutes) | `src/components/ProjectSettings.tsx`, `src/desktop/main/services/backgroundTaskSyncService.ts` | DB assert + unit test |
| [ ] | Vim mode toggle | `src/components/ProjectSettings.tsx` | DB assert + unit test |
| [ ] | Scan and register local/remote projects | `src/components/ProjectSettings.tsx`, `src/desktop/main/services/projectService.ts` | Behavior scenario + DB assert |
| [ ] | Registered project list and deletion | `src/components/ProjectSettings.tsx` | Screenshot comparison + DB assert |
| [ ] | Pane layout global default editor | `src/desktop/renderer/routes/PaneLayoutRoute.tsx`, `src/components/PaneLayoutEditor.tsx` | Screenshot comparison + DB assert |
| [ ] | Pane layout project override editor | `src/desktop/renderer/routes/PaneLayoutRoute.tsx`, `src/components/PaneLayoutEditor.tsx` | Screenshot comparison + DB assert |
| [ ] | Pane layout types: single, horizontal_2, vertical_2, left_right_tb, left_tb_right, quad | `src/entities/PaneLayoutConfig.ts`, `src/components/PaneLayoutEditor.tsx` | Unit/integration test + DB assert |
| [ ] | Pane command persistence and reset-to-global | `src/components/PaneLayoutEditor.tsx`, `src/desktop/main/services/paneLayoutService.ts` | DB assert + behavior scenario |

## Shortcuts And Keyboard Policies

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | `Mod+Shift+O` default task search shortcut and custom capture | `src/desktop/shared/keyboardShortcut.ts`, `src/desktop/main/services/appSettingsService.ts` | Unit test + behavior scenario |
| [ ] | `Mod+Shift+I` notification center shortcut | `src/desktop/shared/keyboardShortcut.ts`, `electron/main.js` | Unit test + behavior scenario |
| [ ] | `Mod+Shift+P` project filter shortcut | `src/desktop/shared/keyboardShortcut.ts`, `src/desktop/renderer/components/BoardCommandProvider.tsx` | Unit test + behavior scenario |
| [ ] | `Mod+N` create task shortcut | `src/desktop/shared/keyboardShortcut.ts`, `electron/main.js` | Unit test + behavior scenario |
| [ ] | `Mod+Shift+N` new window shortcut | `src/desktop/shared/keyboardShortcut.ts`, `electron/main.js` | Unit test + behavior scenario |
| [ ] | `Mod+Shift+[` and `Mod+Shift+]` page history shortcuts | `src/desktop/shared/keyboardShortcut.ts`, `src/desktop/renderer/navigation.tsx` | Unit test + behavior scenario |
| [ ] | `Mod+F` board page find shortcut | `src/desktop/shared/keyboardShortcut.ts`, `src/components/BoardPageFindBar.tsx` | Unit test + scenario S07 |
| [ ] | `Mod+R` reload blocked in desktop app | `src/desktop/shared/keyboardShortcut.ts`, `electron/main.js` | Unit test |
| [ ] | Task detail dock shortcuts: macOS Cmd+1..9, Linux Alt+1..9 | `src/desktop/shared/keyboardShortcut.ts`, `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Unit test + scenario S04 |
| [ ] | Shortcut blocker registration prevents modal/terminal conflicts | `src/desktop/renderer/components/BoardCommandProvider.tsx`, modal/dialog files | Unit/integration test |
| [ ] | Keyboard focus is restored to active terminal after UI closes | `src/desktop/renderer/utils/terminalFocus.ts`, task detail/popover files | Unit/integration test + scenario S03 |

## IPC, Preload, And Window Management

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Generic desktop invoke bridge `kanvibe:invoke(namespace, method, args)` | `electron/preload.js`, `electron/main.js`, `src/desktop/main/serviceRegistry.ts` | Integration test |
| [ ] | Invalid namespace/method is rejected without crashing | `electron/main.js`, `src/desktop/main/serviceRegistry.ts` | Integration test |
| [ ] | Focus existing internal route bridge | `electron/preload.js`, `electron/main.js`, `src/desktop/main/windowOpen.ts` | Scenario S13 + unit test |
| [ ] | Terminal open/write/resize/focus/close IPC | `electron/preload.js`, `electron/main.js`, `src/desktop/main/terminalBridge.ts` | Scenario S03 + integration test |
| [ ] | Terminal data/close events to renderer | `electron/preload.js`, `src/desktop/renderer/components/Terminal.tsx` | Scenario S03 |
| [ ] | Board event broadcast to renderer windows | `electron/main.js`, `src/lib/boardNotifier.ts` | Scenario S11 + integration test |
| [ ] | Desktop notification show/list/mark-read/mark-all/activate/consume activation | `electron/preload.js`, `electron/main.js`, `src/desktop/main/notificationStore.ts` | Scenario S11 + integration test |
| [ ] | Notification changed/activated/shortcut events | `electron/preload.js`, `electron/main.js` | Scenario S11 |
| [ ] | Create-task and task-detail-dock shortcut events from main to renderer | `electron/main.js`, `electron/preload.js` | Unit test + scenario S04 |
| [ ] | Renderer log IPC for preload/window errors | `electron/preload.js`, `electron/main.js` | Unit/integration test |
| [ ] | Window open policy for internal routes, external links, child windows | `electron/main.js`, `src/desktop/main/windowOpen.ts`, `src/desktop/renderer/utils/windowOpen.ts` | Unit test + behavior scenario |
| [ ] | App lifecycle: activate, before-quit, window-all-closed, diagnostics | `electron/main.js`, `electron/diagnostics.js` | Unit/integration test + artifact inspection |

## Service Contracts

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Kanban load by status and done pagination | `src/desktop/main/services/kanbanService.ts` | DB assert + unit/integration test |
| [ ] | Kanban searchable task index | `src/desktop/main/services/kanbanService.ts` | Unit/integration test + behavior scenario |
| [ ] | Create task with title, description, branch, base branch, project, session, ssh host, agent, priority | `src/desktop/main/services/kanbanService.ts` | Scenario S01 + DB assert |
| [ ] | Update task status with notifications and persisted task state | `src/desktop/main/services/kanbanService.ts`, `src/desktop/main/services/kanvibeTaskStateService.ts` | Scenario S02/S09/S11 + DB assert |
| [ ] | Update task title/description/priority | `src/desktop/main/services/kanbanService.ts` | DB assert + unit test |
| [ ] | Update project color from task card/detail | `src/desktop/main/services/kanbanService.ts` | Scenario S08 + DB assert |
| [ ] | Delete task resource cleanup and board removal | `src/desktop/main/services/kanbanService.ts`, `src/lib/worktree.ts` | Scenario S12 + DB assert + unit test |
| [ ] | Branch from task creates worktree/session and DB task | `src/desktop/main/services/kanbanService.ts`, `src/lib/worktree.ts` | Scenario S06 + DB assert + integration test |
| [ ] | Connect terminal session to existing task | `src/desktop/main/services/kanbanService.ts`, `src/lib/worktree.ts` | Scenario S03 + DB assert |
| [ ] | Reorder tasks and move task to column preserve display order | `src/desktop/main/services/kanbanService.ts` | Scenario S02 + DB assert |
| [ ] | Fetch and save PR URL via GitHub CLI prompt/dependency | `src/desktop/main/services/kanbanService.ts`, `src/lib/githubCliDependency.ts` | Behavior scenario + unit/integration test |
| [ ] | Active task PR merge sync and task pull sync | `src/desktop/main/services/kanbanService.ts`, `src/desktop/main/services/backgroundTaskSyncService.ts` | Unit/integration test + notification scenario |
| [ ] | Project CRUD, scan, worktree sync, branch listing, subdirectories | `src/desktop/main/services/projectService.ts` | DB assert + integration test |
| [ ] | Hook service start and update task status from hook server | `src/desktop/main/services/hookService.ts`, `electron/hookServer.js` | Integration test + behavior scenario |
| [ ] | Diff service files/original/content/save | `src/desktop/main/services/diffService.ts` | Scenario S05 + integration test |
| [ ] | Pane layout service global/project/effective/save/delete | `src/desktop/main/services/paneLayoutService.ts` | DB assert + unit/integration test |
| [ ] | App settings service get/set typed settings | `src/desktop/main/services/appSettingsService.ts` | DB assert + unit test |
| [ ] | GitHub CLI dependency get/install local/remote | `src/desktop/main/services/githubCliDependencyService.ts`, `src/lib/githubCliDependency.ts` | Unit/integration test |
| [ ] | Session dependency get/install for tmux/zellij local/remote | `src/desktop/main/services/sessionDependencyService.ts`, `src/lib/remoteSessionDependency.ts` | Unit/integration test |
| [ ] | Release update check/claim/dismiss flow | `src/desktop/main/services/releaseUpdateService.ts` | Unit/integration test |
| [ ] | Desktop notification delivery and board event notification conversion | `src/desktop/main/services/desktopNotificationService.ts`, `src/desktop/shared/taskNotifications.ts` | Unit/integration test |
| [ ] | Background task sync single-loop and interval behavior | `src/desktop/main/services/backgroundTaskSyncService.ts` | Unit/integration test |

## Database And Persistence

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Runtime DB path rules: `KANVIBE_APP_DATA_DIR`, `KANVIBE_DB_PATH`, default `.kanvibe/kanvibe.db` | `src/lib/databasePaths.ts` | Unit test + artifact inspection |
| [ ] | Bundled seed DB path rules: `KANVIBE_SEED_DB_PATH`, `resources/database/app.seed.db` | `src/lib/databasePaths.ts`, `scripts/build-seed-db.ts` | Artifact inspection + DB assert |
| [ ] | Existing SQLite DB readiness without schema-breaking migration | `src/lib/sqliteSchema.ts`, `src/lib/database.ts` | DB assert + integration test |
| [ ] | TypeORM migration baseline records for pre-existing DB | `src/lib/database.ts`, `src/migrations/*.ts` | DB assert + integration test |
| [ ] | `projects` table: id, name, repo_path, default_branch, ssh_host, is_worktree, color, created_at | `src/entities/Project.ts`, `src/lib/sqliteSchema.ts` | DB assert + schema snapshot |
| [ ] | `kanban_tasks` table: id, title, description, status, branch_name, worktree_path, session_type, session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority, display_order, created_at, updated_at | `src/entities/KanbanTask.ts`, `src/lib/sqliteSchema.ts` | DB assert + schema snapshot |
| [ ] | `pane_layout_configs` table: id, layout_type, panes, project_id, is_global, created_at, updated_at | `src/entities/PaneLayoutConfig.ts`, `src/lib/sqliteSchema.ts` | DB assert + schema snapshot |
| [ ] | `app_settings` table: id, key, value, created_at, updated_at | `src/entities/AppSettings.ts`, `src/lib/sqliteSchema.ts` | DB assert + schema snapshot |
| [ ] | SQLite indexes: `idx_kanban_tasks_status_order`, `idx_kanban_tasks_project_branch` | `src/lib/sqliteSchema.ts` | DB assert |
| [ ] | App setting `sidebar_default_collapsed` | `src/desktop/main/services/appSettingsService.ts` | DB assert |
| [ ] | App setting `sidebar_hint_dismissed` | `src/desktop/main/services/appSettingsService.ts` | Scenario S14 + DB assert |
| [ ] | App setting `done_alert_dismissed` | `src/desktop/main/services/appSettingsService.ts` | Scenario S09 + DB assert |
| [ ] | App setting `release_update_dismissed_versions` | `src/desktop/main/services/appSettingsService.ts` | Unit/integration test + DB assert |
| [ ] | App setting `notification_enabled` and `notification_statuses` | `src/desktop/main/services/appSettingsService.ts` | Scenario S11 + DB assert |
| [ ] | App setting `background_sync_enabled` and `background_sync_interval_ms` | `src/desktop/main/services/appSettingsService.ts` | Unit/integration test + DB assert |
| [ ] | App setting `default_session_type` | `src/desktop/main/services/appSettingsService.ts` | DB assert |
| [ ] | App setting `task_search_shortcut` | `src/desktop/main/services/appSettingsService.ts` | Unit test + DB assert |
| [ ] | App setting `vim_mode_enabled` | `src/desktop/main/services/appSettingsService.ts` | Unit test + DB assert |
| [ ] | App setting `theme_preference` | `src/desktop/renderer/actions/appSettings.ts` | Scenario S10 + DB assert |
| [ ] | Route cache remains route-local and does not persist global settings | `src/desktop/renderer/utils/routeCache.ts`, route files | Unit test |

## Git, Worktrees, PTY, And Remote Sessions

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Local shell environment strips generic and internal runtime variables | `src/lib/shellEnvironment.ts` | Unit test |
| [ ] | Local terminal session attach/write/resize/focus/detach | `src/lib/terminal.ts`, `src/desktop/main/terminalBridge.ts` | Scenario S03 + integration test |
| [ ] | Remote terminal session over SSH/tmux | `src/lib/terminal.ts`, `src/lib/remoteSessionDependency.ts`, `src/lib/sshConfig.ts` | Integration test + behavior scenario |
| [ ] | SSH config parsing and ControlMaster reuse options | `src/lib/sshConfig.ts`, `src/lib/gitOperations.ts` | Unit/integration test |
| [ ] | Git command execution local/remote and SSH transport error classification | `src/lib/gitOperations.ts` | Unit/integration test |
| [ ] | Branch list/default branch/remote branch existence | `src/lib/gitOperations.ts` | Unit/integration test |
| [ ] | Repository scan and validation | `src/lib/gitOperations.ts`, `src/desktop/main/services/projectService.ts` | Integration test |
| [ ] | Worktree path/session name generation | `src/lib/worktree.ts` | Unit test |
| [ ] | Tmux pane layout command generation | `src/lib/worktree.ts`, `src/entities/PaneLayoutConfig.ts` | Unit test |
| [ ] | Zellij layout KDL generation | `src/lib/worktree.ts` | Unit test |
| [ ] | Create worktree with session and create session without worktree | `src/lib/worktree.ts` | Scenario S06 + integration test |
| [ ] | Remove worktree/branch and remove session only | `src/lib/worktree.ts` | Scenario S12 + integration test |
| [ ] | Active session list and liveness check | `src/lib/worktree.ts` | Integration test |
| [ ] | Git exclude hook pattern management | `src/lib/gitExclude.ts` | Unit/integration test |
| [ ] | Host file access local/remote reads/writes/listing | `src/lib/hostFileAccess.ts` | Unit/integration test |

## AI Sessions And Hooks

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | AI session aggregate ordering, provider source status, pagination | `src/lib/aiSessions/aggregateAiSessions.ts`, `src/lib/aiSessions/shared.ts` | Unit/integration test + screenshot comparison |
| [ ] | Claude session summary/detail reader for JSONL project files | `src/lib/aiSessions/readClaudeSessions.ts` | Unit/integration test + scenario |
| [ ] | Codex session summary/detail reader for rollout JSONL files | `src/lib/aiSessions/readCodexSessions.ts` | Unit/integration test + scenario |
| [ ] | Gemini session summary/detail reader for chat JSON files | `src/lib/aiSessions/readGeminiSessions.ts` | Unit/integration test + scenario |
| [ ] | OpenCode session summary/detail reader for SQLite DB | `src/lib/aiSessions/readOpenCodeSessions.ts` | Unit/integration test + scenario |
| [ ] | AI session query filters and role filters | `src/lib/aiSessions/**`, `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + unit/integration test |
| [ ] | Hook server endpoint `/status` and hook task update endpoints | `electron/hookServer.js`, `src/lib/hookEndpoint.ts`, `src/desktop/main/services/hookService.ts` | Integration test |
| [ ] | Local and remote hook server URL validation | `src/lib/hookServerStatus.ts`, `src/lib/hookEndpoint.ts` | Unit/integration test |
| [ ] | Claude hooks status/install for project and task | `src/lib/claudeHooksSetup.ts`, `src/desktop/main/services/projectService.ts` | Unit/integration test + screenshot comparison |
| [ ] | Gemini hooks status/install for project and task | `src/lib/geminiHooksSetup.ts`, `src/desktop/main/services/projectService.ts` | Unit/integration test + screenshot comparison |
| [ ] | Codex hooks status/install for project and task | `src/lib/codexHooksSetup.ts`, `src/desktop/main/services/projectService.ts` | Unit/integration test + screenshot comparison |
| [ ] | OpenCode plugin status/install for project and task | `src/lib/openCodeHooksSetup.ts`, `src/lib/openCodePluginRegistry.ts`, `src/desktop/main/services/projectService.ts` | Unit/integration test + screenshot comparison |
| [ ] | Hook task binding and persisted `.kanvibe` task state | `src/lib/hookTaskBinding.ts`, `src/desktop/main/services/kanvibeTaskStateService.ts` | Unit/integration test + DB/file assert |
| [ ] | Hook installation failure board alert and notification | `src/lib/boardNotifier.ts`, `src/desktop/renderer/components/BoardEventAlert.tsx` | Scenario S11 + screenshot comparison |

## Notifications And Background Sync

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Board event `board-updated` refresh behavior | `src/lib/boardNotifier.ts`, `src/desktop/renderer/utils/refresh.ts` | Behavior scenario + integration test |
| [ ] | Board event `task-status-changed` notification | `src/lib/boardNotifier.ts`, `src/desktop/shared/taskNotifications.ts` | Scenario S11 + unit/integration test |
| [ ] | Board event `hook-status-target-missing` notification | `src/lib/boardNotifier.ts`, `src/desktop/shared/taskNotifications.ts` | Unit/integration test + screenshot comparison |
| [ ] | Board event `task-hook-install-failed` alert | `src/lib/boardNotifier.ts`, `src/desktop/renderer/components/BoardEventAlert.tsx` | Screenshot comparison + behavior scenario |
| [ ] | Board event `task-pr-merged-detected` and batch | `src/lib/boardNotifier.ts`, `src/desktop/main/services/backgroundTaskSyncService.ts` | Unit/integration test + notification scenario |
| [ ] | Board event `background-sync-review-needed` action payload | `src/lib/boardNotifier.ts`, `src/desktop/shared/notifications.ts` | Unit/integration test + screenshot comparison |
| [ ] | Notification localization ko/en/zh fallback | `src/desktop/shared/taskNotifications.ts`, `messages/*.json` | Unit test |
| [ ] | Notification center activation routes to current window or new window with Shift | `src/components/NotificationCenterButton.tsx` | Scenario S11/S13 + unit test |
| [ ] | Background sync worktree registration, PR merge detection, pull sync, failure aggregation | `src/desktop/main/services/backgroundTaskSyncService.ts`, `src/desktop/main/services/projectService.ts`, `src/desktop/main/services/kanbanService.ts` | Unit/integration test + behavior scenario |

## Theme, Styling, And i18n

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Semantic primary token `#0064FF` usage for actions/selection/focus/link | `src/styles/globals.css`, `CLAUDE.md` | Screenshot comparison + unit/static test |
| [ ] | Semantic neutral button surface `#202632` usage | `src/styles/globals.css`, `CLAUDE.md` | Screenshot comparison + unit/static test |
| [ ] | Status colors remain separate from primary token | `src/styles/globals.css`, components | Screenshot comparison + unit/static test |
| [ ] | Project color presets and user-selected project colors | `src/lib/projectColor.ts`, `src/components/ProjectColorEditor.tsx` | Screenshot comparison + scenario S08 |
| [ ] | Light/dark/system theme application | `src/desktop/renderer/utils/theme.ts`, `src/components/ProjectSettings.tsx` | Screenshot comparison + scenario S10 |
| [ ] | Korean locale catalog | `messages/ko.json` | Screenshot comparison + scenario S10 |
| [ ] | English locale catalog | `messages/en.json` | Screenshot comparison + scenario S10 |
| [ ] | Existing Chinese locale catalog recorded for follow-up parity decision | `messages/zh.json` | Artifact inspection |
| [ ] | AI provider icons for Claude/Codex/Gemini/OpenCode | `src/components/AiProviderIcon.tsx` | Screenshot comparison |
| [ ] | Monaco diff/editor replacement target documented for native | `src/components/DiffMonacoViewer.tsx`, `src/components/DiffFileEditor.tsx` | Artifact inspection + future native QA |
| [ ] | xterm.js terminal replacement target documented for native | `src/components/Terminal.tsx`, `src/desktop/renderer/components/Terminal.tsx` | Artifact inspection + future native QA |

## Existing QA Assets

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Electron launcher with dynamic CDP port and QA env | `qa/electron/lib/launchElectron.cjs` | Unit test + artifact inspection |
| [ ] | QA report writer | `qa/electron/lib/report.cjs` | Artifact inspection |
| [ ] | Fixture repository clone/worktree helper | `qa/electron/lib/fixtureRepository.cjs` | Unit test + scenario S06 |
| [ ] | Electron smoke QA flow | `qa/electron/flows/smoke.cjs`, `scripts/qa-electron.sh` | Artifact inspection + future baseline command |
| [ ] | Move autocomplete video QA flow | `qa/electron/flows/move-autocomplete.cjs`, `scripts/qa-move-autocomplete-video.sh` | Artifact inspection + future scenario S02/S07 |
| [ ] | AI session history video QA flow | `qa/electron/flows/ai-session-history.cjs`, `scripts/qa-ai-session-history-video.sh` | Artifact inspection + future scenario |
| [ ] | Existing PR #275/#276 regression QA script | `scripts/qa-pr275-pr276.sh` | Artifact inspection + future regression gate |

## Phase 2 Baseline Capture Requirements

| Done | Item | Source | Verification method |
| --- | --- | --- | --- |
| [ ] | Seed DB has 3 projects with different colors | Pasted objective, `src/entities/Project.ts` | DB assert |
| [ ] | Seed DB has 12+ tasks across todo/progress/pending/review/done | Pasted objective, `src/entities/KanbanTask.ts` | DB assert |
| [ ] | Seed DB covers priority, branch, base branch, worktree, PR URL, session, remote/local combinations | Pasted objective | DB assert |
| [ ] | Seed DB includes AI session history fixtures | Pasted objective, `src/lib/aiSessions/**` | Artifact inspection + behavior scenario |
| [ ] | Board baseline: default | Pasted objective, board sources | Screenshot comparison |
| [ ] | Board baseline: sidebar collapsed/expanded if applicable | Pasted objective, board/task-detail sources | Screenshot comparison + manifest note |
| [ ] | Board baseline: find bar open | `src/components/BoardPageFindBar.tsx` | Screenshot comparison |
| [ ] | Board baseline: task card hover | `src/components/TaskCard.tsx` | Screenshot comparison |
| [ ] | Board baseline: context menu open | `src/components/TaskContextMenu.tsx` | Screenshot comparison |
| [ ] | Baseline captures every modal: CreateTask, BranchTask, ProjectBranchTasks, DoneConfirm, HooksStatusDialog, ProjectSettings, notification center | Pasted objective, component sources | Screenshot comparison + manifest |
| [ ] | Task detail baseline: terminal active | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison |
| [ ] | Task detail baseline: AI session history | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison |
| [ ] | Task detail baseline: sidebar hint shown/dismissed | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison + DB assert |
| [ ] | Task detail baseline: dock with PR and without PR | `src/desktop/renderer/routes/TaskDetailRoute.tsx` | Screenshot comparison |
| [ ] | Diff baseline: file tree, viewer, editor | `src/desktop/renderer/routes/DiffRoute.tsx`, diff components | Screenshot comparison |
| [ ] | Settings baseline split by scroll sections | `src/components/ProjectSettings.tsx` | Screenshot comparison |
| [ ] | Pane layout editor baseline | `src/desktop/renderer/routes/PaneLayoutRoute.tsx` | Screenshot comparison |
| [ ] | NotFound baseline | `src/desktop/renderer/routes/NotFoundRoute.tsx` | Screenshot comparison |
| [ ] | Baseline for `ko` and `en` locales | `messages/ko.json`, `messages/en.json` | Screenshot comparison + manifest |
| [ ] | Baseline for light/dark themes if Electron supports them | `src/desktop/renderer/utils/theme.ts` | Screenshot comparison + manifest |

## Phase 5 Scenario Requirements

| Done | Scenario | Required proof |
| --- | --- | --- |
| [ ] | S01 task create -> card appears | Behavior scenario + DB assert |
| [ ] | S02 card drag/drop column move -> status changes | Behavior scenario + DB assert |
| [ ] | S03 task detail -> terminal `echo kanvibe-qa` output | Behavior scenario + video |
| [ ] | S04 Cmd+1..4 dock pane switch and PR slot 4 | Behavior scenario + screenshot/video |
| [ ] | S05 Diff select file -> render -> edit -> save | Behavior scenario + file assert |
| [ ] | S06 branch task creation with temp git repo/worktree | Behavior scenario + DB/git assert |
| [ ] | S07 find bar search -> highlight | Behavior scenario + screenshot |
| [ ] | S08 priority and project color changes reflect on cards | Behavior scenario + DB assert + screenshot |
| [ ] | S09 done confirmation -> done column | Behavior scenario + DB assert |
| [ ] | S10 language switch ko/en -> UI strings replace | Behavior scenario + screenshot |
| [ ] | S11 notification generated and accumulated | Behavior scenario + screenshot + notification store assert |
| [ ] | S12 task delete -> card removed and DB row removed | Behavior scenario + DB assert |
| [ ] | S13 same task re-entry focuses existing window | Behavior scenario + window count/focus assert |
| [ ] | S14 sidebar hint dismissal survives restart | Behavior scenario + DB assert |
