"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import { connectTerminalSession } from "@/app/actions/kanban";
import { SessionType } from "@/entities/KanbanTask";

interface ConnectTerminalFormProps {
  taskId: string;
}

/** 세션이 없는 태스크에 터미널 세션을 연결하는 폼 */
export default function ConnectTerminalForm({ taskId }: ConnectTerminalFormProps) {
  const t = useTranslations("taskDetail");
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);

  function handleSubmit(formData: FormData) {
    const sessionType = formData.get("sessionType") as SessionType;
    setError(null);
    startTransition(async () => {
      const result = await connectTerminalSession(taskId, sessionType);
      if (!result) {
        setError(t("connectFailed"));
      }
    });
  }

  return (
    <form action={handleSubmit} className="flex flex-col items-center gap-4">
      <p className="text-text-muted text-sm">{t("noTerminal")}</p>
      <div className="flex items-center gap-3">
        <label className="text-xs text-text-secondary">{t("selectSessionType")}</label>
        <select
          name="sessionType"
          className="px-3 py-1.5 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
        >
          <option value="tmux">tmux</option>
          <option value="zellij">zellij</option>
        </select>
        <button
          type="submit"
          disabled={isPending}
          className="px-4 py-1.5 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
        >
          {isPending ? t("connecting") : t("connectTerminal")}
        </button>
      </div>
      {error && (
        <p className="text-xs text-status-error">{error}</p>
      )}
    </form>
  );
}
