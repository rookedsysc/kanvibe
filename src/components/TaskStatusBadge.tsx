import { TaskStatus } from "@/entities/KanbanTask";

interface TaskStatusBadgeProps {
  status: TaskStatus;
}

const statusConfig: Record<TaskStatus, { label: string; className: string }> = {
  [TaskStatus.TODO]: {
    label: "Todo",
    className: "bg-gray-100 text-status-todo",
  },
  [TaskStatus.PROGRESS]: {
    label: "Progress",
    className: "bg-yellow-50 text-status-progress",
  },
  [TaskStatus.PENDING]: {
    label: "Pending",
    className: "bg-purple-50 text-status-pending",
  },
  [TaskStatus.REVIEW]: {
    label: "Review",
    className: "bg-blue-50 text-status-review",
  },
  [TaskStatus.DONE]: {
    label: "Done",
    className: "bg-green-50 text-status-done",
  },
};

export default function TaskStatusBadge({ status }: TaskStatusBadgeProps) {
  const config = statusConfig[status];
  return (
    <span className={`text-xs px-2.5 py-1 rounded-full font-medium ${config.className}`}>
      {config.label}
    </span>
  );
}
