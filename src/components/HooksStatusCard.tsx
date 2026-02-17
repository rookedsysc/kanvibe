"use client";

import { useTranslations } from "next-intl";
import { useState, useTransition } from "react";
import { installTaskHooks, installTaskGeminiHooks, installTaskCodexHooks } from "@/app/actions/project";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";

interface HooksStatusCardProps {
  taskId: string;
  initialClaudeStatus: ClaudeHooksStatus | null;
  initialGeminiStatus: GeminiHooksStatus | null;
  initialCodexStatus: CodexHooksStatus | null;
  isRemote: boolean;
}

export default function HooksStatusCard({
  taskId,
  initialClaudeStatus,
  initialGeminiStatus,
  initialCodexStatus,
  isRemote,
}: HooksStatusCardProps) {
  const t = useTranslations("taskDetail");
  const [isPending, startTransition] = useTransition();
  const [claudeStatus, setClaudeStatus] = useState(initialClaudeStatus);
  const [geminiStatus, setGeminiStatus] = useState(initialGeminiStatus);
  const [codexStatus, setCodexStatus] = useState(initialCodexStatus);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  function handleInstallClaude() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskHooks(taskId);
      if (result.success) {
        setClaudeStatus({ installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true });
        setMessage({ type: "success", text: t("hooksInstallSuccess") });
      } else {
        setMessage({ type: "error", text: t("hooksInstallFailed") });
      }
    });
  }

  function handleInstallGemini() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskGeminiHooks(taskId);
      if (result.success) {
        setGeminiStatus({ installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true });
        setMessage({ type: "success", text: t("geminiHooksInstallSuccess") });
      } else {
        setMessage({ type: "error", text: t("hooksInstallFailed") });
      }
    });
  }

  function handleInstallCodex() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskCodexHooks(taskId);
      if (result.success) {
        setCodexStatus({ installed: true, hasNotifyHook: true, hasConfigEntry: true });
        setMessage({ type: "success", text: t("codexHooksInstallSuccess") });
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

      <div className="space-y-2">
        {/* Claude Code Hooks */}
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-secondary font-medium">Claude</span>
          {isRemote ? (
            <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
              {t("hooksRemoteNotSupported")}
            </span>
          ) : claudeStatus?.installed ? (
            <>
              <span className="text-xs px-2 py-0.5 bg-status-done/15 text-status-done rounded">
                {t("hooksInstalled")}
              </span>
              <button
                onClick={handleInstallClaude}
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
                onClick={handleInstallClaude}
                disabled={isPending}
                className="px-3 py-1.5 text-xs bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md transition-colors disabled:opacity-50"
              >
                {isPending ? t("installingHooks") : t("installHooks")}
              </button>
            </>
          )}
        </div>

        {/* Gemini CLI Hooks */}
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-secondary font-medium">Gemini</span>
          {isRemote ? (
            <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
              {t("hooksRemoteNotSupported")}
            </span>
          ) : geminiStatus?.installed ? (
            <>
              <span className="text-xs px-2 py-0.5 bg-status-done/15 text-status-done rounded">
                {t("hooksInstalled")}
              </span>
              <button
                onClick={handleInstallGemini}
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
                onClick={handleInstallGemini}
                disabled={isPending}
                className="px-3 py-1.5 text-xs bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md transition-colors disabled:opacity-50"
              >
                {isPending ? t("installingHooks") : t("installHooks")}
              </button>
            </>
          )}
        </div>

        {/* Codex CLI Hooks */}
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-secondary font-medium">Codex</span>
          {isRemote ? (
            <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
              {t("hooksRemoteNotSupported")}
            </span>
          ) : codexStatus?.installed ? (
            <>
              <span className="text-xs px-2 py-0.5 bg-status-done/15 text-status-done rounded">
                {t("hooksInstalled")}
              </span>
              <button
                onClick={handleInstallCodex}
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
                onClick={handleInstallCodex}
                disabled={isPending}
                className="px-3 py-1.5 text-xs bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md transition-colors disabled:opacity-50"
              >
                {isPending ? t("installingHooks") : t("installHooks")}
              </button>
            </>
          )}
        </div>
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
