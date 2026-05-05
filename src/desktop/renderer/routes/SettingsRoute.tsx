import { useEffect, useState } from "react";
import ProjectSettings from "@/components/ProjectSettings";
import {
  getDefaultSessionType,
  getNotificationSettings,
  getSidebarDefaultCollapsed,
  getThemePreference,
  type ThemePreference,
} from "@/desktop/renderer/actions/appSettings";
import { getAllProjects, getAvailableHosts } from "@/desktop/renderer/actions/project";
import { useRouter } from "@/desktop/renderer/navigation";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS, logDesktopInitialLoadTimeout } from "@/desktop/renderer/utils/loadingTimeout";
import { SessionType } from "@/entities/KanbanTask";

interface SettingsData {
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sshHosts: string[];
  sidebarDefaultCollapsed: boolean;
  notificationSettings: Awaited<ReturnType<typeof getNotificationSettings>>;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
  themePreference: ThemePreference;
}

function createEmptySettingsData(): SettingsData {
  return {
    projects: [],
    sshHosts: [],
    sidebarDefaultCollapsed: false,
    notificationSettings: { isEnabled: true, enabledStatuses: ["progress", "pending", "review"] },
    defaultSessionType: SessionType.TMUX,
    themePreference: "system",
  };
}

export default function SettingsRoute() {
  const router = useRouter();
  const refreshSignal = useRefreshSignal(["all", "settings"]);
  const [data, setData] = useState<SettingsData | null>(null);

  useEffect(() => {
    document.title = "Settings";
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadingTimeout = window.setTimeout(() => {
      if (!cancelled) {
        logDesktopInitialLoadTimeout("settings");
        setData((currentData) => currentData ?? createEmptySettingsData());
      }
    }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

    Promise.all([
      getAllProjects(),
      getAvailableHosts(),
      getSidebarDefaultCollapsed(),
      getNotificationSettings(),
      getDefaultSessionType(),
      getThemePreference(),
    ]).then(([projects, sshHosts, sidebarDefaultCollapsed, notificationSettings, defaultSessionType, themePreference]) => {
      window.clearTimeout(loadingTimeout);
      if (!cancelled) {
        setData({
          projects,
          sshHosts,
          sidebarDefaultCollapsed,
          notificationSettings,
          defaultSessionType,
          themePreference,
        });
      }
    }).catch((error) => {
      window.clearTimeout(loadingTimeout);
      console.error("Failed to load settings route data:", error);
      if (!cancelled) {
        setData((currentData) => currentData ?? createEmptySettingsData());
      }
    });

    return () => {
      cancelled = true;
      window.clearTimeout(loadingTimeout);
    };
  }, [refreshSignal]);

  if (!data) {
    return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
  }

  return (
    <ProjectSettings
      isOpen
      variant="page"
      onClose={() => router.back()}
      projects={data.projects}
      sshHosts={data.sshHosts}
      sidebarDefaultCollapsed={data.sidebarDefaultCollapsed}
      defaultSessionType={data.defaultSessionType}
      themePreference={data.themePreference}
      onDefaultSessionTypeChange={(sessionType) => {
        setData((currentData) => currentData ? { ...currentData, defaultSessionType: sessionType } : currentData);
      }}
      onThemePreferenceChange={(themePreference) => {
        setData((currentData) => currentData ? { ...currentData, themePreference } : currentData);
      }}
      notificationSettings={data.notificationSettings}
    />
  );
}
