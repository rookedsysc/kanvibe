import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { useParams } from "react-router-dom";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import { getGitDiffFiles } from "@/desktop/renderer/actions/diff";
import { getTaskById } from "@/desktop/renderer/actions/kanban";
import DiffPageClient from "@/desktop/renderer/components/DiffPageClient";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

export default function DiffRoute() {
  const { id = "" } = useParams();
  const t = useTranslations("diffView");
  const router = useRouter();
  const refreshSignal = useRefreshSignal(["all", "diff"]);
  const [state, setState] = useState<{ task: Awaited<ReturnType<typeof getTaskById>>; files: Awaited<ReturnType<typeof getGitDiffFiles>> } | null>(null);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      const task = await getTaskById(id);
      const files = task?.branchName && task.worktreePath ? await getGitDiffFiles(id) : [];

      if (!cancelled) {
        setState({ task, files });
        document.title = task?.branchName ? `Diff - ${task.branchName}` : "";
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [id, refreshSignal]);

  if (!state) {
    return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
  }

  if (!state.task) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-bg-page px-4">
        <div className="flex flex-col items-center gap-4 text-center">
          <p className="text-text-muted">{t("taskNotFound")}</p>
          <button
            type="button"
            onClick={() => router.back()}
            className="rounded-md border border-border-default bg-bg-surface px-4 py-2 text-sm text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
          >
            {t("goBack")}
          </button>
        </div>
      </div>
    );
  }

  if (!state.task.branchName) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noBranch")}</p>
        <Link href={`/task/${id}`} className="text-sm text-text-brand hover:underline">{t("backToTask")}</Link>
      </div>
    );
  }

  if (!state.task.worktreePath) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noWorktree")}</p>
        <Link href={`/task/${id}`} className="text-sm text-text-brand hover:underline">{t("backToTask")}</Link>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-bg-page">
      <header className="shrink-0 flex items-center gap-3 px-4 py-2.5 bg-bg-surface border-b border-border-default">
        <Link href={`/task/${id}`} className="flex items-center gap-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0">
            <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {t("backToTask")}
        </Link>

        <div className="h-4 w-px bg-border-default" />
        <h1 className="text-sm font-semibold text-text-primary">{t("title")}</h1>

        <div className="h-4 w-px bg-border-default" />
        <div className="flex items-center gap-1.5">
          <span className="text-xs font-mono text-text-muted bg-tag-base-bg text-tag-base-text px-2 py-0.5 rounded-md">{state.task.baseBranch ?? "main"}</span>
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="text-text-muted">
            <path d="M5 3l4 4-4 4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          <span className="text-xs font-mono bg-tag-branch-bg text-tag-branch-text px-2 py-0.5 rounded-md">{state.task.branchName}</span>
        </div>

        <div className="ml-auto flex items-center gap-2 text-xs text-text-muted">
          <span>{state.files.length} files</span>
        </div>
      </header>

      <DiffPageClient taskId={id} files={state.files} />
    </div>
  );
}
