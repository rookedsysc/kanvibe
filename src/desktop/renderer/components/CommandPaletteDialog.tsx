"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { runBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { TASK_STATUS_ORDER, type TaskStatus } from "@/entities/KanbanTask";

type CommandPaletteView = "commands" | "statuses";

export default function CommandPaletteDialog() {
  const boardCommands = useBoardCommands();
  const t = useTranslations("commandPalette");
  const tBoard = useTranslations("board");
  const tc = useTranslations("common");
  const [view, setView] = useState<CommandPaletteView>("commands");
  const {
    isCommandPaletteOpen,
    closeCommandPalette,
    hasActiveTaskContext,
    activeTaskCurrentStatus,
    commandPaletteBoardFocusedTaskId,
  } = boardCommands;
  const hasMoveTarget = hasActiveTaskContext || Boolean(commandPaletteBoardFocusedTaskId);

  useEffect(() => {
    if (isCommandPaletteOpen) {
      setView("commands");
    }
  }, [isCommandPaletteOpen]);

  function closeDialog() {
    closeCommandPalette();
    requestActiveTerminalFocusAfterUiSettles();
  }

  useEscapeKey(
    () => (view === "statuses" ? setView("commands") : closeDialog()),
    { enabled: isCommandPaletteOpen },
  );

  function runSync() {
    void runBackgroundTaskSyncNow().catch((error) => {
      console.error("Failed to run background task sync", error);
    });
    closeDialog();
  }

  function selectStatus(status: TaskStatus) {
    if (hasActiveTaskContext) {
      boardCommands.moveActiveTaskToStatus(status);
    } else {
      boardCommands.moveFocusedTaskToStatus(status);
    }

    closeDialog();
  }

  if (!isCommandPaletteOpen) {
    return null;
  }

  const statuses = hasActiveTaskContext && activeTaskCurrentStatus
    ? TASK_STATUS_ORDER.filter((status) => status !== activeTaskCurrentStatus)
    : TASK_STATUS_ORDER;

  return (
    <div data-terminal-focus-blocker="true" className="fixed inset-0 z-[500] flex items-start justify-center bg-black/45 px-4 pt-24">
      <button
        type="button"
        aria-label={tc("close")}
        className="absolute inset-0 cursor-default"
        onClick={closeDialog}
      />
      <div
        role="dialog"
        aria-modal="true"
        className="relative z-10 w-full max-w-md overflow-hidden rounded-2xl border border-border-default bg-bg-surface shadow-2xl"
      >
        <div className="border-b border-border-default px-4 py-3">
          <p className="text-sm font-semibold text-text-primary">
            {view === "statuses" ? t("statusListTitle") : t("title")}
          </p>
        </div>

        {view === "commands" ? (
          <div>
            <button
              type="button"
              onClick={runSync}
              className="flex w-full flex-col items-start gap-0.5 border-b border-border-subtle px-4 py-3 text-left transition-colors hover:bg-bg-page"
            >
              <span className="text-sm font-medium text-text-primary">{t("syncLabel")}</span>
              <span className="text-xs text-text-muted">{t("syncDescription")}</span>
            </button>
            <button
              type="button"
              disabled={!hasMoveTarget}
              onClick={() => setView("statuses")}
              className="flex w-full flex-col items-start gap-0.5 border-b border-border-subtle px-4 py-3 text-left transition-colors enabled:hover:bg-bg-page disabled:cursor-not-allowed disabled:opacity-50"
            >
              <span className="text-sm font-medium text-text-primary">{t("moveLabel")}</span>
              <span className="text-xs text-text-muted">
                {hasMoveTarget ? t("moveDescription") : t("moveDisabledHint")}
              </span>
            </button>
          </div>
        ) : (
          <div>
            {statuses.map((status) => (
              <button
                key={status}
                type="button"
                onClick={() => selectStatus(status)}
                className="flex w-full items-center px-4 py-3 text-left text-sm font-medium text-text-primary transition-colors hover:bg-bg-page"
              >
                {tBoard(`columns.${status}`)}
              </button>
            ))}
          </div>
        )}

        <div className="border-t border-border-default px-4 py-2 text-xs text-text-muted">
          {t("hint")}
        </div>
      </div>
    </div>
  );
}
