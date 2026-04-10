"use client";

import { useTransition } from "react";
import { useRouter } from "@/desktop/renderer/navigation";
import { TaskPriority } from "@/entities/TaskPriority";
import { updateTask } from "@/desktop/renderer/actions/kanban";
import PrioritySelector from "./PrioritySelector";

interface PriorityEditorProps {
  taskId: string;
  currentPriority: TaskPriority | null;
}

export default function PriorityEditor({ taskId, currentPriority }: PriorityEditorProps) {
  const [isPending, startTransition] = useTransition();
  const router = useRouter();

  function handleChange(priority: TaskPriority | null) {
    startTransition(async () => {
      await updateTask(taskId, { priority });
      router.refresh();
    });
  }

  return (
    <div className={isPending ? "opacity-50 pointer-events-none" : ""}>
      <PrioritySelector value={currentPriority} onChange={handleChange} />
    </div>
  );
}
