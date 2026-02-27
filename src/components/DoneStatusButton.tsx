"use client";

import { useState } from "react";
import DoneConfirmDialog from "./DoneConfirmDialog";

interface DoneStatusButtonProps {
  onStatusChange: () => void | Promise<void>;
  label: string;
  hasCleanableResources: boolean;
  doneAlertDismissed: boolean;
}

/** Done 상태 전환 버튼. 정리 대상 리소스가 있으면 확인 모달을 표시한다 */
export default function DoneStatusButton({
  onStatusChange,
  label,
  hasCleanableResources,
  doneAlertDismissed,
}: DoneStatusButtonProps) {
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isDismissed, setIsDismissed] = useState(doneAlertDismissed);

  const shouldShowAlert = hasCleanableResources && !isDismissed;

  function handleClick() {
    if (shouldShowAlert) {
      setIsDialogOpen(true);
      return;
    }
    onStatusChange();
  }

  function handleConfirm() {
    setIsDialogOpen(false);
    setIsDismissed(true);
    onStatusChange();
  }

  return (
    <>
      <button
        type="button"
        onClick={handleClick}
        className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors"
      >
        {label}
      </button>

      <DoneConfirmDialog
        isOpen={isDialogOpen}
        onConfirm={handleConfirm}
        onCancel={() => setIsDialogOpen(false)}
      />
    </>
  );
}
