import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { useParams } from "react-router-dom";
import InlineLoadError from "@/components/InlineLoadError";
import RouteRefreshFailureBanner from "@/desktop/renderer/components/RouteRefreshFailureBanner";
import { Link, usePathname, useRouter } from "@/desktop/renderer/navigation";
import { getGitDiffFiles } from "@/desktop/renderer/actions/diff";
import { getTaskById } from "@/desktop/renderer/actions/kanban";
import DiffPageClient from "@/desktop/renderer/components/DiffPageClient";
import { useRouteLoadStatus } from "@/desktop/renderer/utils/loadingTimeout";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import {
  navigateToTaskDetail,
  shouldHandleTaskNavigationClick,
} from "@/desktop/renderer/utils/taskNavigation";

interface DiffRouteState {
  task: Awaited<ReturnType<typeof getTaskById>>;
  files: Awaited<ReturnType<typeof getGitDiffFiles>>;
}

export default function DiffRoute() {
  const { id = "" } = useParams();
  const t = useTranslations("diffView");
  const tc = useTranslations("common");
  const router = useRouter();
  const pathname = usePathname();
  const refreshSignal = useRefreshSignal(["all", "diff"]);
  const [state, setState] = useState<DiffRouteState | null>(null);
  const currentLocale = pathname.split("/").filter(Boolean)[0];
  const { isLoadDelayed, loadFailureReason, retrySignal, beginLoad, retryLoad } = useRouteLoadStatus();

  function handleTaskDetailLinkClick(event: React.MouseEvent<HTMLAnchorElement>) {
    if (!shouldHandleTaskNavigationClick(event)) {
      return;
    }

    event.preventDefault();
    void navigateToTaskDetail(id, {
      currentLocale,
      navigate: router.push,
    });
  }

  useEffect(() => {
    let cancelled = false;
    const loadAttempt = beginLoad("diff", { taskId: id });

    void (async () => {
      try {
        const task = await getTaskById(id);
        const files = task?.branchName && task.worktreePath ? await getGitDiffFiles(id) : [];

        if (!cancelled) {
          loadAttempt.succeed();
          setState({ task, files });
          document.title = task?.branchName ? `Diff - ${task.branchName}` : "";
        }
      } catch (error) {
        console.error("Failed to load diff route data:", error);
        if (!cancelled) {
          loadAttempt.fail(error);
        }
      }
    })();

    return () => {
      cancelled = true;
      loadAttempt.cancel();
    };
  }, [beginLoad, id, refreshSignal, retrySignal]);

  // 조회 실패는 task가 없다는 뜻이 아니므로 아래의 task 없음 화면으로 넘기지 않는다
  if (!state) {
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
        <Link href={`/task/${id}`} onClick={handleTaskDetailLinkClick} className="text-sm text-text-brand hover:underline">{t("backToTask")}</Link>
      </div>
    );
  }

  if (!state.task.worktreePath) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noWorktree")}</p>
        <Link href={`/task/${id}`} onClick={handleTaskDetailLinkClick} className="text-sm text-text-brand hover:underline">{t("backToTask")}</Link>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-bg-page">
      {loadFailureReason ? (
        <RouteRefreshFailureBanner reason={loadFailureReason} onRetry={retryLoad} />
      ) : null}
      <header className="shrink-0 flex items-center gap-3 px-4 py-2.5 bg-bg-surface border-b border-border-default">
        <Link href={`/task/${id}`} onClick={handleTaskDetailLinkClick} className="flex items-center gap-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors">
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
