"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { changeAllTaskSessionTypes, getSessionTypeChangeTargets } from "@/desktop/renderer/actions/kanban";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import type { SessionType } from "@/entities/KanbanTask";
import SessionTypeOptions from "./SessionTypeOptions";

/** 일괄 변경이 지금 어느 단계에 있는지. 서버가 task별 진행을 알려 주지 않으므로 확인할 수 있는 단계만 보인다 */
type BulkChangeStage =
  | { kind: "checking" }
  | { kind: "preparing"; current: number; total: number }
  | { kind: "changing"; taskCount: number };

interface BulkChangeOutcome {
  message: string;
  isFailure: boolean;
  canRetry: boolean;
}

interface BulkSessionTypeChangeSettingsProps {
  defaultSessionType: SessionType;
}

/** 이미 세션이 있는 모든 task(Done 포함)의 세션 타입을 한 번에 바꾸는 설정 행 */
export default function BulkSessionTypeChangeSettings({ defaultSessionType }: BulkSessionTypeChangeSettingsProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const [stage, setStage] = useState<BulkChangeStage | null>(null);
  const [selectedSessionType, setSelectedSessionType] = useState(defaultSessionType);
  const [outcome, setOutcome] = useState<BulkChangeOutcome | null>(null);
  const isPending = stage !== null;

  async function ensureDependencyOnEveryHost(sshHosts: (string | null)[]): Promise<boolean> {
    for (const [hostIndex, sshHost] of sshHosts.entries()) {
      setStage({ kind: "preparing", current: hostIndex + 1, total: sshHosts.length });
      const isDependencyReady = await ensureSessionDependencyWithPrompt(selectedSessionType, sshHost, tc);
      if (!isDependencyReady) {
        return false;
      }
    }

    return true;
  }

  function describeStage(currentStage: BulkChangeStage) {
    switch (currentStage.kind) {
      case "checking":
        return t("bulkSessionTypeChecking");
      case "preparing":
        return t("bulkSessionTypePreparingHost", { current: currentStage.current, total: currentStage.total });
      case "changing":
        return t("bulkSessionTypeChanging", { count: currentStage.taskCount });
    }
  }

  /**
   * 바꿀 대상은 아직 선택한 타입이 아닌 task이므로, 실패한 task를 다시 고르는 일은 같은 적용을 한 번 더 하는 것과 같다.
   */
  async function handleApply() {
    if (isPending) return;

    setOutcome(null);
    setStage({ kind: "checking" });
    try {
      const targets = await getSessionTypeChangeTargets(selectedSessionType);
      if (targets.taskCount === 0) {
        setOutcome({ message: t("bulkSessionTypeNoTargets"), isFailure: false, canRetry: false });
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
      setStage({ kind: "changing", taskCount: targets.taskCount });
      const result = await changeAllTaskSessionTypes(selectedSessionType, shouldCloseExistingSessions);
      setOutcome({
        message: t("bulkSessionTypeChangeResult", {
          changedCount: result.changedCount,
          failedCount: result.failedCount,
        }),
        isFailure: result.failedCount > 0,
        canRetry: result.failedCount > 0,
      });
    } catch (error) {
      setOutcome({
        message: error instanceof Error ? error.message : t("bulkSessionTypeChangeFailed"),
        isFailure: true,
        canRetry: true,
      });
    } finally {
      setStage(null);
    }
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
            onClick={() => void handleApply()}
            disabled={isPending}
            aria-busy={isPending || undefined}
            className="px-3 py-1 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
          >
            {t("bulkSessionTypeApply")}
          </button>
        </div>
      </div>
      {stage ? <p role="status" className="text-xs text-text-muted mt-2">{describeStage(stage)}</p> : null}
      {outcome ? (
        <div
          role={outcome.isFailure ? "alert" : "status"}
          className={`mt-2 flex items-center gap-2 text-xs ${outcome.isFailure ? "text-status-error" : "text-text-secondary"}`}
        >
          <span>{outcome.message}</span>
          {outcome.canRetry ? (
            <button
              type="button"
              onClick={() => void handleApply()}
              className="rounded px-1.5 py-0.5 font-medium underline-offset-2 hover:underline focus-visible:outline focus-visible:outline-1 focus-visible:outline-border-brand"
            >
              {t("bulkSessionTypeRetryFailed")}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
