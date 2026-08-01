import { ensureAiToolPatternsExcluded } from "@/lib/gitExclude";
import {
  hasKanvibeHookTarget,
  parseKanvibeTargets,
  upsertKanvibeHookTarget,
} from "@/lib/kanvibeProjectState";

/**
 * hook을 설치하는 client를 알림 대상으로 등록하고 `.kanvibe` 상태 디렉터리를 git 추적에서 제외한다.
 * 같은 client가 다시 설치하면 taskId만 교체되고, 다른 client는 targets.json에 추가된다.
 */
export async function registerKanvibeHookTarget(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
  sshHost?: string | null,
): Promise<void> {
  await ensureAiToolPatternsExcluded(repoPath, sshHost);
  await upsertKanvibeHookTarget(repoPath, { url: hookServerUrl, taskId }, sshHost);
}

/**
 * hook 설치 여부는 스크립트에 박힌 TASK_ID가 아니라 `.kanvibe/targets.json`에
 * 현재 client(hook 서버 URL)와 현재 taskId 쌍이 등록되어 있는지로 판정한다.
 * 여러 client가 같은 worktree를 공유해도 서로의 설치 상태를 깨뜨리지 않는다.
 */
export interface HookTargetRegistrationStatus {
  hasRegisteredHookTarget: boolean;
  registeredHookTargetUrl: string | null;
}

export function verifyHookTargetRegistration(
  targetsContent: string,
  taskId: string | undefined,
  expectedHookServerUrl: string | null,
): HookTargetRegistrationStatus {
  if (!taskId) {
    return { hasRegisteredHookTarget: true, registeredHookTargetUrl: null };
  }

  const registeredTarget = parseKanvibeTargets(targetsContent).targets
    .find((target) => target.taskId === taskId);
  const registeredHookTargetUrl = registeredTarget?.url ?? null;

  /** 원격 hook 서버 주소를 확정하지 못한 경우에는 taskId 등록 여부만으로 판정한다 */
  if (!expectedHookServerUrl) {
    return {
      hasRegisteredHookTarget: registeredTarget !== undefined,
      registeredHookTargetUrl,
    };
  }

  return {
    hasRegisteredHookTarget: hasKanvibeHookTarget(targetsContent, {
      url: expectedHookServerUrl,
      taskId,
    }),
    registeredHookTargetUrl,
  };
}
