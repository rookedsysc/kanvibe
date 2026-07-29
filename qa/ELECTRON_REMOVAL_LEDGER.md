# Electron product removal ledger

This ledger maps every generic desktop service export and direct Electron IPC
channel to its Rust owner and current GPUI consumer status. It is generated
from the current `src/desktop/main/serviceRegistry.ts` boundary, not from the
older PR #309 snapshot.

## Status vocabulary

- `CONNECTED`: the Rust implementation is called by the production GPUI app.
- `SERVICE`: the Rust implementation exists, but at least one matching GPUI
  surface or real macOS behavior gate is still incomplete.
- `RUNTIME`: production wiring exists, but the required packaged macOS evidence
  has not passed.
- `BLOCKED`: the replacement is intentionally incomplete and Electron removal
  is forbidden.

No row authorizes deletion by itself. Electron removal requires every row to be
`CONNECTED`, the matching inventory/Phase 5 evidence to pass, and the release
and stabilization gates in Issue #310 to pass.

## Generic service registry

| Electron namespace | Electron service methods | Rust owner / entry points | GPUI consumer | Status |
| --- | --- | --- | --- | --- |
| `appSettings` | `getAppSetting`, `setAppSetting`, `getSidebarDefaultCollapsed`, `setSidebarDefaultCollapsed`, `getSidebarHintDismissed`, `dismissSidebarHint`, `getDoneAlertDismissed`, `dismissDoneAlert`, `getReleaseUpdateDismissedVersions`, `dismissReleaseUpdateVersion`, `getNotificationSettings`, `setNotificationEnabled`, `setNotificationStatuses`, `registerBackgroundSyncIntervalChangedCallback`, `registerBackgroundSyncEnabledChangedCallback`, `getBackgroundSyncEnabled`, `setBackgroundSyncEnabled`, `getBackgroundSyncIntervalMs`, `setBackgroundSyncIntervalMs`, `getDefaultSessionType`, `setDefaultSessionType`, `getTaskSearchShortcut`, `setTaskSearchShortcut`, `getVimModeEnabled`, `setVimModeEnabled` | `kanvibe-core::KanvibeDb` typed settings; `kanvibe-app::{build_settings_shell, update_native_settings, dismiss_native_sidebar_hint, dismiss_native_release_update}`; process-owned background sync reconfiguration | Settings route, collapsible Task Detail sidebar/hint, persistent Done/release dialogs, notification controls, background service | `RUNTIME` — storage and production consumers are connected; packaged sidebar, Done, notification/status, and release evidence remains |
| `diff` | `getGitDiffFiles`, `getOriginalFileContent`, `getFileContent`, `saveFileContent` | `kanvibe-git::{changed_files, original_file_content, file_content, save_file_content_if_unchanged}`; remote equivalents; `kanvibe-app::{load_native_diff_snapshot, save_native_diff_file}` | Diff route worker/cache/editor | `RUNTIME` — folder/viewed/sidebar, original/current side-by-side, save/conflict, deleted/binary safety are connected; real packaged S05 edit evidence remains |
| `githubCliDependency` | `getGitHubCliStatus`, `installGitHubCli` | `kanvibe-git::{github_cli_available, install_github_cli}` and bounded remote transport; `kanvibe-app::{get_native_github_cli_status, install_native_github_cli}` | Settings dependency controls | `CONNECTED` (real local/SSH installation remains a macOS Phase 5 failure-path gate) |
| `hooks` | `startHookTask`, `updateHookTaskStatus` | `kanvibe-app::{spawn_native_hook_server_with_notifications, handle_native_hook_request}`; `kanvibe-hooks` provider render/install and notifications | Process hook server, board refresh, notification center | `CONNECTED` |
| `kanban` | `installTaskHooksImmediately`, `installTaskHookFilesImmediately`, `getTasksByStatus`, `getMoreDoneTasks`, `getTaskById`, `getSearchableTasks`, `getTaskIdByProjectAndBranch`, `createTask`, `updateTaskStatus`, `updateTask`, `updateProjectColor`, `deleteTask`, `deleteTasks`, `branchFromTask`, `connectTerminalSession`, `reorderTasks`, `moveTaskToColumn`, `fetchAndSavePrUrl`, `syncActiveTaskPullRequests`, `syncActiveTaskPulls` | `kanvibe-core::KanvibeDb`; `kanvibe-app` board/task/form/status/move/delete/branch/connect/project-color/background functions; `kanvibe-git` PR and worktree functions; `kanvibe-hooks` installers | Board, task detail, branch/create forms, background review | `RUNTIME` — CRUD/reorder/status/branch/connect/project-color/sync and production consumers are connected; full pointer/keyboard/Done packaged evidence remains |
| `paneLayout` | `getGlobalPaneLayout`, `getProjectPaneLayout`, `getEffectivePaneLayout`, `getAllPaneLayouts`, `savePaneLayout`, `deletePaneLayout` | `kanvibe-core::KanvibeDb` pane layout APIs; `kanvibe-app::{save_native_pane_layout_type, reset_native_project_pane_layout, update_native_pane_command}` | Pane-layout route with six direct layout choices, missing-project override creation, labeled command editors, and reset | `RUNTIME` — editor/service parity is connected; real packaged S10 pointer/keyboard and terminal-layout evidence remain |
| `project` | `getAllProjects`, `getAvailableHosts`, `getProjectById`, `registerProject`, `deleteProject`, `syncRegisteredProjectWorktrees`, `scanAndRegisterProjects`, `listSubdirectories`, `getProjectBranches`, `getProjectHooksStatus`, `installProjectHooks`, `getTaskHooksStatus`, `installTaskHooks`, `getProjectGeminiHooksStatus`, `installProjectGeminiHooks`, `getTaskGeminiHooksStatus`, `installTaskGeminiHooks`, `getProjectCodexHooksStatus`, `installProjectCodexHooks`, `getTaskCodexHooksStatus`, `installTaskCodexHooks`, `installProjectOpenCodeHooks`, `installTaskOpenCodeHooks`, `getTaskOpenCodeHooksStatus`, `getTaskAiSessions`, `getTaskAiSessionDetail` | `kanvibe-app` local/remote register, scan, delete, sync, four-provider hook status/install and AI orchestration; `kanvibe-git` local/SSH repository/worktree and typed callback-health boundary; `kanvibe-hooks` provider file/URL status and preservation-safe install; `kanvibe-ai` readers | Settings project controls, task branch form, Task Detail provider hook recovery and AI/history | `RUNTIME` — service and production UI are connected; real packaged local/SSH provider fixtures and callback events remain |
| `releaseUpdates` | `getCurrentReleaseVersion`, `selectLatestReleaseUpdate`, `claimReleaseUpdateVersion`, `checkForReleaseUpdate` | `kanvibe-app::{select_native_release_update, native_release_update_service, native_updater}` and release packaging/verifiers | Release update dialog and native Install action | `BLOCKED` — digest/signature/notarization/identity verification and health-ack rollback are implemented, but a signed published asset and forced-failure rollback still require real-macOS Phase 5 evidence |
| `sessionDependency` | `getSessionDependencyStatus`, `installSessionDependency` | `kanvibe-session` shared command policy; `kanvibe-git` bounded local/SSH execution; `kanvibe-app::{get_native_task_session_dependency_status, install_native_task_session_dependency}` | Task terminal dependency check/install/retry controls and pre-spawn gate | `CONNECTED` — production local/SSH service and Task Detail recovery UI are wired; real packaged S14 local/SSH evidence remains |

