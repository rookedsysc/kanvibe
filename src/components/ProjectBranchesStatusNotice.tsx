"use client";

import { useTranslations } from "next-intl";
import InlineLoadError from "@/components/InlineLoadError";
import type { ProjectBranchesStatus } from "@/desktop/renderer/hooks/useProjectBranches";

interface ProjectBranchesStatusNoticeProps {
  status: ProjectBranchesStatus;
  failureReason: string | null;
  onRetry: () => void;
}

/** 브랜치 입력 아래에 목록 조회 상태를 알린다. 지금 보이는 기본 브랜치가 서버 목록이 아니라는 점을 드러낸다 */
export default function ProjectBranchesStatusNotice({ status, failureReason, onRetry }: ProjectBranchesStatusNoticeProps) {
  const t = useTranslations("task");
  const tc = useTranslations("common");

  if (status === "loading") {
    return <p role="status" className="mt-1 text-xs text-text-muted">{t("branchesLoading")}</p>;
  }

  if (status === "failed") {
    return (
      <InlineLoadError
        className="mt-1"
        message={tc("loadFailed", { reason: failureReason ?? "" })}
        onRetry={onRetry}
      />
    );
  }

  return null;
}
