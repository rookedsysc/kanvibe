import { useCallback, useEffect, useState } from "react";
import { getProjectBranches } from "@/desktop/renderer/actions/project";

export type ProjectBranchesStatus = "idle" | "loading" | "loaded" | "failed";

/**
 * 프로젝트의 브랜치 목록과 그 조회 상태를 함께 들고 있는다.
 * 조회가 늦거나 실패해도 빈 목록으로만 보이면 다른 브랜치가 없다고 오해하므로, 화면이 상태를 따로 보여 줄 수 있게 한다.
 */
export function useProjectBranches(projectId: string) {
  const [branches, setBranches] = useState<string[]>([]);
  const [status, setStatus] = useState<ProjectBranchesStatus>(projectId ? "loading" : "idle");
  const [failureReason, setFailureReason] = useState<string | null>(null);
  const [reloadSignal, setReloadSignal] = useState(0);

  useEffect(() => {
    setBranches([]);
    setFailureReason(null);
    if (!projectId) {
      setStatus("idle");
      return;
    }

    let isCancelled = false;
    setStatus("loading");
    getProjectBranches(projectId)
      .then((result) => {
        if (isCancelled) return;
        setBranches(result);
        setStatus("loaded");
      })
      .catch((error: unknown) => {
        if (isCancelled) return;
        setFailureReason(error instanceof Error ? error.message : String(error));
        setStatus("failed");
      });

    return () => {
      isCancelled = true;
    };
  }, [projectId, reloadSignal]);

  const reload = useCallback(() => {
    setReloadSignal((count) => count + 1);
  }, []);

  return { branches, status, failureReason, reload };
}
