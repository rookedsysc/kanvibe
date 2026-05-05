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
    claude: "ClaudeLogoIcon",
    gemini: "GeminiLogoIcon",
    codex: "CodexLogoIcon",
    openCode: "OpenCodeLogoIcon",
  };

  const commonProps = {
    width: 18,
    height: 18,
    viewBox: "0 0 24 24",
    fill: "none",
    "aria-hidden": true,
    "data-testid": "hook-status-tool-icon",
    "data-icon-name": iconNameByTool[tool],
  };

  if (tool === "claude") {
    return (
      <svg {...commonProps}>
        <path d="M12 2.6c1.15 2.9.92 5.15 0 6.72-.92-1.57-1.15-3.82 0-6.72Z" fill="#111111" />
        <path d="M12 21.4c-1.15-2.9-.92-5.15 0-6.72.92 1.57 1.15 3.82 0 6.72Z" fill="#111111" />
        <path d="M2.6 12c2.9-1.15 5.15-.92 6.72 0-1.57.92-3.82 1.15-6.72 0Z" fill="#111111" />
        <path d="M21.4 12c-2.9 1.15-5.15.92-6.72 0 1.57-.92 3.82-1.15 6.72 0Z" fill="#111111" />
        <path d="M5.35 5.35c2.86 1.24 4.3 2.99 4.76 4.75-1.76-.45-3.51-1.9-4.76-4.75Z" fill="#111111" />
        <path d="M18.65 18.65c-2.86-1.24-4.3-2.99-4.76-4.75 1.76.45 3.51 1.9 4.76 4.75Z" fill="#111111" />
        <path d="M18.65 5.35c-1.24 2.86-2.99 4.3-4.75 4.76.45-1.76 1.9-3.51 4.75-4.76Z" fill="#111111" />
        <path d="M5.35 18.65c1.24-2.86 2.99-4.3 4.75-4.76-.45 1.76-1.9 3.51-4.75 4.76Z" fill="#111111" />
      </svg>
    );
  }

  if (tool === "gemini") {
    return (
      <svg {...commonProps}>
        <path d="M12 2.7 14.58 9.42 21.3 12l-6.72 2.58L12 21.3l-2.58-6.72L2.7 12l6.72-2.58L12 2.7Z" fill="#1A73E8" />
        <path d="M17.7 3.8 18.55 6 20.8 6.85 18.55 7.7 17.7 9.95 16.85 7.7 14.6 6.85 16.85 6 17.7 3.8Z" fill="#7FC7FF" />
      </svg>
    );
  }

  if (tool === "codex") {
    return (
      <svg {...commonProps}>
        <rect x="3.2" y="4.2" width="17.6" height="15.6" rx="4" fill="#0B5FFF" />
        <path d="M8.8 9.1 5.9 12l2.9 2.9" stroke="#FFFFFF" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M15.2 9.1 18.1 12l-2.9 2.9" stroke="#FFFFFF" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
        <path d="m13.2 8.2-2.4 7.6" stroke="#A9D8FF" strokeWidth="1.7" strokeLinecap="round" />
      </svg>
    );
  }

  return (
    <svg {...commonProps}>
      <rect x="3" y="4.5" width="18" height="15" rx="3.5" fill="#0A84FF" />
      <path d="M3 8h18" stroke="#CFEAFF" strokeWidth="1.5" />
      <circle cx="6.5" cy="6.4" r="0.7" fill="#FFFFFF" />
      <circle cx="8.7" cy="6.4" r="0.7" fill="#FFFFFF" />
      <path d="m8 11 2.2 2L8 15" stroke="#FFFFFF" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M13 15h3.5" stroke="#FFFFFF" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}
