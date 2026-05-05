"use client";

import { useEffect, useRef } from "react";
import { useTranslations } from "next-intl";

interface TaskContextMenuProps {
  x: number;
  y: number;
  onClose: () => void;
  onBranch: () => void;
  onCreateBranchTodo: () => void;
  onDelete: () => void;
  hasBranch: boolean;
}

/** 칸반 카드 우클릭 시 표시되는 컨텍스트 메뉴 */
export default function TaskContextMenu({
  x,
  y,
  onClose,
  onBranch,
  onCreateBranchTodo,
  onDelete,
  hasBranch,
}: TaskContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const menuItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const t = useTranslations("contextMenu");

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    }

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [onClose]);

  useEffect(() => {
    menuItemRefs.current[0]?.focus();
  }, [hasBranch]);

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const menuItems = menuItemRefs.current.filter(
      (item): item is HTMLButtonElement => item !== null,
    );
    const currentIndex = menuItems.indexOf(document.activeElement as HTMLButtonElement);

    switch (event.key) {
      case "ArrowDown": {
        event.preventDefault();
        const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % menuItems.length : 0;
        menuItems[nextIndex]?.focus();
        break;
      }
      case "ArrowUp": {
        event.preventDefault();
        const nextIndex = currentIndex > 0 ? currentIndex - 1 : menuItems.length - 1;
        menuItems[nextIndex]?.focus();
        break;
      }
      case "Enter":
        if (currentIndex >= 0) {
          event.preventDefault();
          menuItems[currentIndex].click();
        }
        break;
      case "Escape":
        event.preventDefault();
        onClose();
        break;
    }
  }

  return (
    <div
      ref={menuRef}
      role="menu"
      tabIndex={-1}
      onKeyDown={handleKeyDown}
      className="fixed z-[500] min-w-[160px] bg-bg-surface border border-border-default rounded-lg shadow-lg py-1"
      style={{ left: x, top: y }}
    >
      {!hasBranch && (
        <button
          ref={(node) => {
            menuItemRefs.current[0] = node;
          }}
          type="button"
          role="menuitem"
          onClick={onBranch}
          className="w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page focus:bg-bg-page focus:outline-none transition-colors"
        >
          {t("branchOff")}
        </button>
      )}
      {hasBranch && (
        <button
          ref={(node) => {
            menuItemRefs.current[0] = node;
          }}
          type="button"
          role="menuitem"
          onClick={onCreateBranchTodo}
          className="w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page focus:bg-bg-page focus:outline-none transition-colors"
        >
          {t("createBranchTodo")}
        </button>
      )}
      <button
        ref={(node) => {
          menuItemRefs.current[1] = node;
        }}
        type="button"
        role="menuitem"
        onClick={onDelete}
        className="w-full text-left px-4 py-2 text-sm text-status-error hover:bg-red-50 focus:bg-red-50 focus:outline-none transition-colors"
      >
        {t("delete")}
      </button>
    </div>
  );
}
