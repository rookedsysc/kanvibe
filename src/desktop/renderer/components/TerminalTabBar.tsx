import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import {
  TERMINAL_TAB_SHORTCUT_INDEXES,
  createTerminalTabShortcut,
  formatShortcutForDisplay,
  getCurrentShortcutPlatform,
} from "@/desktop/renderer/utils/keyboardShortcut";
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
  const shortcutPlatform = getCurrentShortcutPlatform();
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
      className="flex min-w-0 flex-1 items-center overflow-x-auto"
      role="tablist"
      aria-label={t("terminalTabs")}
    >
      {tabs.map((tab, tabIndex) => {
        const shortcutIndex = TERMINAL_TAB_SHORTCUT_INDEXES[tabIndex];
        const shortcutText = shortcutIndex
          ? formatShortcutForDisplay(createTerminalTabShortcut(shortcutIndex), shortcutPlatform)
          : null;
        /** 활성 탭의 배경 pill이 이미 경계를 그리므로, 그 양옆에는 구분선을 겹쳐 두지 않는다 */
        const hasDivider = tabIndex > 0 && !tab.isActive && !tabs[tabIndex - 1].isActive;

        return (
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
          /** 탭은 남는 폭을 균등하게 나눠 갖고, 제목이 읽히지 않을 만큼 좁아지면 바가 가로로 스크롤된다 */
          className={`group relative flex min-w-44 flex-1 items-center gap-2 rounded-md px-3 py-1 text-xs font-mono transition-colors ${
            tab.isActive
              ? "bg-brand-primary text-text-inverse"
              : "text-terminal-text hover:bg-white/5"
          } ${hasDivider ? "border-l border-white/10" : "border-l border-transparent"}`}
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
              className="min-w-0 flex-1 bg-transparent text-center outline-none"
            />
          ) : (
            <span data-terminal-tab-name className="min-w-0 flex-1 truncate text-center">
              {tab.name}
            </span>
          )}
          {shortcutText ? (
            <span className="shrink-0 text-[10px] opacity-60 transition-opacity group-hover:opacity-0">
              {shortcutText}
            </span>
          ) : null}
          <button
            type="button"
            data-testid={`terminal-tab-close-${tab.id}`}
            aria-label={t("closeTerminalTab", { name: tab.name })}
            onClick={(event) => {
              event.stopPropagation();
              onClose(tab.id);
            }}
            className="absolute right-2 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
          >
            ×
          </button>
        </div>
        );
      })}
      <button
        type="button"
        data-testid="terminal-tab-new"
        aria-label={t("newTerminalTab")}
        onClick={onCreate}
        className="shrink-0 rounded-md px-2 py-1 text-xs text-terminal-text transition-colors hover:bg-white/10"
      >
        +
      </button>
    </div>
  );
}
