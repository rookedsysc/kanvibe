"use client";

import { useTranslations } from "next-intl";

interface DeleteTaskButtonProps {
  onDelete: () => void | Promise<void>;
}

export default function DeleteTaskButton({ onDelete }: DeleteTaskButtonProps) {
  const t = useTranslations("taskDetail");

  function handleClick() {
    if (!confirm(t("deleteConfirm"))) return;
    onDelete();
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      className="px-3 py-1.5 text-xs bg-red-50 hover:bg-red-100 text-status-error rounded-md transition-colors"
    >
      {t("delete")}
    </button>
  );
}
