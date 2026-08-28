import * as aiAccounts from "@/desktop/main/services/aiAccountService";
import * as aiUsage from "@/desktop/main/services/aiUsageService";
import * as appSettings from "@/desktop/main/services/appSettingsService";
import { runBackgroundTaskSyncNow } from "@/desktop/main/services/backgroundTaskSyncService";
import * as diff from "@/desktop/main/services/diffService";
import * as editor from "@/desktop/main/services/editorService";
import * as githubCliDependency from "@/desktop/main/services/githubCliDependencyService";
import * as hooks from "@/desktop/main/services/hookService";
import * as kanban from "@/desktop/main/services/kanbanService";
import * as paneLayout from "@/desktop/main/services/paneLayoutService";
import * as project from "@/desktop/main/services/projectService";
import * as releaseUpdates from "@/desktop/main/services/releaseUpdateService";
import * as sessionDependency from "@/desktop/main/services/sessionDependencyService";
import * as terminalTabs from "@/desktop/main/services/terminalTabService";

const backgroundTaskSync = { runBackgroundTaskSyncNow };

export const desktopServices = {
  aiAccounts,
  aiUsage,
  appSettings,
  backgroundTaskSync,
  diff,
  editor,
  githubCliDependency,
  hooks,
  kanban,
  paneLayout,
  project,
  releaseUpdates,
  sessionDependency,
  terminalTabs,
} as const;

export type DesktopServiceNamespace = keyof typeof desktopServices;
