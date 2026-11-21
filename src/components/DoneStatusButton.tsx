"use client";

import { useState } from "react";
import DoneConfirmDialog from "./DoneConfirmDialog";

interface DoneStatusButtonProps {
  onMoveToDone: () => void;
  label: string;
  /** 다른 상태 이동을 기다리는 동안 겹쳐 옮기지 않도록 막는다 */
  isDisabled?: boolean;
  hasCleanableResources: boolean;
  doneAlertDismissed: boolean;
}

/** Done 상태 전환 버튼. 연결 리소스가 있으면 보존 안내 모달을 표시한다 */
export default function DoneStatusButton({
  onMoveToDone,
  label,
  isDisabled = false,
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

    onMoveToDone();
  }

  function handleConfirm() {
    setIsDialogOpen(false);
    setIsDismissed(true);
    onMoveToDone();
  }

  return (
    <>
      <button
        type="button"
        onClick={handleClick}
        disabled={isDisabled}
        className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors disabled:cursor-progress disabled:opacity-60"
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
