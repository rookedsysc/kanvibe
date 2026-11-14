"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { runBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import { dismissActionStatus, showActionStatus } from "@/desktop/renderer/components/ActionStatusToast";
import { setBoardViewMode, type BoardViewMode } from "@/desktop/renderer/actions/appSettings";
import { useRouter } from "@/desktop/renderer/navigation";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { TASK_STATUS_ORDER, type TaskStatus } from "@/entities/KanbanTask";

interface CommandItem {
  id: string;
  command: string;
  description: string;
  run: () => void;
}

/** 같은 sync를 겹쳐 누르면 서버는 같은 사이클을 기다리므로, 알림도 한 벌만 띄운다 */
let isManualSyncRunning = false;

/**
 * 팔레트는 바로 닫히므로 sync의 진행과 결과를 토스트로 이어서 알린다.
 * 바뀐 것이 없어도 끝났다는 것을 알려야 사용자가 다시 누르지 않는다.
 */
async function runManualSyncWithStatus(
  t: ReturnType<typeof useTranslations>,
  tc: ReturnType<typeof useTranslations>,
) {
  if (isManualSyncRunning) {
    return;
  }

  isManualSyncRunning = true;
  const progressId = showActionStatus({ kind: "progress", message: t("syncRunning") });
  try {
    const result = await runBackgroundTaskSyncNow();
    dismissActionStatus(progressId);
    const resultMessageKey = result.reviewNeeded
      ? "syncReviewNeeded"
      : result.boardUpdated
        ? "syncBoardUpdated"
        : "syncNoChanges";
    showActionStatus({ kind: "success", message: t(resultMessageKey) });
  } catch (error) {
    console.error("Failed to run background task sync", error);
    dismissActionStatus(progressId);
    showActionStatus({
      kind: "failure",
      message: t("syncFailed", { reason: error instanceof Error ? error.message : String(error) }),
      action: { label: tc("retry"), onAction: () => void runManualSyncWithStatus(t, tc) },
    });
  } finally {
    isManualSyncRunning = false;
  }
}

export default function CommandPaletteDialog() {
  const boardCommands = useBoardCommands();
  const router = useRouter();
  const t = useTranslations("commandPalette");
  const tBoard = useTranslations("board");
  const tc = useTranslations("common");
  const inputRef = useRef<HTMLInputElement>(null);
  const resultsListRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
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
      setQuery("");
      setSelectedIndex(0);
      inputRef.current?.focus();
    }
  }, [isCommandPaletteOpen]);

  function closeDialog() {
    closeCommandPalette();
    requestActiveTerminalFocusAfterUiSettles();
  }

  useEscapeKey(closeDialog, { enabled: isCommandPaletteOpen });

  function runSync() {
    void runManualSyncWithStatus(t, tc);
    closeDialog();
  }

  /** 뷰를 저장한 뒤 보드 경로로 간다. 보드 경로는 저장된 뷰를 읽어 칸반이나 워크스페이스를 보여 준다 */
  function switchBoardView(boardViewMode: BoardViewMode) {
    closeCommandPalette();
    void setBoardViewMode(boardViewMode)
      .catch((error) => {
        console.error("Failed to save board view mode", error);
      })
      .then(() => {
        router.push("/");
      });
  }

  function selectStatus(status: TaskStatus) {
    if (hasActiveTaskContext) {
      boardCommands.moveActiveTaskToStatus(status);
    } else {
      boardCommands.moveFocusedTaskToStatus(status);
    }

    closeDialog();
  }

  const moveTargetStatuses = hasActiveTaskContext && activeTaskCurrentStatus
    ? TASK_STATUS_ORDER.filter((status) => status !== activeTaskCurrentStatus)
    : TASK_STATUS_ORDER;

  const commands = useMemo<CommandItem[]>(() => {
    const items: CommandItem[] = [
      { id: "sync", command: "sync", description: t("syncLabel"), run: runSync },
    ];

    if (hasMoveTarget) {
      for (const status of moveTargetStatuses) {
        items.push({
          id: `move:${status}`,
          command: `move ${status}`,
          description: t("moveToStatusLabel", { status: tBoard(`columns.${status}`) }),
          run: () => selectStatus(status),
        });
      }
    }

    items.push(
      { id: "view:kanban", command: "view kanban", description: t("viewKanbanLabel"), run: () => switchBoardView("kanban") },
      { id: "view:workspace", command: "view workspace", description: t("viewWorkspaceLabel"), run: () => switchBoardView("workspace") },
    );

    return items;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasMoveTarget, moveTargetStatuses, t, tBoard]);

  const results = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) {
      return commands;
    }

    return commands.filter((command) => (
      `${command.command} ${command.description}`.toLowerCase().includes(normalizedQuery)
    ));
  }, [commands, query]);

  const selectedResultIndex = results.length === 0
    ? 0
    : Math.min(selectedIndex, results.length - 1);

  useEffect(() => {
    if (!isCommandPaletteOpen) {
      return;
    }

    const selectedResult = resultsListRef.current?.children[selectedResultIndex];
    if (selectedResult instanceof HTMLElement) {
      selectedResult.scrollIntoView?.({ block: "nearest" });
    }
  }, [isCommandPaletteOpen, results, selectedResultIndex]);

  function handleInputKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setSelectedIndex((current) => (results.length === 0 ? 0 : Math.min(current + 1, results.length - 1)));
        break;
      case "ArrowUp":
        event.preventDefault();
        setSelectedIndex((current) => Math.max(current - 1, 0));
        break;
      case "Enter":
        event.preventDefault();
        results[selectedResultIndex]?.run();
        break;
    }
  }

  if (!isCommandPaletteOpen) {
    return null;
  }

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
        <div className="flex items-center gap-2 border-b border-border-default px-4 py-3">
          <span aria-hidden="true" className="text-sm font-medium text-text-muted">&gt;</span>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelectedIndex(0);
            }}
            onKeyDown={handleInputKeyDown}
            placeholder={t("placeholder")}
            className="w-full bg-transparent text-sm text-text-primary outline-none"
          />
        </div>

        <div className="max-h-[420px] overflow-y-auto">
          {results.length === 0 ? (
            <div className="px-4 py-6 text-sm text-text-muted">{t("empty")}</div>
          ) : (
            <div ref={resultsListRef}>
              {results.map((command, index) => (
                <button
                  key={command.id}
                  type="button"
                  onClick={command.run}
                  onMouseEnter={() => setSelectedIndex(index)}
                  className={`flex w-full items-center gap-3 px-4 py-2.5 text-left text-sm transition-colors ${
                    index === selectedResultIndex ? "bg-brand-primary/10" : "hover:bg-bg-page"
                  }`}
                >
                  <span className="shrink-0 font-mono text-xs font-semibold text-text-primary">{command.command}</span>
                  <span className="truncate text-text-muted">{command.description}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="border-t border-border-default px-4 py-2 text-xs text-text-muted">
          {t("hint")}
        </div>
      </div>
    </div>
  );
}
