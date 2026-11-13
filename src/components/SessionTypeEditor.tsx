"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import { useRouter } from "@/desktop/renderer/navigation";
import { changeTaskSessionType, isTaskSessionAlive } from "@/desktop/renderer/actions/kanban";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import type { SessionType } from "@/entities/KanbanTask";
import SessionTypeOptions from "./SessionTypeOptions";

interface SessionTypeEditorProps {
  taskId: string;
  currentSessionType: SessionType;
  sshHost: string | null;
}

/** 이미 세션이 있는 task의 터미널 세션 타입을 바꾼다. 켜진 기존 세션은 닫을지 물어본 뒤 처리한다 */
export default function SessionTypeEditor({ taskId, currentSessionType, sshHost }: SessionTypeEditorProps) {
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);

  function handleChange(nextSessionType: SessionType) {
    if (isPending || nextSessionType === currentSessionType) {
      return;
    }

    setError(null);
    startTransition(async () => {
      try {
        const isDependencyReady = await ensureSessionDependencyWithPrompt(nextSessionType, sshHost, tc);
        if (!isDependencyReady) {
          return;
        }

        const isExistingSessionAlive = await isTaskSessionAlive(taskId);
        const shouldCloseExistingSession = isExistingSessionAlive
          && window.confirm(t("closeExistingSessionConfirm", { sessionType: currentSessionType }));
        const changedTask = await changeTaskSessionType(taskId, nextSessionType, shouldCloseExistingSession);
        if (!changedTask) {
          setError(t("sessionTypeChangeFailed"));
          return;
        }

        router.refresh();
      } catch (changeError) {
        setError(changeError instanceof Error ? changeError.message : t("sessionTypeChangeFailed"));
      }
    });
  }

  return (
    <div className="flex flex-col items-end gap-1" aria-busy={isPending}>
      <select
        value={currentSessionType}
        onChange={(event) => handleChange(event.target.value as SessionType)}
        disabled={isPending}
        className="text-xs bg-tag-session-bg text-tag-session-text px-2 py-0.5 rounded-full border-none focus:outline-none focus:ring-1 focus:ring-brand-primary disabled:opacity-50"
      >
        <SessionTypeOptions />
      </select>
      {isPending && <p role="status" className="text-xs text-text-muted">{t("sessionTypeChanging")}</p>}
      {error && <p role="alert" className="text-xs text-status-error">{error}</p>}
    </div>
  );
}
