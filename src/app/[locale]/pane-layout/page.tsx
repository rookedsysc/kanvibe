"use client";

import { useState, useEffect } from "react";
import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { ipcProject, ipcPaneLayout } from "@/lib/ipc";
import PaneLayoutEditor from "@/components/PaneLayoutEditor";
import type { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";
import type { Project } from "@/entities/Project";

export default function PaneLayoutPage() {
  const t = useTranslations("paneLayout");
  const [loading, setLoading] = useState(true);
  const [globalConfig, setGlobalConfig] = useState<PaneLayoutConfig | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectConfigs, setProjectConfigs] = useState<Map<string, PaneLayoutConfig | null>>(new Map());

  useEffect(() => {
    async function load() {
      try {
        const [global, projs] = await Promise.all([
          ipcPaneLayout.getGlobal(),
          ipcProject.getAll(),
        ]);

        setGlobalConfig(global);
        setProjects(projs);

        const configs = new Map<string, PaneLayoutConfig | null>();
        await Promise.all(
          projs.map(async (project) => {
            try {
              const config = await ipcPaneLayout.getProject(project.id);
              configs.set(project.id, config);
            } catch {
              configs.set(project.id, null);
            }
          })
        );
        setProjectConfigs(configs);
      } catch (error) {
        console.error("Pane 레이아웃 데이터 로딩 실패:", error);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  if (loading) {
    return (
      <div className="min-h-screen bg-bg-page flex items-center justify-center">
        <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
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
          <Link href="/" className="text-sm text-brand-primary hover:underline">
            {t("backToBoard")}
          </Link>
        </div>

        <section className="bg-bg-surface rounded-xl border border-border-default p-5">
          <h2 className="text-sm font-semibold text-text-primary mb-1">{t("globalDefault")}</h2>
          <p className="text-xs text-text-muted mb-4">{t("globalDescription")}</p>
          <PaneLayoutEditor initialConfig={globalConfig} isGlobal />
        </section>

        <section>
          <h2 className="text-sm font-semibold text-text-primary mb-1">{t("projectOverride")}</h2>
          <p className="text-xs text-text-muted mb-4">{t("projectOverrideDescription")}</p>

          {projects.length === 0 ? (
            <p className="text-sm text-text-muted">{t("noProjects")}</p>
          ) : (
            <div className="space-y-4">
              {projects.map((project) => {
                const config = projectConfigs.get(project.id) ?? null;
                return (
                  <details
                    key={project.id}
                    className="bg-bg-surface rounded-xl border border-border-default"
                  >
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
