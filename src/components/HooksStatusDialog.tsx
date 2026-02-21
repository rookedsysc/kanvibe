"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  installTaskHooks,
  installTaskGeminiHooks,
  installTaskCodexHooks,
  installTaskOpenCodeHooks,
} from "@/app/actions/project";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";

interface HooksStatusDialogProps {
  isOpen: boolean;
  onClose: () => void;
  taskId: string;
  claudeStatus: ClaudeHooksStatus | null;
  geminiStatus: GeminiHooksStatus | null;
  codexStatus: CodexHooksStatus | null;
  openCodeStatus: OpenCodeHooksStatus | null;
  isRemote: boolean;
}

export default function HooksStatusDialog({
  isOpen,
  onClose,
  taskId,
  claudeStatus,
  geminiStatus,
  codexStatus,
  openCodeStatus,
  isRemote,
}: HooksStatusDialogProps) {
  const t = useTranslations("taskDetail");
  const [isPending, startTransition] = useTransition();
  const [localClaudeStatus, setLocalClaudeStatus] = useState(claudeStatus);
  const [localGeminiStatus, setLocalGeminiStatus] = useState(geminiStatus);
  const [localCodexStatus, setLocalCodexStatus] = useState(codexStatus);
  const [localOpenCodeStatus, setLocalOpenCodeStatus] = useState(openCodeStatus);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  if (!isOpen) return null;

  function handleInstallClaude() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskHooks(taskId);
      if (result.success) {
        setLocalClaudeStatus({ installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true });
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
        setLocalGeminiStatus({ installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true });
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
        setLocalCodexStatus({ installed: true, hasNotifyHook: true, hasConfigEntry: true });
        setMessage({ type: "success", text: t("codexHooksInstallSuccess") });
      } else {
        setMessage({ type: "error", text: t("hooksInstallFailed") });
      }
    });
  }

  function handleInstallOpenCode() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskOpenCodeHooks(taskId);
      if (result.success) {
        setLocalOpenCodeStatus({ installed: true, hasPlugin: true });
        setMessage({ type: "success", text: t("openCodeHooksInstallSuccess") });
      } else {
        setMessage({ type: "error", text: t("hooksInstallFailed") });
      }
    });
  }

  return (
    <div className="fixed inset-0 z-[500] flex items-center justify-center bg-black/20">
      <div className="w-full max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-text-primary">
            {t("hooksStatus")}
          </h2>
          <button
            onClick={onClose}
            disabled={isPending}
            className="text-text-muted hover:text-text-primary text-lg"
          >
            ×
          </button>
        </div>

        <div className="space-y-2">
          {/* Claude Code Hooks */}
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-text-secondary font-medium">Claude</span>
            {isRemote ? (
              <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
                {t("hooksRemoteNotSupported")}
              </span>
            ) : localClaudeStatus?.installed ? (
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
            ) : localGeminiStatus?.installed ? (
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
            ) : localCodexStatus?.installed ? (
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

          {/* OpenCode Hooks */}
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-text-secondary font-medium">OpenCode</span>
            {isRemote ? (
              <span className="text-xs px-2 py-0.5 bg-bg-page border border-border-default rounded text-text-muted">
                {t("hooksRemoteNotSupported")}
              </span>
            ) : localOpenCodeStatus?.installed ? (
              <>
                <span className="text-xs px-2 py-0.5 bg-status-done/15 text-status-done rounded">
                  {t("hooksInstalled")}
                </span>
                <button
                  onClick={handleInstallOpenCode}
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
                  onClick={handleInstallOpenCode}
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
            className={`text-xs mt-4 ${
              message.type === "success" ? "text-status-done" : "text-status-error"
            }`}
          >
            {message.text}
          </p>
        )}

        <div className="mt-6 flex justify-end">
          <button
            onClick={onClose}
            disabled={isPending}
            className="px-4 py-1.5 text-sm bg-bg-page border border-border-default hover:border-brand-primary text-text-secondary rounded-md transition-colors disabled:opacity-50"
          >
            {t("hooksStatusDialog.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
