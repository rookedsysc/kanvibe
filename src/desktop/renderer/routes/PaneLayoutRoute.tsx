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

function createEmptyPaneLayoutState(): PaneLayoutState {
  return {
    globalConfig: null,
    projects: [],
    projectConfigs: new Map(),
  };
}

export default function PaneLayoutRoute() {
  const t = useTranslations("paneLayout");
  const refreshSignal = useRefreshSignal(["all", "pane-layout"]);
  const [state, setState] = useState<PaneLayoutState | null>(null);

  useEffect(() => {
    document.title = "Pane Layout";
  }, []);

  useEffect(() => {
    let cancelled = false;
    let loadingTimeout: number | null = window.setTimeout(() => {
      loadingTimeout = null;
      if (!cancelled) {
        logDesktopInitialLoadTimeout("pane-layout");
        setState((current) => current ?? createEmptyPaneLayoutState());
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
          } catch {
            projectConfigs.set(project.id, null);
          }
        }));

        clearLoadingTimeout();
        if (!cancelled) {
          setState({ globalConfig, projects, projectConfigs });
        }
      } catch (error) {
        clearLoadingTimeout();
        console.error("Failed to load pane layout route data:", error);
        if (!cancelled) {
          setState((current) => current ?? createEmptyPaneLayoutState());
        }
      }
    })();

    return () => {
      cancelled = true;
      clearLoadingTimeout();
    };
  }, [refreshSignal]);

  if (!state) {
    return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
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
                const config = state.projectConfigs.get(project.id) ?? null;
                return (
                  <details key={project.id} className="bg-bg-surface rounded-xl border border-border-default">
                    <summary className="flex items-center justify-between p-4 cursor-pointer select-none">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-text-primary">{project.name}</span>
                        <span className={`text-xs px-2 py-0.5 rounded-full ${config ? "bg-brand-primary/10 text-brand-primary" : "bg-bg-page text-text-muted"}`}>
                          {config ? t("hasOverride") : t("usingGlobal")}
                        </span>
                      </div>
                      <span className="text-xs text-text-muted">{t("configure")}</span>
                    </summary>
                    <div className="p-4 pt-0 border-t border-border-default">
                      <PaneLayoutEditor projectId={project.id} initialConfig={config} />
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
