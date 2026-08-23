"use client";

import { useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import { CountUpNumber } from "@/components/CountUpNumber";
import { Link } from "@/desktop/renderer/navigation";
import type { DiffFile } from "@/desktop/renderer/actions/diff";
import { summarizeDiffFiles, type TaskDiffStats } from "@/desktop/shared/taskDiffStats";

/** 파일 상태를 한 글자로 줄인 표시. 좁은 목록에서 경로를 밀어내지 않으려고 약어를 쓴다 */
const STATUS_LABELS: Record<DiffFile["status"], string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
};

const STATUS_COLORS: Record<DiffFile["status"], string> = {
  added: "text-status-success",
  modified: "text-status-warning",
  deleted: "text-status-error",
  renamed: "text-status-info",
};

function FileDiffIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="18" cy="18" r="2.5" />
      <path d="M6 8.5v4c0 2 1.5 3.5 3.5 3.5H14" />
      <path d="M15 13l3 3-3 3" />
    </svg>
  );
}

/** 파일 수와 추가·삭제 줄 수. 보드 카드와 상세 패널이 같은 숫자를 같은 배치로 보여준다 */
function DiffStatCounts({ stats }: { stats: TaskDiffStats }) {
  const t = useTranslations("taskDetail.diffStats");

  return (
    <>
      <CountUpNumber value={stats.fileCount} className="font-semibold text-text-secondary" testId="task-diff-file-count" />
      <span className="text-status-success" title={t("additions", { count: stats.additions })}>
        +<CountUpNumber value={stats.additions} testId="task-diff-additions" />
      </span>
      <span className="text-status-error" title={t("deletions", { count: stats.deletions })}>
        -<CountUpNumber value={stats.deletions} testId="task-diff-deletions" />
      </span>
    </>
  );
}

/**
 * 보드 카드에 그 태스크의 변경 규모를 띄운다.
 * 아직 아무것도 바뀌지 않은 태스크는 배지를 그리지 않는다. 0만 늘어놓으면 카드가 볼 것 없는 값으로 채워진다.
 */
export function TaskDiffStatsBadge({ stats }: { stats?: TaskDiffStats }) {
  const t = useTranslations("taskDetail.diffStats");

  if (!stats || stats.fileCount === 0) {
    return null;
  }

  const summaryText = t("summary", {
    files: stats.fileCount,
    additions: stats.additions,
    deletions: stats.deletions,
  });

  return (
    <span
      className="inline-flex items-center gap-1 rounded border border-border-subtle px-1.5 py-0.5 text-[10px] leading-none"
      title={summaryText}
      aria-label={summaryText}
      data-testid="task-card-diff-stats"
    >
      <FileDiffIcon />
      <DiffStatCounts stats={stats} />
    </span>
  );
}

/** 추가와 삭제의 비율을 한 줄 막대로 보여준다. 숫자를 읽기 전에 변경의 성격부터 눈에 들어오게 한다 */
function ChangeRatioBar({ stats }: { stats: TaskDiffStats }) {
  const changedLineCount = stats.additions + stats.deletions;
  const additionPercent = changedLineCount === 0 ? 0 : Math.round((stats.additions / changedLineCount) * 100);

  return (
    <div
      className="flex h-1 w-full overflow-hidden rounded-full bg-border-subtle"
      aria-hidden="true"
      data-testid="task-diff-ratio-bar"
    >
      <div className="kv-diff-ratio-fill bg-status-success" style={{ width: `${additionPercent}%` }} />
      <div className="kv-diff-ratio-fill bg-status-error" style={{ width: `${100 - additionPercent}%` }} />
    </div>
  );
}

function ChangedFileList({ files }: { files: DiffFile[] }) {
  return (
    <ul className="max-h-48 w-full space-y-0.5 overflow-y-auto" data-testid="task-diff-file-list">
      {files.map((file) => (
        <li key={file.path} className="flex items-center gap-1.5 text-[11px] leading-4">
          <span className={`w-2.5 shrink-0 font-mono font-semibold ${STATUS_COLORS[file.status]}`}>
            {STATUS_LABELS[file.status]}
          </span>
          <span className="min-w-0 flex-1 truncate text-text-secondary" title={file.path}>
            {file.path}
          </span>
          <span className="shrink-0 tabular-nums text-status-success">+{file.additions}</span>
          <span className="shrink-0 tabular-nums text-status-error">-{file.deletions}</span>
        </li>
      ))}
    </ul>
  );
}

/**
 * 태스크 상세의 작업 정보에서 변경 규모를 보여준다.
 * 파일 목록은 접어 두고 집계만 남긴다. 정보 패널은 다른 항목과 자리를 나눠 써야 해서 목록이 항상 펼쳐져 있으면 나머지를 밀어낸다.
 */
export function TaskDiffSummary({ taskId, files }: { taskId: string; files: DiffFile[] }) {
  const t = useTranslations("taskDetail");
  const [isExpanded, setIsExpanded] = useState(false);
  const stats = useMemo(() => summarizeDiffFiles(files), [files]);
  const hasChanges = stats.fileCount > 0;

  return (
    <div className="flex flex-wrap items-center justify-between gap-y-1.5" data-testid="task-diff-summary">
      <dt className="text-xs text-text-muted">{t("diffFiles")}</dt>
      <dd className="flex items-center gap-1.5">
        {hasChanges && (
          <button
            type="button"
            onClick={() => setIsExpanded((current) => !current)}
            aria-expanded={isExpanded}
            aria-label={t(isExpanded ? "diffStats.collapse" : "diffStats.expand")}
            className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-xs transition-colors hover:bg-bg-page"
            data-testid="task-diff-summary-toggle"
          >
            <DiffStatCounts stats={stats} />
          </button>
        )}
        <Link
          href={`/task/${taskId}/diff`}
          className="inline-flex items-center gap-1.5 rounded bg-tag-branch-bg px-2 py-0.5 text-xs text-tag-branch-text transition-opacity hover:opacity-80"
          title={t("viewDiff")}
        >
          <FileDiffIcon />
        </Link>
      </dd>

      {hasChanges && <ChangeRatioBar stats={stats} />}
      {hasChanges && isExpanded && <ChangedFileList files={files} />}
    </div>
  );
}
