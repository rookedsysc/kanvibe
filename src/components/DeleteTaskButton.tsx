"use client";

import { useState } from "react";
import { useFormStatus } from "react-dom";
import { useTranslations } from "next-intl";

interface DeleteTaskButtonProps {
  deleteAction: () => Promise<void>;
}

/** 다른 데스크톱 전파와 worktree 정리가 끝날 때까지 버튼을 막아 같은 요청을 두 번 보내지 않게 한다 */
function DeleteSubmitButton() {
  const t = useTranslations("taskDetail");
  const { pending } = useFormStatus();

  return (
    <button
      type="submit"
      disabled={pending}
      aria-busy={pending || undefined}
      className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs bg-red-50 hover:bg-red-100 text-status-error rounded-md transition-colors disabled:cursor-progress disabled:opacity-60"
    >
      {pending ? (
        <span aria-hidden="true" className="h-3 w-3 rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin" />
      ) : null}
      {pending ? t("deleting") : t("delete")}
    </button>
  );
}

export default function DeleteTaskButton({
  deleteAction,
}: DeleteTaskButtonProps) {
  const t = useTranslations("taskDetail");
  const [failureReason, setFailureReason] = useState<string | null>(null);

  async function submitDeletion() {
    setFailureReason(null);
    try {
      await deleteAction();
    } catch (error) {
      setFailureReason(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <form
      action={submitDeletion}
      onSubmit={(e) => {
        if (!confirm(t("deleteConfirm"))) {
          e.preventDefault();
        }
      }}
    >
      <DeleteSubmitButton />
      {failureReason ? (
        <p role="alert" className="mt-2 text-xs text-status-error">
          {t("deleteFailed", { reason: failureReason })}
        </p>
      ) : null}
    </form>
  );
}
