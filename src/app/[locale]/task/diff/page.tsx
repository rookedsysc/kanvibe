"use client";

import { Suspense, useState, useEffect } from "react";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { Link } from "@/i18n/navigation";
import { ipcKanban, ipcDiff } from "@/lib/ipc";
import type { DiffFile } from "@/lib/ipc";
import type { KanbanTask } from "@/entities/KanbanTask";
import DiffPageClient from "@/components/DiffPageClient";

function DiffPageContent() {
  const searchParams = useSearchParams();
  const taskId = searchParams.get("id");
  const t = useTranslations("diffView");

  const [task, setTask] = useState<KanbanTask | null>(null);
  const [files, setFiles] = useState<DiffFile[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!taskId) return;

    async function load() {
      try {
        const taskData = await ipcKanban.getTaskById(taskId!);
        if (!taskData) {
          setLoading(false);
          return;
        }

        setTask(taskData);
        document.title = `Diff - ${taskData.branchName ?? "KanVibe"}`;

        if (taskData.branchName) {
          const diffFiles = await ipcDiff.getGitDiffFiles(taskId!);
          setFiles(diffFiles);
        }
      } catch (error) {
        console.error("Diff 데이터 로딩 실패:", error);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, [taskId]);

  if (!taskId) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <p className="text-text-muted text-sm">Task ID가 필요합니다.</p>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
      </div>
    );
  }

  if (!task) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <p className="text-text-muted text-sm">작업을 찾을 수 없습니다.</p>
      </div>
    );
  }

  if (!task.branchName) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noBranch")}</p>
        <Link href={`/task?id=${taskId}`} className="text-sm text-text-brand hover:underline">
          {t("backToTask")}
        </Link>
      </div>
    );
  }

  if (!task.worktreePath) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noWorktree")}</p>
        <Link href={`/task?id=${taskId}`} className="text-sm text-text-brand hover:underline">
          {t("backToTask")}
        </Link>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-bg-page">
      <header className="shrink-0 flex items-center gap-3 px-4 py-2.5 bg-bg-surface border-b border-border-default">
        <Link
          href={`/task?id=${taskId}`}
          className="flex items-center gap-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0">
            <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {t("backToTask")}
        </Link>

        <div className="h-4 w-px bg-border-default" />

        <h1 className="text-sm font-semibold text-text-primary">{t("title")}</h1>

        {task.branchName && (
          <>
            <div className="h-4 w-px bg-border-default" />
            <div className="flex items-center gap-1.5">
              <span className="text-xs font-mono text-text-muted bg-tag-base-bg text-tag-base-text px-2 py-0.5 rounded-md">
                {task.baseBranch ?? "main"}
              </span>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="text-text-muted">
                <path d="M5 3l4 4-4 4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="text-xs font-mono bg-tag-branch-bg text-tag-branch-text px-2 py-0.5 rounded-md">
                {task.branchName}
              </span>
            </div>
          </>
        )}

        <div className="ml-auto flex items-center gap-2 text-xs text-text-muted">
          <span>{files.length} files</span>
        </div>
      </header>

      <DiffPageClient taskId={taskId} files={files} />
    </div>
  );
}

export default function DiffPage() {
  return (
    <Suspense
      fallback={
        <div className="h-screen flex items-center justify-center bg-bg-page">
          <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
        </div>
      }
    >
      <DiffPageContent />
    </Suspense>
  );
}
