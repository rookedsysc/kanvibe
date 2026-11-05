import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import PaneLayoutEditor from "@/components/PaneLayoutEditor";
import { Link } from "@/desktop/renderer/navigation";
import { getAllProjects } from "@/desktop/renderer/actions/project";
import { getGlobalPaneLayout, getProjectPaneLayout } from "@/desktop/renderer/actions/paneLayout";
import type { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS, logDesktopInitialLoadTimeout } from "@/desktop/renderer/utils/loadingTimeout";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

interface PaneLayoutState {
  globalConfig: PaneLayoutConfig | null;
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  projectConfigs: Map<string, PaneLayoutConfig | null>;
}

export default function PaneLayoutRoute() {
  const t = useTranslations("paneLayout");
  const tc = useTranslations("common");
  const [retrySignal, setRetrySignal] = useState(0);
  const [loadFailed, setLoadFailed] = useState(false);
  const refreshSignal = useRefreshSignal(["all", "pane-layout"]);
  const [state, setState] = useState<PaneLayoutState | null>(null);

  useEffect(() => {
    document.title = "Pane Layout";
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoadFailed(false);
    let loadingTimeout: number | null = window.setTimeout(() => {
      loadingTimeout = null;
      if (!cancelled) {
        logDesktopInitialLoadTimeout("pane-layout");
        setLoadFailed(true);
      }
    }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

    const clearLoadingTimeout = () => {
      if (loadingTimeout === null) {
        return;
      }

      window.clearTimeout(loadingTimeout);
      loadingTimeout = null;
    };

    (async () => {
      try {
        const [globalConfig, projects] = await Promise.all([getGlobalPaneLayout(), getAllProjects()]);
        const projectConfigs = new Map<string, PaneLayoutConfig | null>();

        await Promise.all(projects.map(async (project) => {
          try {
            projectConfigs.set(project.id, await getProjectPaneLayout(project.id));
          } catch (error) {
            console.error("Failed to load project pane layout:", error);
          }
        }));

        clearLoadingTimeout();
        if (!cancelled) {
          setLoadFailed(false);
          setState((current) => {
            for (const project of projects) {
              if (!projectConfigs.has(project.id) && current?.projectConfigs.has(project.id)) {
                projectConfigs.set(project.id, current.projectConfigs.get(project.id) ?? null);
              }
            }
            return { globalConfig, projects, projectConfigs };
          });
        }
      } catch (error) {
        clearLoadingTimeout();
        console.error("Failed to load pane layout route data:", error);
        if (!cancelled) {
          setLoadFailed(true);
        }
      }
    })();

    return () => {
      cancelled = true;
      clearLoadingTimeout();
    };
  }, [refreshSignal, retrySignal]);

  const retryButton = (
    <button type="button" onClick={() => setRetrySignal((current) => current + 1)}
      className="text-sm text-brand-primary hover:underline">
      {t("retry")}
    </button>
  );

  if (!state) {
    return (
      <div className="min-h-screen flex flex-col gap-3 items-center justify-center bg-bg-page text-text-muted">
        {loadFailed ? <><p role="alert">{t("loadError")}</p>{retryButton}</> : tc("loading")}
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-bg-page p-6">
      <div className="max-w-2xl mx-auto space-y-8">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-text-primary">{t("title")}</h1>
            <p className="text-sm text-text-secondary mt-1">{t("description")}</p>
          </div>
          <Link href="/" className="text-sm text-brand-primary hover:underline">{t("backToBoard")}</Link>
        </div>

        <section className="bg-bg-surface rounded-xl border border-border-default p-5">
          <h2 className="text-sm font-semibold text-text-primary mb-1">{t("globalDefault")}</h2>
          <p className="text-xs text-text-muted mb-4">{t("globalDescription")}</p>
          <PaneLayoutEditor initialConfig={state.globalConfig} isGlobal />
        </section>

        <section>
          <h2 className="text-sm font-semibold text-text-primary mb-1">{t("projectOverride")}</h2>
          <p className="text-xs text-text-muted mb-4">{t("projectOverrideDescription")}</p>

          {state.projects.length === 0 ? (
            <p className="text-sm text-text-muted">{t("noProjects")}</p>
          ) : (
            <div className="space-y-4">
              {state.projects.map((project) => {
                const config = state.projectConfigs.get(project.id);
                const hasLoadedConfig = state.projectConfigs.has(project.id);
                return (
                  <details key={project.id} className="bg-bg-surface rounded-xl border border-border-default">
                    <summary className="flex items-center justify-between p-4 cursor-pointer select-none">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-text-primary">{project.name}</span>
                        <span className={`text-xs px-2 py-0.5 rounded-full ${config ? "bg-brand-primary/10 text-brand-primary" : "bg-bg-page text-text-muted"}`}>
                          {hasLoadedConfig ? (config ? t("hasOverride") : t("usingGlobal")) : t("loadError")}
                        </span>
                      </div>
                      <span className="text-xs text-text-muted">{t("configure")}</span>
                    </summary>
                    <div className="p-4 pt-0 border-t border-border-default">
                      {hasLoadedConfig ? (
                        <PaneLayoutEditor projectId={project.id} initialConfig={config} />
                      ) : (
                        <div className="py-3 space-y-2"><p role="alert" className="text-sm text-status-error">{t("loadError")}</p>{retryButton}</div>
                      )}
                    </div>
                  </details>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
