"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { runBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { TASK_STATUS_ORDER, type TaskStatus } from "@/entities/KanbanTask";

interface CommandItem {
  id: string;
  label: string;
  run: () => void;
}

export default function CommandPaletteDialog() {
  const boardCommands = useBoardCommands();
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

  const moveTargetStatuses = hasActiveTaskContext && activeTaskCurrentStatus
    ? TASK_STATUS_ORDER.filter((status) => status !== activeTaskCurrentStatus)
    : TASK_STATUS_ORDER;

  const commands = useMemo<CommandItem[]>(() => {
    const items: CommandItem[] = [
      { id: "sync", label: t("syncLabel"), run: runSync },
    ];

    if (hasMoveTarget) {
      for (const status of moveTargetStatuses) {
        items.push({
          id: `move:${status}`,
          label: t("moveToStatusLabel", { status: tBoard(`columns.${status}`) }),
          run: () => selectStatus(status),
        });
      }
    }

    return items;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasMoveTarget, moveTargetStatuses, t, tBoard]);

  const results = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) {
      return commands;
    }

    return commands.filter((command) => command.label.toLowerCase().includes(normalizedQuery));
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
                  className={`flex w-full items-center px-4 py-2.5 text-left text-sm font-medium text-text-primary transition-colors ${
                    index === selectedResultIndex ? "bg-brand-primary/10" : "hover:bg-bg-page"
                  }`}
                >
                  {command.label}
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
