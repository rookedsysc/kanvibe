"use client";

import { useTranslations } from "next-intl";

interface DeleteTaskButtonProps {
  deleteAction: () => Promise<void>;
}

export default function DeleteTaskButton({
  deleteAction,
}: DeleteTaskButtonProps) {
  const t = useTranslations("taskDetail");

  return (
    <form
      action={deleteAction}
      onSubmit={(e) => {
        if (!confirm(t("deleteConfirm"))) {
          e.preventDefault();
        }
      }}
    >
      <button
        type="submit"
        className="px-3 py-1.5 text-xs bg-red-50 hover:bg-red-100 text-status-error rounded-md transition-colors"
      >
        {t("delete")}
      </button>
    </form>
  );
}
