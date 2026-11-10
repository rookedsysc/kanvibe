"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import { changeAllTaskSessionTypes, getSessionTypeChangeTargets } from "@/desktop/renderer/actions/kanban";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import type { SessionType } from "@/entities/KanbanTask";
import SessionTypeOptions from "./SessionTypeOptions";

interface BulkSessionTypeChangeSettingsProps {
  defaultSessionType: SessionType;
}

/** 이미 세션이 있는 모든 task(Done 포함)의 세션 타입을 한 번에 바꾸는 설정 행 */
export default function BulkSessionTypeChangeSettings({ defaultSessionType }: BulkSessionTypeChangeSettingsProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [selectedSessionType, setSelectedSessionType] = useState(defaultSessionType);
  const [resultMessage, setResultMessage] = useState<string | null>(null);

  async function ensureDependencyOnEveryHost(sshHosts: (string | null)[]): Promise<boolean> {
    for (const sshHost of sshHosts) {
      const isDependencyReady = await ensureSessionDependencyWithPrompt(selectedSessionType, sshHost, tc);
      if (!isDependencyReady) {
        return false;
      }
    }

    return true;
  }

  function handleApply() {
    setResultMessage(null);
    startTransition(async () => {
      try {
        const targets = await getSessionTypeChangeTargets(selectedSessionType);
        if (targets.taskCount === 0) {
          setResultMessage(t("bulkSessionTypeNoTargets"));
          return;
        }

        const isChangeConfirmed = window.confirm(t("bulkSessionTypeChangeConfirm", {
          count: targets.taskCount,
          sessionType: selectedSessionType,
        }));
        if (!isChangeConfirmed || !await ensureDependencyOnEveryHost(targets.sshHosts)) {
          return;
        }

        const shouldCloseExistingSessions = targets.liveSessionCount > 0
          && window.confirm(t("bulkCloseExistingSessionsConfirm", { count: targets.liveSessionCount }));
        const result = await changeAllTaskSessionTypes(selectedSessionType, shouldCloseExistingSessions);
        setResultMessage(t("bulkSessionTypeChangeResult", {
          changedCount: result.changedCount,
          failedCount: result.failedCount,
        }));
      } catch (error) {
        setResultMessage(error instanceof Error ? error.message : t("bulkSessionTypeChangeFailed"));
      }
    });
  }

  return (
    <div className="mt-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <span className="text-sm text-text-primary">{t("bulkSessionTypeChange")}</span>
          <p className="text-xs text-text-muted mt-0.5">{t("bulkSessionTypeChangeDescription")}</p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={selectedSessionType}
            onChange={(event) => setSelectedSessionType(event.target.value as SessionType)}
            disabled={isPending}
            className="px-2 py-1 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
          >
            <SessionTypeOptions />
          </select>
          <button
            type="button"
            onClick={handleApply}
            disabled={isPending}
            className="px-3 py-1 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
          >
            {t("bulkSessionTypeApply")}
          </button>
        </div>
      </div>
      {resultMessage && <p className="text-xs text-text-secondary mt-2">{resultMessage}</p>}
    </div>
  );
}
