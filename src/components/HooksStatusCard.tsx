"use client";

import { useEffect, useRef, useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  AlertCircleIcon,
  CheckmarkCircle02Icon,
  Clock01Icon,
} from "@hugeicons/core-free-icons";
import { useTranslations } from "next-intl";
import {
  getTaskHooksStatuses,
  installTaskCodexHooks,
  installTaskGeminiHooks,
  installTaskHooks,
  installTaskOpenCodeHooks,
} from "@/desktop/renderer/actions/project";
import { AiProviderIcon, type AiProviderIconName } from "@/components/AiProviderIcon";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";

interface HooksStatusCardProps {
  taskId: string;
  initialClaudeStatus: ClaudeHooksStatus | null;
  initialGeminiStatus: GeminiHooksStatus | null;
  initialCodexStatus: CodexHooksStatus | null;
  initialOpenCodeStatus: OpenCodeHooksStatus | null;
  isRemote: boolean;
  /** 상태를 읽는 중이면 true. 읽기 전의 빈 상태가 미설치로만 보이지 않게 한다 */
  isLoading?: boolean;
  onStatusesChange?: (updates: {
    claudeStatus?: ClaudeHooksStatus | null;
    geminiStatus?: GeminiHooksStatus | null;
    codexStatus?: CodexHooksStatus | null;
    openCodeStatus?: OpenCodeHooksStatus | null;
  }) => void;
}

type HookToolKey = "claude" | "gemini" | "codex" | "openCode";
type InstallMessage = { type: "success" | "error"; text: string; retry?: () => void };
type HookStatusUpdates = {
  claudeStatus?: ClaudeHooksStatus | null;
  geminiStatus?: GeminiHooksStatus | null;
  codexStatus?: CodexHooksStatus | null;
  openCodeStatus?: OpenCodeHooksStatus | null;
};

