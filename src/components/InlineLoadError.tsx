"use client";

import { useTranslations } from "next-intl";

interface InlineLoadErrorProps {
  /** 사용자에게 보일 실패 사유. 번역된 문장을 그대로 받는다 */
  message: string;
  onRetry: () => void;
  className?: string;
}

/** 조회 실패를 빈 목록이나 기본값으로 바꾸지 않고, 실패한 자리에 사유와 재시도를 남긴다 */
export default function InlineLoadError({ message, onRetry, className = "" }: InlineLoadErrorProps) {
  const tc = useTranslations("common");

  return (
    <div
      role="alert"
      className={`flex items-start justify-between gap-3 rounded-md border border-status-error/20 bg-bg-page px-3 py-2 text-xs text-status-error ${className}`}
    >
      <span className="min-w-0 break-words">{message}</span>
      <button
        type="button"
        onClick={onRetry}
        className="shrink-0 rounded px-1.5 py-0.5 font-medium underline-offset-2 hover:underline focus-visible:outline focus-visible:outline-1 focus-visible:outline-border-brand"
      >
        {tc("retry")}
      </button>
    </div>
  );
}
