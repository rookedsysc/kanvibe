"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import { dismissDoneAlert } from "@/app/actions/appSettings";

interface DoneConfirmDialogProps {
  isOpen: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Done 이동 시 리소스 삭제 경고 모달. "다시 묻지 않기" 체크 시 DB에 설정을 저장한다 */
export default function DoneConfirmDialog({
  isOpen,
  onConfirm,
  onCancel,
}: DoneConfirmDialogProps) {
  const t = useTranslations("common.doneAlert");
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const [isPending, startTransition] = useTransition();

  if (!isOpen) return null;

  function handleConfirm() {
    if (dontAskAgain) {
      startTransition(async () => {
        await dismissDoneAlert();
        onConfirm();
      });
    } else {
      onConfirm();
    }
  }

  return (
    <div className="fixed inset-0 z-[500] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-sm bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <h2 className="text-lg font-semibold text-text-primary mb-2">
          {t("title")}
        </h2>
        <p className="text-sm text-text-secondary mb-4">
          {t("message")}
        </p>

        <label className="flex items-center gap-2 mb-5 cursor-pointer">
          <input
            type="checkbox"
            checked={dontAskAgain}
            onChange={(e) => setDontAskAgain(e.target.checked)}
            className="w-4 h-4 rounded border-border-default text-brand-primary focus:ring-brand-primary"
          />
          <span className="text-xs text-text-muted">{t("dontAskAgain")}</span>
        </label>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={isPending}
            className="px-4 py-1.5 text-sm bg-bg-page border border-border-default hover:border-brand-primary text-text-secondary rounded-md transition-colors"
          >
            {t("cancel")}
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={isPending}
            className="px-4 py-1.5 text-sm bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md transition-colors disabled:opacity-50"
          >
            {t("confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
