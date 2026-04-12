"use client";

import { useTranslations } from "next-intl";
import { useEffect, useState } from "react";
import HooksStatusDialog from "@/components/HooksStatusDialog";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";

interface HooksStatusCardProps {
  taskId: string;
  initialClaudeStatus: ClaudeHooksStatus | null;
  initialGeminiStatus: GeminiHooksStatus | null;
  initialCodexStatus: CodexHooksStatus | null;
  initialOpenCodeStatus: OpenCodeHooksStatus | null;
  isRemote: boolean;
}

export default function HooksStatusCard({
  taskId,
  initialClaudeStatus,
  initialGeminiStatus,
  initialCodexStatus,
  initialOpenCodeStatus,
  isRemote,
}: HooksStatusCardProps) {
  const t = useTranslations("taskDetail");
  const [claudeStatus, setClaudeStatus] = useState(initialClaudeStatus);
  const [geminiStatus, setGeminiStatus] = useState(initialGeminiStatus);
  const [codexStatus, setCodexStatus] = useState(initialCodexStatus);
  const [openCodeStatus, setOpenCodeStatus] = useState(initialOpenCodeStatus);
  const [isDialogOpen, setIsDialogOpen] = useState(false);

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

  // 신호등 상태 계산
  const getOverallStatus = () => {
    const installed = [claudeStatus?.installed, geminiStatus?.installed, codexStatus?.installed, openCodeStatus?.installed];
    const installedCount = installed.filter((x) => x === true).length;

    if (installedCount === 4) return { icon: "🟢", label: "All OK" };
    if (installedCount === 0) return { icon: "🔴", label: "Not Installed" };
    return { icon: "🟡", label: "Partial" };
  };

  const overallStatus = getOverallStatus();

  return (
    <>
      <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
        <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-3">
          {t("hooksStatus")}
        </h3>

        {/* 신호등 버튼 */}
        {!isRemote && (
          <button
            onClick={() => setIsDialogOpen(true)}
            className="w-full px-3 py-2 text-sm bg-bg-page border border-border-default hover:border-brand-primary rounded-md transition-colors text-left flex items-center gap-2"
          >
            <span className="text-base">{overallStatus.icon}</span>
            <span className="text-text-primary font-medium">{overallStatus.label}</span>
            <span className="ml-auto text-text-muted">→</span>
          </button>
        )}
      </div>

      {/* Hooks Status Dialog */}
      {!isRemote && (
        <HooksStatusDialog
          isOpen={isDialogOpen}
          onClose={() => setIsDialogOpen(false)}
          taskId={taskId}
          claudeStatus={claudeStatus}
          geminiStatus={geminiStatus}
          codexStatus={codexStatus}
          openCodeStatus={openCodeStatus}
          isRemote={isRemote}
        />
      )}
    </>
  );
}