export default function HooksStatusCard({
  taskId,
  initialClaudeStatus,
  initialGeminiStatus,
  initialCodexStatus,
  initialOpenCodeStatus,
  isLoading = false,
  onStatusesChange,
}: HooksStatusCardProps) {
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");
  const [claudeStatus, setClaudeStatus] = useState(initialClaudeStatus);
  const [geminiStatus, setGeminiStatus] = useState(initialGeminiStatus);
  const [codexStatus, setCodexStatus] = useState(initialCodexStatus);
  const [openCodeStatus, setOpenCodeStatus] = useState(initialOpenCodeStatus);
  const [installingTools, setInstallingTools] = useState<HookToolKey[]>([]);
  const [message, setMessage] = useState<InstallMessage | null>(null);
  const onStatusesChangeRef = useRef(onStatusesChange);

  useEffect(() => {
    onStatusesChangeRef.current = onStatusesChange;
  }, [onStatusesChange]);

  useEffect(() => {
    setClaudeStatus(initialClaudeStatus);
  }, [initialClaudeStatus]);

  useEffect(() => {
    setGeminiStatus(initialGeminiStatus);
  }, [initialGeminiStatus]);

  useEffect(() => {
    setCodexStatus(initialCodexStatus);
  }, [initialCodexStatus]);

  useEffect(() => {
    setOpenCodeStatus(initialOpenCodeStatus);
  }, [initialOpenCodeStatus]);

  const hookItems = [
    {
      key: "claude" as const,
      title: "Claude",
      status: claudeStatus,
      install: () => runInstall("claude", () => installTaskHooks(taskId), (result) => {
        if (result.success && result.status) {
          setClaudeStatus(result.status);
          onStatusesChange?.({ claudeStatus: result.status });
          setMessage(getResultMessage(result.status.installed, t("hooksInstallSuccess"), t));
          return;
        }

        setMessage({ type: "error", text: getInstallFailureText(t, result.error) });
      }),
    },
    {
      key: "gemini" as const,
      title: "Gemini",
      status: geminiStatus,
      install: () => runInstall("gemini", () => installTaskGeminiHooks(taskId), (result) => {
        if (result.success && result.status) {
          setGeminiStatus(result.status);
          onStatusesChange?.({ geminiStatus: result.status });
          setMessage(getResultMessage(result.status.installed, t("geminiHooksInstallSuccess"), t));
          return;
        }

        setMessage({ type: "error", text: getInstallFailureText(t, result.error) });
      }),
    },
    {
      key: "codex" as const,
      title: "Codex",
      status: codexStatus,
      install: () => runInstall("codex", () => installTaskCodexHooks(taskId), (result) => {
        if (result.success && result.status) {
          setCodexStatus(result.status);
          onStatusesChange?.({ codexStatus: result.status });
          setMessage(getResultMessage(result.status.installed, t("codexHooksInstallSuccess"), t));
          return;
        }

        setMessage({ type: "error", text: getInstallFailureText(t, result.error) });
      }),
    },
    {
      key: "openCode" as const,
      title: "OpenCode",
      status: openCodeStatus,
      install: () => runInstall("openCode", () => installTaskOpenCodeHooks(taskId), (result) => {
        if (result.success && result.status) {
          setOpenCodeStatus(result.status);
          onStatusesChange?.({ openCodeStatus: result.status });
          setMessage(getResultMessage(result.status.installed, t("openCodeHooksInstallSuccess"), t));
          return;
        }

        setMessage({ type: "error", text: getInstallFailureText(t, result.error) });
      }),
    },
  ];

  const installedCount = hookItems.filter((item) => item.status?.installed === true).length;
  const overallStatus = getOverallStatus(installedCount, hookItems.length);

  async function runInstall<T>(
    tool: HookToolKey,
    install: () => Promise<T>,
    applyResult: (result: T) => void,
  ) {
    setMessage(null);
    setInstallingTools((current) => (current.includes(tool) ? current : [...current, tool]));

    try {
      const result = await install();
      applyResult(result);
      if (isSuccessfulInstallResult(result)) {
        await refreshAllHookStatuses().catch(() => undefined);
      }
    } catch (error) {
      // IPC가 끊기는 등 결과 객체 없이 실패해도 설치 중 표시만 사라지지 않게 사유와 다시 시도를 남긴다
      setMessage({
        type: "error",
        text: getInstallFailureText(t, error instanceof Error ? error.message : String(error)),
        retry: () => void runInstall(tool, install, applyResult),
      });
    } finally {
      setInstallingTools((current) => current.filter((value) => value !== tool));
    }
  }

  /** provider별로 따로 물으면 원격 task에서 SSH 접속이 네 번 차례로 생기므로 한 번에 읽는다 */
  async function refreshAllHookStatuses() {
    const latestStatuses = await getTaskHooksStatuses(taskId);

    applyRefreshedStatuses({
      claudeStatus: latestStatuses?.claude,
      geminiStatus: latestStatuses?.gemini,
      codexStatus: latestStatuses?.codex,
      openCodeStatus: latestStatuses?.openCode,
    });
  }

  function applyRefreshedStatuses(updates: HookStatusUpdates) {
    const parentUpdates: HookStatusUpdates = {};

    if (updates.claudeStatus) {
      setClaudeStatus(updates.claudeStatus);
      parentUpdates.claudeStatus = updates.claudeStatus;
    }
    if (updates.geminiStatus) {
      setGeminiStatus(updates.geminiStatus);
      parentUpdates.geminiStatus = updates.geminiStatus;
    }
    if (updates.codexStatus) {
      setCodexStatus(updates.codexStatus);
      parentUpdates.codexStatus = updates.codexStatus;
    }
    if (updates.openCodeStatus) {
      setOpenCodeStatus(updates.openCodeStatus);
      parentUpdates.openCodeStatus = updates.openCodeStatus;
    }

    if (Object.keys(parentUpdates).length > 0) {
      onStatusesChangeRef.current?.(parentUpdates);
    }
  }

  return (
    <div className="rounded-lg border border-border-default bg-bg-surface p-4 shadow-sm">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
            {t("hooksStatus")}
          </h3>
          <p className="mt-1 text-xs text-text-muted">{t("hooksCurrentTaskId", { taskId })}</p>
        </div>
        <span className={`inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2 py-1 text-[11px] font-medium ${overallStatus.className}`}>
          <StatusIcon status={overallStatus.kind} />
          {installedCount}/{hookItems.length}
        </span>
      </div>

      {isLoading ? (
        <p role="status" className="mb-3 text-xs text-text-muted">{tc("loading")}</p>
      ) : null}

      {message ? (
        <div role={message.type === "error" ? "alert" : "status"} className={`mb-3 rounded-md border px-3 py-2 text-xs ${message.type === "success"
          ? "border-status-done/20 bg-status-done/10 text-status-done"
          : "border-status-error/20 bg-status-error/10 text-status-error"
        }`}>
          {message.text}
          {message.retry ? (
            <button
              type="button"
              onClick={message.retry}
              className="ml-2 rounded px-1.5 py-0.5 font-medium underline-offset-2 hover:underline focus-visible:outline focus-visible:outline-1 focus-visible:outline-border-brand"
            >
              {tc("retry")}
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="space-y-2">
        {hookItems.map((item) => {
          const registrationFailed = item.key === "openCode" && Boolean(openCodeStatus?.pluginRegistrationError);
          const isInstalled = item.status?.installed === true;
          const isInstalling = installingTools.includes(item.key);
          const actionLabel = isInstalling
            ? t("installingHooks")
            : isInstalled
              ? t("hooksStatusDialog.reinstall")
              : t("installHooks");

          return (
            <section key={item.key} className="rounded-md border border-border-default bg-bg-page/60 p-3">
              <div className="flex items-center gap-3">
                <span className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md border ${isInstalled
                  ? "border-status-done/20 bg-status-done/10 text-status-done"
                  : "border-status-error/20 bg-status-error/10 text-status-error"
                }`}>
                  <HookToolIcon tool={item.key} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 items-center gap-2">
                    <h4 className="truncate text-sm font-semibold text-text-primary">{item.title}</h4>
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${isInstalled
                      ? "bg-status-done/15 text-status-done"
                      : "bg-status-error/15 text-status-error"
                    }`}>
                      {registrationFailed ? t("hooksVerificationFailed") : isInstalled ? t("hooksInstalled") : t("hooksNotInstalled")}
                    </span>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    if (!isInstalling) {
                      void item.install();
                    }
                  }}
                  disabled={isInstalling}
                  aria-label={`${actionLabel} ${item.title}`}
                  className={`shrink-0 rounded-md px-3 py-1.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${isInstalled
                    ? "border border-border-default bg-bg-surface text-text-secondary hover:border-brand-primary hover:text-text-primary"
                    : "bg-brand-primary text-text-inverse hover:bg-brand-hover"
                  }`}
                >
                  {actionLabel}
                </button>
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function isSuccessfulInstallResult(result: unknown): result is { success: true } {
  return result !== null
    && typeof result === "object"
    && "success" in result
    && (result as { success?: unknown }).success === true;
}

function getInstallFailureText(t: ReturnType<typeof useTranslations>, error?: string) {
  return error ? t("hooksInstallFailed", { error }) : t("hooksInstallFailed");
}

function getResultMessage(installed: boolean, successText: string, t: ReturnType<typeof useTranslations>): InstallMessage {
  return {
    type: installed ? "success" : "error",
    text: installed ? successText : t("hooksInstallIncomplete"),
  };
}

function getOverallStatus(installedCount: number, totalCount: number) {
  if (installedCount === totalCount) {
    return {
      kind: "ok" as const,
      className: "border-status-done/20 bg-status-done/15 text-status-done",
    };
  }

  if (installedCount === 0) {
    return {
      kind: "error" as const,
      className: "border-status-error/20 bg-status-error/15 text-status-error",
    };
  }

  return {
    kind: "partial" as const,
    className: "border-status-warning/20 bg-status-warning/15 text-status-warning",
  };
}

function StatusIcon({ status }: { status: "ok" | "partial" | "error" }) {
  const icon = status === "ok"
    ? CheckmarkCircle02Icon
    : status === "partial"
      ? Clock01Icon
      : AlertCircleIcon;

  return (
    <HugeiconsIcon
      icon={icon}
      size={13}
      strokeWidth={2}
      aria-hidden="true"
      data-testid="hooks-overall-status-icon"
    />
  );
}

function HookToolIcon({ tool }: { tool: HookToolKey }) {
  const providerByTool: Record<HookToolKey, AiProviderIconName> = {
    claude: "claude",
    gemini: "gemini",
    codex: "codex",
    openCode: "opencode",
  };

  return (
    <AiProviderIcon
      provider={providerByTool[tool]}
      testId="hook-status-tool-icon"
      className="inline-flex h-[18px] w-[18px] items-center justify-center"
      imageClassName="block h-[18px] w-[18px] object-contain"
      size={18}
    />
  );
}
