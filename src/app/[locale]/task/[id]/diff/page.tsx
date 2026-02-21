import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { getTranslations } from "next-intl/server";
import { getTaskById } from "@/app/actions/kanban";
import { getGitDiffFiles } from "@/app/actions/diff";
import { Link } from "@/i18n/navigation";
import DiffPageClient from "@/components/DiffPageClient";

export const dynamic = "force-dynamic";

interface DiffPageProps {
  params: Promise<{ locale: string; id: string }>;
}

/** 브라우저 탭 제목을 "Diff - {branchName}" 형식으로 동적 생성 */
export async function generateMetadata({
  params,
}: DiffPageProps): Promise<Metadata> {
  const { id } = await params;
  const task = await getTaskById(id);

  if (!task) return { title: "KanVibe" };

  return { title: `Diff - ${task.branchName ?? "KanVibe"}` };
}

export default async function DiffPage({ params }: DiffPageProps) {
  const { id } = await params;
  const task = await getTaskById(id);
  const t = await getTranslations("diffView");

  if (!task) notFound();

  /** 브랜치 정보가 없으면 안내 메시지를 표시한다 */
  if (!task.branchName) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noBranch")}</p>
        <Link
          href={`/task/${id}`}
          className="text-sm text-text-brand hover:underline"
        >
          {t("backToTask")}
        </Link>
      </div>
    );
  }

  if (!task.worktreePath) {
    return (
      <div className="h-screen flex flex-col items-center justify-center bg-bg-page gap-4">
        <p className="text-text-muted text-sm">{t("noWorktree")}</p>
        <Link
          href={`/task/${id}`}
          className="text-sm text-text-brand hover:underline"
        >
          {t("backToTask")}
        </Link>
      </div>
    );
  }

  const files = await getGitDiffFiles(id);

  return (
    <div className="h-screen flex flex-col bg-bg-page">
      {/* 상단 네비게이션 바 */}
      <header className="shrink-0 flex items-center gap-3 px-4 py-2.5 bg-bg-surface border-b border-border-default">
        <Link
          href={`/task/${id}`}
          className="flex items-center gap-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            className="shrink-0"
          >
            <path
              d="M10 12L6 8L10 4"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          {t("backToTask")}
        </Link>

        <div className="h-4 w-px bg-border-default" />

        <h1 className="text-sm font-semibold text-text-primary">
          {t("title")}
        </h1>

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

        {/* 파일 변경 수 뱃지 */}
        <div className="ml-auto flex items-center gap-2 text-xs text-text-muted">
          <span>{files.length} files</span>
        </div>
      </header>

      {/* 메인 영역 */}
      <DiffPageClient taskId={id} files={files} />
    </div>
  );
}
