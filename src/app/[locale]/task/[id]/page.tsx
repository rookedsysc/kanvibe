import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { redirect } from "@/i18n/navigation";
import { getTranslations } from "next-intl/server";
import {
  getTaskById,
  updateTaskStatus,
  deleteTask,
  fetchAndSavePrUrl,
} from "@/app/actions/kanban";
import { TaskStatus } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import TerminalLoader from "@/components/TerminalLoader";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import CollapsibleSidebar from "@/components/CollapsibleSidebar";
import TaskDetailTitleCard from "@/components/TaskDetailTitleCard";
import TaskDetailInfoCard from "@/components/TaskDetailInfoCard";
import { getTaskHooksStatus, getTaskGeminiHooksStatus, getTaskCodexHooksStatus } from "@/app/actions/project";
import { getSidebarDefaultCollapsed, getSidebarHintDismissed, getDoneAlertDismissed } from "@/app/actions/appSettings";
import { Link } from "@/i18n/navigation";

export const dynamicConfig = "force-dynamic";

/** 브라우저 탭 제목을 "{branchName} - {projectName}" 형식으로 동적 생성 */
export async function generateMetadata({
  params,
}: TaskDetailPageProps): Promise<Metadata> {
  const { id } = await params;
  const task = await getTaskById(id);

  if (!task) return { title: "KanVibe" };

  const parts = [task.branchName, task.project?.name].filter(Boolean);
  const title = parts.length > 0 ? parts.join(" - ") : "KanVibe";

  return { title };
}

interface TaskDetailPageProps {
  params: Promise<{ locale: string; id: string }>;
}

const STATUS_TRANSITIONS: { status: TaskStatus; labelKey: string }[] = [
  { status: TaskStatus.TODO, labelKey: "moveToTodo" },
  { status: TaskStatus.PROGRESS, labelKey: "moveToProgress" },
  { status: TaskStatus.REVIEW, labelKey: "moveToReview" },
  { status: TaskStatus.DONE, labelKey: "moveToDone" },
];

/** 에이전트 타입별 태그 스타일 매핑 */
const AGENT_TAG_STYLES: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

export default async function TaskDetailPage({ params }: TaskDetailPageProps) {
  const { locale, id } = await params;
  const task = await getTaskById(id);
  const t = await getTranslations("taskDetail");

  if (!task) notFound();

  /* 브랜치가 있고 PR URL이 아직 없으면 자동 조회 */
  if (task.branchName && !task.prUrl) {
    const prUrl = await fetchAndSavePrUrl(task.id);
    if (prUrl) task.prUrl = prUrl;
  }

  const hasTerminal = task.sessionType && task.sessionName;
  const claudeHooksStatus = task.projectId ? await getTaskHooksStatus(id) : null;
  const geminiHooksStatus = task.projectId ? await getTaskGeminiHooksStatus(id) : null;
  const codexHooksStatus = task.projectId ? await getTaskCodexHooksStatus(id) : null;
  const sidebarDefaultCollapsed = await getSidebarDefaultCollapsed();
  const sidebarHintDismissed = await getSidebarHintDismissed();
  const doneAlertDismissed = await getDoneAlertDismissed();

  async function handleStatusChange(formData: FormData) {
    "use server";
    const newStatus = formData.get("status") as TaskStatus;
    await updateTaskStatus(id, newStatus);
    if (newStatus === TaskStatus.DONE) {
      redirect({ href: "/", locale });
    }
  }

  async function handleDelete() {
    "use server";
    await deleteTask(id);
    redirect({ href: "/", locale });
  }

  const agentTagStyle = task.agentType
    ? AGENT_TAG_STYLES[task.agentType] ?? "bg-tag-neutral-bg text-tag-neutral-text"
    : null;

  return (
    <div className="h-screen flex flex-col lg:flex-row bg-bg-page p-4 gap-4">
      {/* 사이드바 - 작업 정보 (접기/열기 가능) */}
      <CollapsibleSidebar defaultCollapsed={sidebarDefaultCollapsed} showHint={!sidebarHintDismissed}>
        {/* 보드로 돌아가기 */}
        <Link
          href="/"
          className="flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
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
          {t("backToBoard")}
        </Link>

        <TaskDetailTitleCard task={task} />

        <TaskDetailInfoCard task={task} agentTagStyle={agentTagStyle} />

        {/* 상태 변경 + 네비게이션 카드 */}
        <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
          <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-3">
            {t("actions")}
          </h3>
          <div className="flex flex-wrap gap-2">
            {STATUS_TRANSITIONS.filter(
              (transition) => transition.status !== task.status
            ).map((transition) =>
              transition.status === TaskStatus.DONE ? (
                <DoneStatusButton
                  key={transition.status}
                  statusChangeAction={handleStatusChange}
                  label={t(transition.labelKey)}
                  hasCleanableResources={!!(task.branchName || task.sessionType)}
                  doneAlertDismissed={doneAlertDismissed}
                />
              ) : (
                <form key={transition.status} action={handleStatusChange}>
                  <input
                    type="hidden"
                    name="status"
                    value={transition.status}
                  />
                  <button
                    type="submit"
                    className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors"
                  >
                    {t(transition.labelKey)}
                  </button>
                </form>
              )
            )}
          </div>
          <div className="mt-3 pt-3 border-t border-border-subtle">
            <DeleteTaskButton deleteAction={handleDelete} />
          </div>
        </div>

        {/* Hooks 상태 카드 */}
        <HooksStatusCard
          taskId={task.id}
          initialClaudeStatus={claudeHooksStatus}
          initialGeminiStatus={geminiHooksStatus}
          initialCodexStatus={codexHooksStatus}
          isRemote={!!task.sshHost}
        />
      </CollapsibleSidebar>

      {/* 터미널 영역 */}
      <main className="flex-1 flex flex-col min-h-0 min-w-0">
        {hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg overflow-hidden shadow-md">
            {/* macOS 스타일 윈도우 크롬 */}
            <div className="bg-terminal-chrome flex items-center gap-2 px-4 py-2.5 shrink-0">
              <span className="w-3 h-3 rounded-full bg-traffic-close" />
              <span className="w-3 h-3 rounded-full bg-traffic-minimize" />
              <span className="w-3 h-3 rounded-full bg-traffic-maximize" />
              <span className="ml-3 text-xs text-terminal-text font-mono truncate">
                {task.sessionName ?? t("terminal")}
              </span>
            </div>
            {/* 터미널 본체 */}
            <div className="flex-1 min-h-0 bg-terminal-bg">
              <TerminalLoader taskId={task.id} />
            </div>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center border border-dashed border-border-default rounded-lg bg-bg-surface">
            {task.projectId ? (
              <ConnectTerminalForm taskId={task.id} />
            ) : (
              <p className="text-text-muted text-sm">{t("noTerminal")}</p>
            )}
          </div>
        )}
      </main>
    </div>
  );
}
