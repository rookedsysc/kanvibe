"use client";

import { useState, useRef } from "react";
import DoneConfirmDialog from "./DoneConfirmDialog";

interface DoneStatusButtonProps {
  statusChangeAction: (formData: FormData) => Promise<void>;
  label: string;
  hasCleanableResources: boolean;
  doneAlertDismissed: boolean;
}

/** Done 상태 전환 버튼. 정리 대상 리소스가 있으면 확인 모달을 표시한다 */
export default function DoneStatusButton({
  statusChangeAction,
  label,
  hasCleanableResources,
  doneAlertDismissed,
}: DoneStatusButtonProps) {
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isDismissed, setIsDismissed] = useState(doneAlertDismissed);
  const formRef = useRef<HTMLFormElement>(null);

  const shouldShowAlert = hasCleanableResources && !isDismissed;

  function handleClick(e: React.MouseEvent) {
    if (shouldShowAlert) {
      e.preventDefault();
      setIsDialogOpen(true);
    }
  }

  function handleConfirm() {
    setIsDialogOpen(false);
    setIsDismissed(true);
    formRef.current?.requestSubmit();
  }

  return (
    <>
      <form ref={formRef} action={statusChangeAction}>
        <input type="hidden" name="status" value="done" />
        <button
          type="submit"
          onClick={handleClick}
          className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors"
        >
          {label}
        </button>
      </form>

      <DoneConfirmDialog
        isOpen={isDialogOpen}
        onConfirm={handleConfirm}
        onCancel={() => setIsDialogOpen(false)}
      />
    </>
  );
}
