"use client";

import { useEffect, useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  installTaskHooks,
  installTaskGeminiHooks,
  installTaskCodexHooks,
  installTaskOpenCodeHooks,
} from "@/desktop/renderer/actions/project";
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

interface StatusCheck {
  label: string;
  ok: boolean;
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

  useEffect(() => {
    setLocalClaudeStatus(claudeStatus);
  }, [claudeStatus]);

  useEffect(() => {
    setLocalGeminiStatus(geminiStatus);
  }, [geminiStatus]);

  useEffect(() => {
    setLocalCodexStatus(codexStatus);
  }, [codexStatus]);

  useEffect(() => {
    setLocalOpenCodeStatus(openCodeStatus);
  }, [openCodeStatus]);

  useEffect(() => {
    setMessage(null);
  }, [isOpen, claudeStatus, geminiStatus, codexStatus, openCodeStatus]);

  function getInstallFailureText(error?: string) {
    return error ? t("hooksInstallFailed", { error }) : t("hooksInstallFailed");
  }

  function getInstallIncompleteText() {
    return t("hooksInstallIncomplete");
  }

  function renderChecks(checks: StatusCheck[]) {
    return (
      <div className="mt-1 flex flex-wrap gap-1">
        {checks.map((check) => (
          <span
            key={check.label}
            className={`px-1.5 py-0.5 rounded text-[10px] border ${check.ok
              ? "bg-status-done/10 text-status-done border-status-done/20"
              : "bg-status-error/10 text-status-error border-status-error/20"
            }`}
          >
            {check.label}
          </span>
        ))}
      </div>
    );
  }

  function applyClaudeResult(result: Awaited<ReturnType<typeof installTaskHooks>>) {
    if (result.success && result.status) {
      setLocalClaudeStatus(result.status);
      setMessage({
        type: result.status.installed ? "success" : "error",
        text: result.status.installed ? t("hooksInstallSuccess") : getInstallIncompleteText(),
      });
      return;
    }

    setMessage({ type: "error", text: getInstallFailureText(result.error) });
  }

  function applyGeminiResult(result: Awaited<ReturnType<typeof installTaskGeminiHooks>>) {
    if (result.success && result.status) {
      setLocalGeminiStatus(result.status);
      setMessage({
        type: result.status.installed ? "success" : "error",
        text: result.status.installed ? t("geminiHooksInstallSuccess") : getInstallIncompleteText(),
      });
      return;
    }

    setMessage({ type: "error", text: getInstallFailureText(result.error) });
  }

  function applyCodexResult(result: Awaited<ReturnType<typeof installTaskCodexHooks>>) {
    if (result.success && result.status) {
      setLocalCodexStatus(result.status);
      setMessage({
        type: result.status.installed ? "success" : "error",
        text: result.status.installed ? t("codexHooksInstallSuccess") : getInstallIncompleteText(),
      });
      return;
    }

    setMessage({ type: "error", text: getInstallFailureText(result.error) });
  }

  function applyOpenCodeResult(result: Awaited<ReturnType<typeof installTaskOpenCodeHooks>>) {
    if (result.success && result.status) {
      setLocalOpenCodeStatus(result.status);
      setMessage({
        type: result.status.installed ? "success" : "error",
        text: result.status.installed ? t("openCodeHooksInstallSuccess") : getInstallIncompleteText(),
      });
      return;
    }

    setMessage({ type: "error", text: getInstallFailureText(result.error) });
  }

  if (!isOpen) return null;

  function handleInstallClaude() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskHooks(taskId);
      applyClaudeResult(result);
    });
  }

  function handleInstallGemini() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskGeminiHooks(taskId);
      applyGeminiResult(result);
    });
  }

  function handleInstallCodex() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskCodexHooks(taskId);
      applyCodexResult(result);
    });
  }

  function handleInstallOpenCode() {
    setMessage(null);
    startTransition(async () => {
      const result = await installTaskOpenCodeHooks(taskId);
      applyOpenCodeResult(result);
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
                  {isPending ? t("installingHooks") : t("hooksStatusDialog.reinstall")}
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
          {localClaudeStatus && renderChecks([
            { label: "prompt", ok: !!localClaudeStatus.hasPromptHook },
            { label: "stop", ok: !!localClaudeStatus.hasStopHook },
            { label: "question", ok: !!localClaudeStatus.hasQuestionHook },
            { label: "settings", ok: !!localClaudeStatus.hasSettingsEntry },
            { label: "task id", ok: !!localClaudeStatus.hasTaskIdBinding },
            { label: "mapping", ok: !!localClaudeStatus.hasStatusMappings },
          ])}

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
                  {isPending ? t("installingHooks") : t("hooksStatusDialog.reinstall")}
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
          {localGeminiStatus && renderChecks([
            { label: "prompt", ok: !!localGeminiStatus.hasPromptHook },
            { label: "stop", ok: !!localGeminiStatus.hasStopHook },
            { label: "settings", ok: !!localGeminiStatus.hasSettingsEntry },
            { label: "task id", ok: !!localGeminiStatus.hasTaskIdBinding },
            { label: "mapping", ok: !!localGeminiStatus.hasStatusMappings },
          ])}

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
                  {isPending ? t("installingHooks") : t("hooksStatusDialog.reinstall")}
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
          {localCodexStatus && renderChecks([
            { label: "notify", ok: !!localCodexStatus.hasNotifyHook },
            { label: "config", ok: !!localCodexStatus.hasConfigEntry },
            { label: "task id", ok: !!localCodexStatus.hasTaskIdBinding },
            { label: "review", ok: !!localCodexStatus.hasReviewStatus },
            { label: "event filter", ok: !!localCodexStatus.hasAgentTurnCompleteFilter },
          ])}

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
                  {isPending ? t("installingHooks") : t("hooksStatusDialog.reinstall")}
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
          {localOpenCodeStatus && renderChecks([
            { label: "plugin", ok: !!localOpenCodeStatus.hasPlugin },
            { label: "task id", ok: !!localOpenCodeStatus.hasTaskIdBinding },
            { label: "endpoint", ok: !!localOpenCodeStatus.hasStatusEndpoint },
            { label: "mapping", ok: !!localOpenCodeStatus.hasEventMappings },
            { label: "main only", ok: !!localOpenCodeStatus.hasMainSessionGuard },
            { label: "late progress guard", ok: !!localOpenCodeStatus.hasDuplicateProgressGuard },
          ])}
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
