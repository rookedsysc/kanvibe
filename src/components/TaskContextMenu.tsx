"use client";

import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import type { TaskStatus } from "@/entities/KanbanTask";

interface TaskContextMenuStatusOption {
  status: TaskStatus;
  label: string;
  colorClass: string;
}

interface TaskContextMenuProps {
  x: number;
  y: number;
  onClose: () => void;
  onBranch: () => void;
  onCreateBranchTodo: () => void;
  onStatusChange: (status: TaskStatus) => void;
  onDelete: () => void;
  hasBranch: boolean;
  currentStatus: TaskStatus;
  statusOptions: TaskContextMenuStatusOption[];
}

/** 칸반 카드 우클릭 시 표시되는 컨텍스트 메뉴 */
export default function TaskContextMenu({
  x,
  y,
  onClose,
  onBranch,
  onCreateBranchTodo,
  onStatusChange,
  onDelete,
  hasBranch,
  currentStatus,
  statusOptions,
}: TaskContextMenuProps) {
  const [isStatusDropdownOpen, setIsStatusDropdownOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const statusOptionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const t = useTranslations("contextMenu");
  const currentStatusOption = statusOptions.find((option) => option.status === currentStatus);
  const currentStatusLabel = currentStatusOption?.label ?? currentStatus;

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

  useEffect(() => {
    if (!isStatusDropdownOpen) return;

    statusOptionRefs.current[0]?.focus();
  }, [isStatusDropdownOpen]);

  function closeStatusDropdown({ restoreFocus = true } = {}) {
    setIsStatusDropdownOpen(false);

    if (restoreFocus) {
      requestAnimationFrame(() => {
        menuItemRefs.current[1]?.focus();
      });
    }
  }

  function handleStatusSelect(status: TaskStatus) {
    setIsStatusDropdownOpen(false);

    if (status === currentStatus) {
      requestAnimationFrame(() => {
        menuItemRefs.current[1]?.focus();
      });
      return;
    }

    onStatusChange(status);
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (isStatusDropdownOpen) {
      const statusItems = statusOptionRefs.current.filter(
        (item): item is HTMLButtonElement => item !== null,
      );
      const currentIndex = statusItems.indexOf(document.activeElement as HTMLButtonElement);

      switch (event.key) {
        case "ArrowDown": {
          event.preventDefault();
          const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % statusItems.length : 0;
          statusItems[nextIndex]?.focus();
          break;
        }
        case "ArrowUp": {
          event.preventDefault();
          const nextIndex = currentIndex > 0 ? currentIndex - 1 : statusItems.length - 1;
          statusItems[nextIndex]?.focus();
          break;
        }
        case "Home":
          event.preventDefault();
          statusItems[0]?.focus();
          break;
        case "End":
          event.preventDefault();
          statusItems[statusItems.length - 1]?.focus();
          break;
        case "Enter":
        case " ":
          if (currentIndex >= 0) {
            event.preventDefault();
            statusItems[currentIndex].click();
          }
          break;
        case "ArrowLeft":
        case "Escape":
          event.preventDefault();
          closeStatusDropdown();
          break;
      }

      return;
    }

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
      case "ArrowRight":
        if (document.activeElement === menuItemRefs.current[1]) {
          event.preventDefault();
          setIsStatusDropdownOpen(true);
        }
        break;
      case "Enter":
      case " ":
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
      className="fixed z-[500] min-w-[220px] rounded-lg border border-border-default bg-bg-surface py-1 shadow-lg"
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
        aria-haspopup="menu"
        aria-expanded={isStatusDropdownOpen}
        aria-label={`${t("changeStatus")} ${currentStatusLabel}`}
        onClick={() => setIsStatusDropdownOpen((current) => !current)}
        className="flex w-full items-center gap-2 px-4 py-2 text-left text-sm text-text-primary transition-colors hover:bg-bg-page focus:bg-bg-page focus:outline-none"
      >
        <span className="min-w-0 flex-1">{t("changeStatus")}</span>
        <span className="inline-flex max-w-[96px] items-center gap-1.5 truncate rounded-md border border-border-subtle bg-bg-page px-1.5 py-0.5 text-xs text-text-secondary">
          {currentStatusOption ? (
            <span
              aria-hidden="true"
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${currentStatusOption.colorClass}`}
            />
          ) : null}
          <span className="truncate">{currentStatusLabel}</span>
        </span>
        <span aria-hidden="true" className="text-xs text-text-muted">
          {isStatusDropdownOpen ? "⌃" : "⌄"}
        </span>
      </button>
      {isStatusDropdownOpen && (
        <div
          role="menu"
          aria-label={t("changeStatus")}
          className="mx-1 mb-1 rounded-md border border-border-subtle bg-bg-page p-1 shadow-inner"
        >
          {statusOptions.map((option, index) => (
            <button
              key={option.status}
              ref={(node) => {
                statusOptionRefs.current[index] = node;
              }}
              type="button"
              role="menuitemradio"
              aria-checked={option.status === currentStatus}
              onClick={() => handleStatusSelect(option.status)}
              className={`flex w-full items-center gap-2 rounded px-2.5 py-1.5 text-left text-sm transition-colors focus:outline-none ${
                option.status === currentStatus
                  ? "bg-brand-subtle text-text-brand"
                  : "text-text-primary hover:bg-bg-surface focus:bg-bg-surface"
              }`}
            >
              <span
                aria-hidden="true"
                className={`h-2 w-2 shrink-0 rounded-full ${option.colorClass}`}
              />
              <span className="min-w-0 flex-1 truncate">{option.label}</span>
              {option.status === currentStatus ? (
                <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-brand-primary" />
              ) : null}
            </button>
          ))}
        </div>
      )}
      <button
        ref={(node) => {
          menuItemRefs.current[2] = node;
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
