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

  return (
    <div
      ref={menuRef}
      className="fixed z-[500] min-w-[160px] bg-bg-surface border border-border-default rounded-lg shadow-lg py-1"
      style={{ left: x, top: y }}
    >
      {!hasBranch && (
        <button
          onClick={onBranch}
          className="w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page transition-colors"
        >
          {t("branchOff")}
        </button>
      )}
      {hasBranch && (
        <button
          onClick={onCreateBranchTodo}
          className="w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page transition-colors"
        >
          {t("createBranchTodo")}
        </button>
      )}
      <button
        onClick={onDelete}
        className="w-full text-left px-4 py-2 text-sm text-status-error hover:bg-red-50 transition-colors"
      >
        {t("delete")}
      </button>
    </div>
  );
}
