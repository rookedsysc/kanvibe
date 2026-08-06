import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

interface TerminalTabBarProps {
  tabs: TerminalTab[];
  onSelect: (tabId: string) => void;
  onCreate: () => void;
  onClose: (tabId: string) => void;
  onRename: (tabId: string, name: string) => void;
  onMove: (tabId: string, targetIndex: number) => void;
}

/**
 * tmux window, zellij tab, terminal 세션 탭을 같은 모양으로 보여 주는 탭 바.
 * 어떤 세션 타입인지는 여기서 알 필요가 없고, 조작은 전부 상위가 넘긴 콜백으로 나간다.
 */
export default function TerminalTabBar({
  tabs,
  onSelect,
  onCreate,
  onClose,
  onRename,
  onMove,
}: TerminalTabBarProps) {
  const t = useTranslations("taskDetail");
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [draggingTabId, setDraggingTabId] = useState<string | null>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    renameInputRef.current?.select();
  }, [renamingTabId]);

  const commitRename = (tabId: string, nextName: string) => {
    const trimmedName = nextName.trim();
    const previousName = tabs.find((tab) => tab.id === tabId)?.name;

    setRenamingTabId(null);
    if (trimmedName && trimmedName !== previousName) {
      onRename(tabId, trimmedName);
    }
  };

  return (
    <div
      data-testid="terminal-tab-bar"
      className="flex items-center gap-1 overflow-x-auto"
      role="tablist"
      aria-label={t("terminalTabs")}
    >
      {tabs.map((tab, tabIndex) => (
        <div
          key={tab.id}
          data-terminal-tab-id={tab.id}
          role="tab"
          aria-selected={tab.isActive}
          tabIndex={-1}
          draggable={renamingTabId !== tab.id}
          onDragStart={() => setDraggingTabId(tab.id)}
          onDragEnd={() => setDraggingTabId(null)}
          onDragOver={(event) => event.preventDefault()}
          onDrop={(event) => {
            event.preventDefault();
            if (draggingTabId && draggingTabId !== tab.id) {
              onMove(draggingTabId, tabIndex);
            }
            setDraggingTabId(null);
          }}
          onClick={() => {
            if (!tab.isActive) {
              onSelect(tab.id);
            }
          }}
          onDoubleClick={() => setRenamingTabId(tab.id)}
          className={`group flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-mono transition-colors ${
            tab.isActive
              ? "bg-brand-primary text-text-inverse"
              : "bg-button-neutral text-terminal-text hover:bg-button-neutral-hover"
          }`}
        >
          {renamingTabId === tab.id ? (
            <input
              ref={renameInputRef}
              defaultValue={tab.name}
              data-testid="terminal-tab-rename-input"
              aria-label={t("renameTerminalTab")}
              onClick={(event) => event.stopPropagation()}
              onBlur={(event) => commitRename(tab.id, event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  commitRename(tab.id, event.currentTarget.value);
                  return;
                }

                if (event.key === "Escape") {
                  setRenamingTabId(null);
                }
              }}
              className="w-24 bg-transparent outline-none"
            />
          ) : (
            <span className="max-w-40 truncate">{tab.name}</span>
          )}
          <button
            type="button"
            data-testid={`terminal-tab-close-${tab.id}`}
            aria-label={t("closeTerminalTab", { name: tab.name })}
            onClick={(event) => {
              event.stopPropagation();
              onClose(tab.id);
            }}
            className="opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
          >
            ×
          </button>
        </div>
      ))}
      <button
        type="button"
        data-testid="terminal-tab-new"
        aria-label={t("newTerminalTab")}
        onClick={onCreate}
        className="shrink-0 rounded-md px-2 py-1 text-xs text-terminal-text transition-colors hover:bg-button-neutral-hover"
      >
        +
      </button>
    </div>
  );
}
