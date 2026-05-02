import * as appSettings from "@/desktop/main/services/appSettingsService";
import * as diff from "@/desktop/main/services/diffService";
import * as githubCliDependency from "@/desktop/main/services/githubCliDependencyService";
import * as hooks from "@/desktop/main/services/hookService";
import * as kanban from "@/desktop/main/services/kanbanService";
import * as paneLayout from "@/desktop/main/services/paneLayoutService";
import * as project from "@/desktop/main/services/projectService";
import * as sessionDependency from "@/desktop/main/services/sessionDependencyService";

export const desktopServices = {
  appSettings,
  diff,
  githubCliDependency,
  hooks,
  kanban,
  paneLayout,
  project,
  sessionDependency,
} as const;

export type DesktopServiceNamespace = keyof typeof desktopServices;
