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
  getTaskOpenCodeHooksStatus,
  installTaskCodexHooks,
  installTaskGeminiHooks,
  installTaskHooks,
  installTaskOpenCodeHooks,
} from "@/desktop/renderer/actions/project";
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
  onStatusesChange?: (updates: {
    claudeStatus?: ClaudeHooksStatus | null;
    geminiStatus?: GeminiHooksStatus | null;
    codexStatus?: CodexHooksStatus | null;
    openCodeStatus?: OpenCodeHooksStatus | null;
  }) => void;
}

type HookToolKey = "claude" | "gemini" | "codex" | "openCode";
type InstallMessage = { type: "success" | "error"; text: string };

export default function HooksStatusCard({
  taskId,
  initialClaudeStatus,
  initialGeminiStatus,
  initialCodexStatus,
  initialOpenCodeStatus,
  isRemote,
  onStatusesChange,
}: HooksStatusCardProps) {
  const t = useTranslations("taskDetail");
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

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      const latestStatus = await getTaskOpenCodeHooksStatus(taskId);
      if (cancelled) {
        return;
      }

      setOpenCodeStatus(latestStatus);
      onStatusesChangeRef.current?.({ openCodeStatus: latestStatus });
    })();

    return () => {
      cancelled = true;
    };
  }, [taskId]);

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
    } finally {
      setInstallingTools((current) => current.filter((value) => value !== tool));
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

      {message ? (
        <div className={`mb-3 rounded-md border px-3 py-2 text-xs ${message.type === "success"
          ? "border-status-done/20 bg-status-done/10 text-status-done"
          : "border-status-error/20 bg-status-error/10 text-status-error"
        }`}>
          {message.text}
        </div>
      ) : null}

      <div className="space-y-2">
        {hookItems.map((item) => {
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
                      {isInstalled ? t("hooksInstalled") : t("hooksNotInstalled")}
                    </span>
                  </div>
                  {isRemote ? <p className="mt-0.5 text-xs text-text-muted">{t("hooksRemoteNotSupported")}</p> : null}
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
  const iconNameByTool: Record<HookToolKey, string> = {
    claude: "ClaudeIcon",
    gemini: "GeminiIcon",
    codex: "CodexIcon",
    openCode: "OpenCodeIcon",
  };

  const commonProps = {
    width: 17,
    height: 17,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.9,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
    "data-testid": "hook-status-tool-icon",
    "data-icon-name": iconNameByTool[tool],
  };

  if (tool === "claude") {
    return (
      <svg {...commonProps}>
        <path d="M12 3.5v17" />
        <path d="M5.1 7.5l13.8 9" />
        <path d="M18.9 7.5l-13.8 9" />
        <circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none" />
      </svg>
    );
  }

  if (tool === "gemini") {
    return (
      <svg {...commonProps}>
        <path d="M12 3l1.6 5.4L19 10l-5.4 1.6L12 17l-1.6-5.4L5 10l5.4-1.6L12 3Z" fill="currentColor" stroke="none" />
        <path d="M18 15l.7 2.3L21 18l-2.3.7L18 21l-.7-2.3L15 18l2.3-.7L18 15Z" fill="currentColor" stroke="none" />
      </svg>
    );
  }

  if (tool === "codex") {
    return (
      <svg {...commonProps}>
        <path d="M7.5 5 4 8.5 7.5 12" />
        <path d="M16.5 5 20 8.5 16.5 12" />
        <path d="M9 19h6" />
        <path d="M12 12v7" />
        <circle cx="12" cy="8.5" r="1.8" fill="currentColor" stroke="none" />
      </svg>
    );
  }

  return (
    <svg {...commonProps}>
      <rect x="4" y="5" width="16" height="14" rx="3" />
      <path d="m8 10 2.2 2L8 14" />
      <path d="M13 14h3" />
    </svg>
  );
}
