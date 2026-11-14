import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import InlineLoadError from "@/components/InlineLoadError";
import RouteRefreshFailureBanner from "@/desktop/renderer/components/RouteRefreshFailureBanner";
import ProjectSettings from "@/components/ProjectSettings";
import {
  getBackgroundSyncSettings,
  getDefaultSessionType,
  getNotificationSettings,
  getSidebarDefaultCollapsed,
  getThemePreference,
  getVimModeEnabled,
  type ThemePreference,
} from "@/desktop/renderer/actions/appSettings";
import { getAllProjects, getAvailableHosts } from "@/desktop/renderer/actions/project";
import { useRouter } from "@/desktop/renderer/navigation";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { useRouteLoadStatus } from "@/desktop/renderer/utils/loadingTimeout";

interface SettingsData {
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sshHosts: string[];
  sidebarDefaultCollapsed: boolean;
  notificationSettings: Awaited<ReturnType<typeof getNotificationSettings>>;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
  vimModeEnabled: Awaited<ReturnType<typeof getVimModeEnabled>>;
  themePreference: ThemePreference;
  backgroundSyncSettings: Awaited<ReturnType<typeof getBackgroundSyncSettings>>;
}

export default function SettingsRoute() {
  const router = useRouter();
  const refreshSignal = useRefreshSignal(["all", "settings"]);
  const [data, setData] = useState<SettingsData | null>(null);
  const tc = useTranslations("common");
  const { isLoadDelayed, loadFailureReason, retrySignal, beginLoad, retryLoad } = useRouteLoadStatus();

  useEffect(() => {
    document.title = "Settings";
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadAttempt = beginLoad("settings");

    Promise.all([
      getAllProjects(),
      getAvailableHosts(),
      getSidebarDefaultCollapsed(),
      getNotificationSettings(),
      getDefaultSessionType(),
      getVimModeEnabled(),
      getThemePreference(),
      getBackgroundSyncSettings(),
    ]).then(([projects, sshHosts, sidebarDefaultCollapsed, notificationSettings, defaultSessionType, vimModeEnabled, themePreference, backgroundSyncSettings]) => {
      if (!cancelled) {
        loadAttempt.succeed();
        setData({
          projects,
          sshHosts,
          sidebarDefaultCollapsed,
          notificationSettings,
          defaultSessionType,
          vimModeEnabled,
          themePreference,
          backgroundSyncSettings,
        });
      }
    }).catch((error) => {
      console.error("Failed to load settings route data:", error);
      if (!cancelled) {
        loadAttempt.fail(error);
      }
    });

    return () => {
      cancelled = true;
      loadAttempt.cancel();
    };
  }, [beginLoad, refreshSignal, retrySignal]);

  if (!data) {
    return (
      <div className="min-h-screen flex flex-col items-center justify-center gap-3 bg-bg-page px-4 text-text-muted">
        {loadFailureReason ? (
          <InlineLoadError
            className="w-full max-w-md"
            message={tc("loadFailed", { reason: loadFailureReason })}
            onRetry={retryLoad}
          />
        ) : (
          <>
            <span>Loading...</span>
            {isLoadDelayed ? <p role="status" className="text-xs">{tc("loadDelayed")}</p> : null}
          </>
        )}
      </div>
    );
  }

  return (
    <>
      {loadFailureReason ? (
        <RouteRefreshFailureBanner reason={loadFailureReason} onRetry={retryLoad} />
      ) : null}
      <ProjectSettings
        isOpen
        variant="page"
        onClose={() => router.back()}
        projects={data.projects}
        sshHosts={data.sshHosts}
        sidebarDefaultCollapsed={data.sidebarDefaultCollapsed}
        defaultSessionType={data.defaultSessionType}
        vimModeEnabled={data.vimModeEnabled}
        themePreference={data.themePreference}
        onDefaultSessionTypeChange={(sessionType) => {
          setData((currentData) => currentData ? { ...currentData, defaultSessionType: sessionType } : currentData);
        }}
        onThemePreferenceChange={(themePreference) => {
          setData((currentData) => currentData ? { ...currentData, themePreference } : currentData);
        }}
        onVimModeEnabledChange={(vimModeEnabled) => {
          setData((currentData) => currentData ? { ...currentData, vimModeEnabled } : currentData);
        }}
        notificationSettings={data.notificationSettings}
        backgroundSyncSettings={data.backgroundSyncSettings}
      />
    </>
  );
}
