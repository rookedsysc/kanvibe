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
import TaskStatusBadge from "@/components/TaskStatusBadge";
import PriorityEditor from "@/components/PriorityEditor";
import TerminalLoader from "@/components/TerminalLoader";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import CollapsibleSidebar from "@/components/CollapsibleSidebar";
import { getTaskHooksStatus, getTaskGeminiHooksStatus, getTaskCodexHooksStatus } from "@/app/actions/project";
import { getSidebarDefaultCollapsed, getSidebarHintDismissed, getDoneAlertDismissed } from "@/app/actions/appSettings";
import { Link } from "@/i18n/navigation";

export const dynamicConfig = "force-dynamic";

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

        {/* 제목 + 상태 카드 */}
        <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
          <div className="flex items-center gap-2 mb-3">
            <TaskStatusBadge status={task.status} />
          </div>
          <h1 className="text-xl font-bold text-text-primary leading-tight">
            {task.title}
          </h1>
          {task.description && (
            <p className="text-sm text-text-secondary mt-3 leading-relaxed">
              {task.description}
            </p>
          )}
        </div>

        {/* 메타데이터 카드 */}
        <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
          <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-4">
            {t("info")}
          </h3>
          <dl className="space-y-3">
            {task.project && (
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("project")}</dt>
                <dd className="text-xs px-2 py-0.5 rounded-full font-medium bg-tag-project-bg text-tag-project-text truncate max-w-[160px]">
                  {task.project.name}
                </dd>
              </div>
            )}
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("priority")}</dt>
              <dd>
                <PriorityEditor taskId={task.id} currentPriority={task.priority} />
              </dd>
            </div>
            {task.prUrl && (
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("prLink")}</dt>
                <dd>
                  <a
                    href={task.prUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-xs bg-tag-pr-bg text-tag-pr-text px-2 py-0.5 rounded hover:opacity-80 transition-opacity"
                  >
                    <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                      <path d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z"/>
                    </svg>
                    PR
                    <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
                      <path d="M4 12L12 4M12 4H6M12 4v6" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  </a>
                </dd>
              </div>
            )}
            {task.agentType && (
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("agent")}</dt>
                <dd
                  className={`text-xs px-2 py-0.5 rounded-full font-medium ${agentTagStyle}`}
                >
                  {task.agentType}
                </dd>
              </div>
            )}
            {task.sessionType && (
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("session")}</dt>
                <dd className="text-xs bg-tag-session-bg text-tag-session-text px-2 py-0.5 rounded-full">
                  {task.sessionType}
                </dd>
              </div>
            )}
            {task.sshHost && (
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("sshHost")}</dt>
                <dd className="text-xs font-mono bg-tag-ssh-bg text-tag-ssh-text px-2 py-0.5 rounded">
                  {task.sshHost}
                </dd>
              </div>
            )}
            <div className="border-t border-border-subtle pt-3 mt-3">
              <div className="flex items-center justify-between gap-2 mb-2">
                <dt className="text-xs text-text-muted">{t("createdAt")}</dt>
                <dd className="text-xs text-text-secondary">
                  {new Date(task.createdAt).toLocaleDateString()}
                </dd>
              </div>
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("updatedAt")}</dt>
                <dd className="text-xs text-text-secondary">
                  {new Date(task.updatedAt).toLocaleDateString()}
                </dd>
              </div>
            </div>
          </dl>
        </div>

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
        {task.projectId && (
          <HooksStatusCard
            taskId={task.id}
            initialClaudeStatus={claudeHooksStatus}
            initialGeminiStatus={geminiHooksStatus}
            initialCodexStatus={codexHooksStatus}
            isRemote={!!task.sshHost}
          />
        )}
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
