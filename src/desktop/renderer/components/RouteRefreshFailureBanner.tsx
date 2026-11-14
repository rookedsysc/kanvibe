"use client";

import { useTranslations } from "next-intl";
import InlineLoadError from "@/components/InlineLoadError";

interface RouteRefreshFailureBannerProps {
  reason: string;
  onRetry: () => void;
}

/** 이전 데이터를 보여 주는 중 새로 고침이 실패했음을 화면 위쪽에 알린다. 데이터는 지우지 않는다 */
export default function RouteRefreshFailureBanner({ reason, onRetry }: RouteRefreshFailureBannerProps) {
  const tc = useTranslations("common");

  return (
    <InlineLoadError
      className="fixed left-1/2 top-3 z-[500] w-[min(32rem,calc(100%-2rem))] -translate-x-1/2 shadow-md"
      message={tc("refreshFailed", { reason })}
      onRetry={onRetry}
    />
  );
}
