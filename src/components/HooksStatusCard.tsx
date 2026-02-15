"use client";

import { useTranslations } from "next-intl";
import { useState, useTransition } from "react";
import { installTaskHooks } from "@/app/actions/project";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";

interface HooksStatusCardProps {
  taskId: string;
  initialStatus: ClaudeHooksStatus | null;
  isRemote: boolean;
}

export default function HooksStatusCard({
  taskId,
  initialStatus,
  isRemote,
}: HooksStatusCardProps) {
  const t = useTranslations("taskDetail");
  const [isPending, startTransition] = useTransition();
  const [hooksStatus, setHooksStatus] = useState(initialStatus);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  function handleInstall() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskHooks(taskId);
      if (result.success) {
        setHooksStatus({ installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true });
        setMessage({ type: "success", text: t("hooksInstallSuccess") });
      } else {
        setMessage({ type: "error", text: t("hooksInstallFailed") });
      }
    });
  }

  return (
    <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
      <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-3">
        {t("hooksStatus")}
      </h3>

      <div className="flex items-center justify-between gap-2">
        {isRemote ? (
          <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
            {t("hooksRemoteNotSupported")}
          </span>
        ) : hooksStatus?.installed ? (
          <>
            <span className="text-xs px-2 py-0.5 bg-status-done/15 text-status-done rounded">
              {t("hooksInstalled")}
            </span>
            <button
              onClick={handleInstall}
              disabled={isPending}
              className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors disabled:opacity-50"
            >
              {isPending ? t("installingHooks") : t("installHooks")}
            </button>
          </>
        ) : (
          <>
            <span className="text-xs px-2 py-0.5 bg-status-error/15 text-status-error rounded">
              {t("hooksNotInstalled")}
            </span>
            <button
              onClick={handleInstall}
              disabled={isPending}
              className="px-3 py-1.5 text-xs bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md transition-colors disabled:opacity-50"
            >
              {isPending ? t("installingHooks") : t("installHooks")}
            </button>
          </>
        )}
      </div>

      {message && (
        <p
          className={`text-xs mt-2 ${
            message.type === "success" ? "text-status-done" : "text-status-error"
          }`}
        >
          {message.text}
        </p>
      )}
    </div>
  );
}