## Direct Electron IPC

| Electron channel | Rust replacement | GPUI/runtime consumer | Status |
| --- | --- | --- | --- |
| `kanvibe:show-notification` | `NativeNotificationService` plus macOS `UNUserNotificationCenter` adapter | Hook/background publishers | `RUNTIME` — signed packaged delivery/click evidence remains |
| `kanvibe:notifications-list` | `NativeNotificationService::list` | Notification center | `CONNECTED` |
| `kanvibe:notifications-mark-read` | `NativeNotificationService::mark_read` | Notification activation | `CONNECTED` |
| `kanvibe:notifications-mark-all-read` | `NativeNotificationService::mark_all_read` | Notification center | `CONNECTED` |
| `kanvibe:notifications-activate` | persisted activation plus `consume_activation` | GPUI route/background-review activation | `RUNTIME` — signed system-notification click remains |
| `kanvibe:notifications-consume-activation` | `NativeNotificationService::consume_activation` | GPUI revision watcher | `CONNECTED` |
| `kanvibe:invoke` | Removed as a native architectural boundary; GPUI calls typed Rust APIs directly | All routes | `CONNECTED` (deletion waits for every namespace row) |
| `kanvibe:focus-existing-internal-route` | `resolve_window_open_action`, navigation history, GPUI window focus policy | Task/navigation actions | `RUNTIME` — S13 packaged window-count/focus evidence remains |
| `kanvibe:terminal-open` | `kanvibe-pty::{build_task_session_pty_request, spawn_pty}` and `TerminalView` entity | Task terminal | `RUNTIME` — S03/S04/S13/S14 terminal checklist must pass twice |

Electron terminal write/resize/focus/close and data/close event channels are
registered dynamically by the legacy terminal bridge rather than through
`ipcMain.handle`. Their Rust replacements are `PtyController` writer/resize/
terminate operations and the GPUI terminal event task; they share the same
`RUNTIME` terminal gate above.

## Product command boundary

- `./kanvibe-native` is the Node-free root command surface for native
  dev/build/test/check/package and Phase 5 verification.
- Native Linux and macOS CI invoke that root surface; macOS bundle CI also
  verifies the updater helper and exact `KanVibeBuildCommit`.
- `legacy:electron:*` package scripts expose the retained parity baseline
  explicitly. Generic package-manager desktop defaults still select that
  baseline until Phase 5, signed release, stabilization, and rollback evidence
  pass, so this boundary does not authorize Electron deletion.

## Deletion gate

The final Electron-removal commit must:

1. change every `SERVICE`, `RUNTIME`, and `BLOCKED` row to `CONNECTED` with a
   concrete inventory/run/release artifact;
2. archive approved baseline evidence;
3. remove the mapped Electron service/IPC/product implementation;
4. make the Issue #310 zero-reference search pass outside archive/history;
5. build, test, and package the native product from a clean checkout without
   installing Node or Electron.
