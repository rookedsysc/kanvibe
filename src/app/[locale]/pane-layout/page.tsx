import { getTranslations } from "next-intl/server";
import { Link } from "@/i18n/navigation";
import { getAllProjects } from "@/app/actions/project";
import { getGlobalPaneLayout, getProjectPaneLayout } from "@/app/actions/paneLayout";
import PaneLayoutEditor from "@/components/PaneLayoutEditor";
import type { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";

export const dynamic = "force-dynamic";

export default async function PaneLayoutPage() {
  const t = await getTranslations("paneLayout");

  let globalConfig: PaneLayoutConfig | null = null;
  let projects: Awaited<ReturnType<typeof getAllProjects>> = [];
  try {
    [globalConfig, projects] = await Promise.all([
      getGlobalPaneLayout(),
      getAllProjects(),
    ]);
  } catch (error) {
    console.error("Pane 레이아웃 페이지 데이터 로딩 실패:", error);
  }

  /** 프로젝트별 레이아웃 조회 */
  const projectConfigs = new Map<string, PaneLayoutConfig | null>();
  await Promise.all(
    projects.map(async (project) => {
      try {
        const config = await getProjectPaneLayout(project.id);
        projectConfigs.set(project.id, config);
      } catch {
        projectConfigs.set(project.id, null);
      }
    })
  );

  return (
    <div className="min-h-screen bg-bg-page p-6">
      <div className="max-w-2xl mx-auto space-y-8">
        {/* 헤더 */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-text-primary">{t("title")}</h1>
            <p className="text-sm text-text-secondary mt-1">{t("description")}</p>
          </div>
          <Link
            href="/"
            className="text-sm text-brand-primary hover:underline"
          >
            {t("backToBoard")}
          </Link>
        </div>

        {/* 글로벌 기본값 */}
        <section className="bg-bg-surface rounded-xl border border-border-default p-5">
          <h2 className="text-sm font-semibold text-text-primary mb-1">
            {t("globalDefault")}
          </h2>
          <p className="text-xs text-text-muted mb-4">{t("globalDescription")}</p>
          <PaneLayoutEditor initialConfig={globalConfig} isGlobal />
        </section>

        {/* 프로젝트별 오버라이드 */}
        <section>
          <h2 className="text-sm font-semibold text-text-primary mb-1">
            {t("projectOverride")}
          </h2>
          <p className="text-xs text-text-muted mb-4">
            {t("projectOverrideDescription")}
          </p>

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
                        <span className="text-sm font-medium text-text-primary">
                          {project.name}
                        </span>
                        <span
                          className={`text-xs px-2 py-0.5 rounded-full ${
                            config
                              ? "bg-brand-primary/10 text-brand-primary"
                              : "bg-bg-page text-text-muted"
                          }`}
                        >
                          {config ? t("hasOverride") : t("usingGlobal")}
                        </span>
                      </div>
                      <span className="text-xs text-text-muted">{t("configure")}</span>
                    </summary>
                    <div className="p-4 pt-0 border-t border-border-default">
                      <PaneLayoutEditor
                        projectId={project.id}
                        initialConfig={config}
                      />
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
