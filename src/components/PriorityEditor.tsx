"use client";

import { useTranslations } from "next-intl";
import { useRouter } from "@/desktop/renderer/navigation";
import { TaskPriority } from "@/entities/TaskPriority";
import { updateTask } from "@/desktop/renderer/actions/kanban";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";
import PrioritySelector from "./PrioritySelector";

interface PriorityEditorProps {
  taskId: string;
  currentPriority: TaskPriority | null;
}

export default function PriorityEditor({ taskId, currentPriority }: PriorityEditorProps) {
  const tc = useTranslations("common");
  const router = useRouter();
  const prioritySave = useTrackedAction(async (priority: TaskPriority | null) => {
    await updateTask(taskId, { priority });
    router.refresh();
  });

  return (
    <div aria-busy={prioritySave.isPending || undefined}>
      {/* fieldset을 막으면 안의 버튼이 모두 실제로 막혀 키보드로도 겹쳐 저장하지 못한다 */}
      <fieldset disabled={prioritySave.isPending} className="m-0 min-w-0 border-0 p-0 disabled:opacity-50">
        <PrioritySelector value={currentPriority} onChange={(priority) => void prioritySave.run(priority)} />
      </fieldset>
      {prioritySave.isPending ? (
        <p role="status" className="mt-1 text-[11px] text-text-muted">{tc("saving")}</p>
      ) : null}
      {prioritySave.error ? (
        <p role="alert" className="mt-1 text-[11px] text-status-error">
          {tc("saveFailed", { reason: prioritySave.error })}
        </p>
      ) : null}
    </div>
  );
}
