import { notFound } from "next/navigation";
import { getTranslations } from "next-intl/server";
import { getTaskById, updateTaskStatus, deleteTask } from "@/app/actions/kanban";
import { TaskStatus } from "@/entities/KanbanTask";
import TaskStatusBadge from "@/components/TaskStatusBadge";
import TerminalLoader from "@/components/TerminalLoader";
import { Link } from "@/i18n/navigation";

export const dynamicConfig = "force-dynamic";

interface TaskDetailPageProps {
  params: Promise<{ id: string }>;
}

export default async function TaskDetailPage({ params }: TaskDetailPageProps) {
  const { id } = await params;
  const task = await getTaskById(id);
  const t = await getTranslations("taskDetail");

  if (!task) notFound();

  const hasTerminal = task.sessionType && task.sessionName;

  const STATUS_TRANSITIONS: { status: TaskStatus; labelKey: string }[] = [
    { status: TaskStatus.TODO, labelKey: "moveToTodo" },
    { status: TaskStatus.PROGRESS, labelKey: "moveToProgress" },
    { status: TaskStatus.REVIEW, labelKey: "moveToReview" },
    { status: TaskStatus.DONE, labelKey: "moveToDone" },
  ];

  async function handleStatusChange(formData: FormData) {
    "use server";
    const newStatus = formData.get("status") as TaskStatus;
    await updateTaskStatus(id, newStatus);
  }

  async function handleDelete() {
    "use server";
    await deleteTask(id);
  }

  return (
    <div className="h-screen flex flex-col">
      <header className="flex items-center gap-4 px-6 py-4 border-b border-border-default bg-bg-surface shrink-0">
        <Link
          href="/"
          className="text-text-secondary hover:text-text-primary transition-colors"
        >
          &larr; {t("backToBoard")}
        </Link>
      </header>

      <main className="flex-1 flex flex-col min-h-0 p-4">
        <div className="flex items-start justify-between mb-4 shrink-0">
          <div>
            <div className="flex items-center gap-3 mb-2">
              <h1 className="text-2xl font-bold text-text-primary">
                {task.title}
              </h1>
              <TaskStatusBadge status={task.status} />
            </div>

            {task.description && (
              <p className="text-text-secondary mt-2">{task.description}</p>
            )}

            <div className="flex flex-wrap gap-3 mt-4 text-sm text-text-secondary">
              {task.branchName && (
                <span className="font-mono bg-tag-branch-bg text-tag-branch-text px-2 py-1 rounded-md">
                  {task.branchName}
                </span>
              )}
              {task.agentType && (
                <span>Agent: {task.agentType}</span>
              )}
              {task.sessionType && (
                <span>Session: {task.sessionType}</span>
              )}
              {task.sshHost && <span>SSH: {task.sshHost}</span>}
            </div>
          </div>

          <div className="flex items-center gap-2">
            {STATUS_TRANSITIONS.filter(
              (transition) => transition.status !== task.status
            ).map((transition) => (
              <form key={transition.status} action={handleStatusChange}>
                <input
                  type="hidden"
                  name="status"
                  value={transition.status}
                />
                <button
                  type="submit"
                  className="px-3 py-1.5 text-xs bg-bg-surface border border-border-default hover:border-border-strong text-text-secondary rounded-md transition-colors"
                >
                  {t(transition.labelKey)}
                </button>
              </form>
            ))}
            <form action={handleDelete}>
              <button
                type="submit"
                className="px-3 py-1.5 text-xs bg-red-50 hover:bg-red-100 text-status-error rounded-md transition-colors"
              >
                {t("delete")}
              </button>
            </form>
          </div>
        </div>

        {hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0">
            <h2 className="text-lg font-semibold mb-3 text-text-primary shrink-0">
              {t("terminal")}
            </h2>
            <div className="flex-1 min-h-0">
              <TerminalLoader taskId={task.id} />
            </div>
          </div>
        ) : (
          <div className="text-center py-20 text-text-muted border border-dashed border-border-default rounded-xl">
            {t("noTerminal")}
          </div>
        )}
      </main>
    </div>
  );
}
