import { execGit } from "@/lib/gitOperations";
import { getHookServerUrl } from "@/lib/hookEndpoint";
import { quoteShellArgument, readTextFile } from "@/lib/hostFileAccess";
import { getKanvibeTargetsPath, parseKanvibeTargets } from "@/lib/kanvibeProjectState";
import { getTaskCliPath, TASK_CLI_PEER_RESULT_PREFIX } from "@/lib/taskCliScript";

/** 다른 client 하나에 보낸 전파 요청의 결과 */
export interface PeerTargetResult {
  url: string;
  isSuccess: boolean;
  httpStatus: string;
  message: string;
}

/**
 * 전파 요청은 worktree가 있는 호스트에서 task CLI로 보낸다.
 * targets.json의 주소는 그 호스트에서 각 client로 가는 경로이고, 토큰도 그 호스트의 파일에서 바로 읽혀
 * 명령줄에 실리지 않기 때문이다. 원격 task 생성이 끝날 때까지 기다리는 client가 있을 수 있어 넉넉히 잡는다.
 */
const PEER_PROPAGATION_TIMEOUT_MS = 10 * 60_000;

/** 같은 worktree를 공유하는 다른 KanVibe client에 task 삭제를 전파한다. 그 client는 다시 전파하지 않는다 */
export async function propagateTaskDeletionToPeers(
  worktreePath: string,
  sshHost?: string | null,
): Promise<PeerTargetResult[]> {
  return runPeerPropagation(worktreePath, sshHost, "propagate-delete", []);
}

/**
 * 다른 client에 새 브랜치 worktree를 task로 등록하라고 전파한다.
 * 프로젝트 루트의 targets.json에는 그 프로젝트를 다루는 모든 client와 각 client의 루트 task가 있다.
 */
export async function propagateTaskRegistrationToPeers(
  projectRepoPath: string,
  branchName: string,
  sshHost?: string | null,
): Promise<PeerTargetResult[]> {
  return runPeerPropagation(projectRepoPath, sshHost, "propagate-register", ["--branch", branchName]);
}

async function runPeerPropagation(
  repoPath: string,
  sshHost: string | null | undefined,
  commandName: "propagate-delete" | "propagate-register",
  extraArguments: string[],
): Promise<PeerTargetResult[]> {
  const ownUrl = await getHookServerUrl(sshHost);
  const peerUrls = await readPeerTargetUrls(repoPath, ownUrl, sshHost);
  if (peerUrls.length === 0) {
    return [];
  }

  try {
    const commandArguments = [commandName, "--exclude-url", ownUrl, ...extraArguments].map(quoteShellArgument).join(" ");
    const output = await execGit(
      `bash ${quoteShellArgument(getTaskCliPath(repoPath, sshHost))} ${commandArguments}`,
      sshHost,
      { timeoutMs: PEER_PROPAGATION_TIMEOUT_MS },
    );
    return parsePeerTargetResults(output);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.warn("[task-cli] 다른 client로 전파하지 못했습니다", { repoPath, sshHost: sshHost ?? null, commandName, message });
    return peerUrls.map((url) => ({ url, isSuccess: false, httpStatus: "000", message }));
  }
}

/** 실행 전에 전파할 상대가 있는지 본다. 혼자 쓰는 worktree에서는 task CLI를 부르지 않는다 */
async function readPeerTargetUrls(repoPath: string, ownUrl: string, sshHost?: string | null): Promise<string[]> {
  const normalizedOwnUrl = ownUrl.replace(/\/+$/, "");
  const targetsContent = await readTextFile(getKanvibeTargetsPath(repoPath, sshHost), sshHost);

  return parseKanvibeTargets(targetsContent).targets
    .map((target) => target.url)
    .filter((url) => url !== normalizedOwnUrl);
}

function parsePeerTargetResults(output: string): PeerTargetResult[] {
  return output.split("\n")
    .filter((line) => line.startsWith(`${TASK_CLI_PEER_RESULT_PREFIX}\t`))
    .map((line) => {
      const [, url = "", httpStatus = "000", message = ""] = line.split("\t");
      return { url, httpStatus, message, isSuccess: httpStatus.startsWith("2") };
    });
}
