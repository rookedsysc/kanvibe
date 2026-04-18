"use client";

import { useEffect, useMemo, useState } from "react";
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

type HookToolKey = "claude" | "gemini" | "codex" | "openCode";

const TASK_ID_FILE_PATH = ".kanvibe/task-id";

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
  const [installingTool, setInstallingTool] = useState<HookToolKey | null>(null);
  const [expandedManualTool, setExpandedManualTool] = useState<HookToolKey | null>(null);
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
    setExpandedManualTool(null);
  }, [isOpen, claudeStatus, geminiStatus, codexStatus, openCodeStatus]);

  function getInstallFailureText(error?: string) {
    return error ? t("hooksInstallFailed", { error }) : t("hooksInstallFailed");
  }

  function getInstallIncompleteText() {
    return t("hooksInstallIncomplete");
  }

  function renderChecks(checks: StatusCheck[]) {
    return (
      <div className="mt-3 flex flex-wrap gap-1.5">
        {checks.map((check) => (
          <span
            key={check.label}
            className={`rounded-full border px-2 py-1 text-[11px] ${check.ok
              ? "border-status-done/20 bg-status-done/10 text-status-done"
              : "border-status-error/20 bg-status-error/10 text-status-error"
            }`}
          >
            {check.label}
          </span>
        ))}
      </div>
    );
  }

  function buildManualBindingCommand(currentTaskId: string) {
    return [
      "mkdir -p .kanvibe",
      `printf '%s\\n' '${currentTaskId}' > ${TASK_ID_FILE_PATH}`,
    ].join("\n");
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

  async function runInstall<T>(
    tool: HookToolKey,
    install: () => Promise<T>,
    applyResult: (result: T) => void,
  ) {
    setMessage(null);
    setInstallingTool(tool);

    try {
      const result = await install();
      applyResult(result);
    } finally {
      setInstallingTool(null);
    }
  }

  const manualBindingCommand = useMemo(() => buildManualBindingCommand(taskId), [taskId]);

  const hookItems = [
    {
      key: "claude" as const,
      title: "Claude",
      status: localClaudeStatus,
      checks: localClaudeStatus ? [
        { label: "prompt", ok: !!localClaudeStatus.hasPromptHook },
        { label: "stop", ok: !!localClaudeStatus.hasStopHook },
        { label: "question", ok: !!localClaudeStatus.hasQuestionHook },
        { label: "settings", ok: !!localClaudeStatus.hasSettingsEntry },
        { label: "taskId", ok: !!localClaudeStatus.hasTaskIdBinding },
        { label: "mapping", ok: !!localClaudeStatus.hasStatusMappings },
      ] : [],
      files: [
        TASK_ID_FILE_PATH,
        ".claude/hooks/kanvibe-prompt-hook.sh",
        ".claude/hooks/kanvibe-question-hook.sh",
        ".claude/hooks/kanvibe-stop-hook.sh",
        ".claude/settings.json",
      ],
      onInstall: () => runInstall("claude", () => installTaskHooks(taskId), applyClaudeResult),
    },
    {
      key: "gemini" as const,
      title: "Gemini",
      status: localGeminiStatus,
      checks: localGeminiStatus ? [
        { label: "prompt", ok: !!localGeminiStatus.hasPromptHook },
        { label: "stop", ok: !!localGeminiStatus.hasStopHook },
        { label: "settings", ok: !!localGeminiStatus.hasSettingsEntry },
        { label: "taskId", ok: !!localGeminiStatus.hasTaskIdBinding },
        { label: "mapping", ok: !!localGeminiStatus.hasStatusMappings },
      ] : [],
      files: [
        TASK_ID_FILE_PATH,
        ".gemini/hooks/kanvibe-prompt-hook.sh",
        ".gemini/hooks/kanvibe-stop-hook.sh",
        ".gemini/settings.json",
      ],
      onInstall: () => runInstall("gemini", () => installTaskGeminiHooks(taskId), applyGeminiResult),
    },
    {
      key: "codex" as const,
      title: "Codex",
      status: localCodexStatus,
      checks: localCodexStatus ? [
        { label: "notify", ok: !!localCodexStatus.hasNotifyHook },
        { label: "config", ok: !!localCodexStatus.hasConfigEntry },
        { label: "taskId", ok: !!localCodexStatus.hasTaskIdBinding },
        { label: "review", ok: !!localCodexStatus.hasReviewStatus },
        { label: "event", ok: !!localCodexStatus.hasAgentTurnCompleteFilter },
      ] : [],
      files: [
        TASK_ID_FILE_PATH,
        ".codex/hooks/kanvibe-notify-hook.sh",
        ".codex/config.toml",
      ],
      onInstall: () => runInstall("codex", () => installTaskCodexHooks(taskId), applyCodexResult),
    },
    {
      key: "openCode" as const,
      title: "OpenCode",
      status: localOpenCodeStatus,
      checks: localOpenCodeStatus ? [
        { label: "plugin", ok: !!localOpenCodeStatus.hasPlugin },
        { label: "taskId", ok: !!localOpenCodeStatus.hasTaskIdBinding },
        { label: "endpoint", ok: !!localOpenCodeStatus.hasStatusEndpoint },
        { label: "mapping", ok: !!localOpenCodeStatus.hasEventMappings },
        { label: "main only", ok: !!localOpenCodeStatus.hasMainSessionGuard },
        { label: "dedupe", ok: !!localOpenCodeStatus.hasDuplicateProgressGuard },
      ] : [],
      files: [
        TASK_ID_FILE_PATH,
        ".opencode/plugins/kanvibe-plugin.ts",
      ],
      onInstall: () => runInstall("openCode", () => installTaskOpenCodeHooks(taskId), applyOpenCodeResult),
    },
  ];

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[500] flex items-center justify-center bg-black/30 px-4 py-6">
      <div className="max-h-[90vh] w-full max-w-5xl overflow-y-auto rounded-2xl border border-border-default bg-bg-surface p-6 shadow-xl">
        <div className="mb-4 flex items-center justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold text-text-primary">{t("hooksStatus")}</h2>
            <p className="mt-1 text-sm text-text-muted">{t("hooksCurrentTaskId", { taskId })}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={installingTool !== null}
            className="text-lg text-text-muted transition-colors hover:text-text-primary disabled:opacity-50"
          >
            ×
          </button>
        </div>

        {message ? (
          <div className={`mb-4 rounded-lg border px-4 py-3 text-sm ${message.type === "success"
            ? "border-status-done/20 bg-status-done/10 text-status-done"
            : "border-status-error/20 bg-status-error/10 text-status-error"
          }`}>
            {message.text}
          </div>
        ) : null}

        <div className="grid gap-4 md:grid-cols-2">
          {hookItems.map((item) => {
            const isInstalled = item.status?.installed === true;
            const isInstalling = installingTool === item.key;
            const detectedTaskId = item.status?.boundTaskId ?? null;
            const hasTaskIdMismatch = Boolean(detectedTaskId && detectedTaskId !== taskId);

            return (
              <section key={item.key} className="rounded-xl border border-border-default bg-bg-page/40 p-4">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h3 className="text-sm font-semibold text-text-primary">{item.title}</h3>
                    <p className="mt-1 text-xs text-text-muted">
                      {isRemote
                        ? t("hooksRemoteNotSupported")
                        : isInstalled
                          ? t("hooksInstalled")
                          : t("hooksNotInstalled")}
                    </p>
                  </div>
                  <span className={`rounded-full border px-2 py-1 text-[11px] font-medium ${isRemote
                    ? "border-border-default bg-bg-surface text-text-muted"
                    : isInstalled
                      ? "border-status-done/20 bg-status-done/15 text-status-done"
                      : "border-status-error/20 bg-status-error/15 text-status-error"
                  }`}>
                    {isRemote
                      ? t("hooksRemoteNotSupported")
                      : isInstalled
                        ? t("hooksInstalled")
                        : t("hooksNotInstalled")}
                  </span>
                </div>

                {item.checks.length > 0 ? renderChecks(item.checks) : null}

                <div className="mt-3 rounded-lg border border-border-subtle bg-bg-surface px-3 py-2 text-[11px] text-text-secondary">
                  <p>{t("hooksCurrentTaskId", { taskId })}</p>
                  {detectedTaskId ? (
                    <p className={hasTaskIdMismatch ? "mt-1 text-status-error" : "mt-1 text-text-muted"}>
                      {t("hooksBoundTaskId", { taskId: detectedTaskId })}
                    </p>
                  ) : null}
                </div>

                {!isRemote ? (
                  <div className="mt-4 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={item.onInstall}
                      disabled={installingTool !== null}
                      className={`rounded-md px-3 py-1.5 text-xs transition-colors disabled:opacity-50 ${isInstalled
                        ? "border border-border-default bg-bg-surface text-text-secondary hover:border-brand-primary hover:text-text-primary"
                        : "bg-brand-primary text-text-inverse hover:bg-brand-hover"
                      }`}
                    >
                      {isInstalling
                        ? t("installingHooks")
                        : isInstalled
                          ? t("hooksStatusDialog.reinstall")
                          : t("installHooks")}
                    </button>
                    {!isInstalled ? (
                      <button
                        type="button"
                        onClick={() => setExpandedManualTool((current) => current === item.key ? null : item.key)}
                        className="rounded-md border border-border-default bg-bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
                      >
                        {expandedManualTool === item.key ? t("close") : t("hooksManualInstallGuide")}
                      </button>
                    ) : null}
                  </div>
                ) : null}

                {expandedManualTool === item.key && !isRemote && !isInstalled ? (
                  <div className="mt-4 rounded-lg border border-border-default bg-bg-surface p-3">
                    <p className="text-xs text-text-secondary">{t("hooksManualInstallDescription", { tool: item.title })}</p>
                    <pre className="mt-3 overflow-x-auto rounded-md bg-[#0f172a] px-3 py-2 text-[11px] text-white">
                      <code>{manualBindingCommand}</code>
                    </pre>
                    <p className="mt-3 text-[11px] font-medium text-text-secondary">{t("hooksManualFiles")}</p>
                    <div className="mt-2 flex flex-wrap gap-1.5">
                      {item.files.map((filePath) => (
                        <span key={filePath} className="rounded-full border border-border-default bg-bg-page px-2 py-1 text-[11px] text-text-muted">
                          {filePath}
                        </span>
                      ))}
                    </div>
                    <p className="mt-3 text-[11px] text-text-muted">{t("hooksManualRecheck")}</p>
                  </div>
                ) : null}
              </section>
            );
          })}
        </div>

        <div className="mt-6 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            disabled={installingTool !== null}
            className="rounded-md border border-border-default bg-bg-page px-4 py-1.5 text-sm text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary disabled:opacity-50"
          >
            {t("hooksStatusDialog.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
