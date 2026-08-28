import { execFile } from "child_process";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const VS_CODE_COMMAND = "code";
const VS_CODE_LAUNCH_TIMEOUT_MS = 10_000;

export interface EditorOpenResult {
  ok: boolean;
  error?: string;
}

/**
 * VS Code CLI 인자를 만든다.
 * 원격 작업이라도 명령은 항상 로컬에서 돈다. 원격 셸에서 `code`를 실행하면 그 머신에서 창을 띄우려 하고,
 * Remote-SSH의 `code` 셔틀은 이미 Remote-SSH로 붙은 통합 터미널 안에서만 로컬로 넘어오기 때문이다.
 * 그래서 원격은 로컬 VS Code에 `--remote ssh-remote+<host>`로 붙을 곳을 알려 주는 방향으로 뒤집는다.
 */
export function buildVsCodeOpenArgs(targetPath: string, sshHost: string | null): string[] {
  return sshHost ? ["--remote", `ssh-remote+${sshHost}`, targetPath] : [targetPath];
}

/**
 * 작업을 열 경로와 호스트를 정한다.
 * worktree가 있으면 그 작업의 실제 작업 사본이므로 프로젝트 저장소보다 먼저 쓴다.
 */
async function resolveTaskEditorTarget(taskId: string): Promise<{ targetPath: string; sshHost: string | null } | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOneBy({ id: taskId });
  if (!task) {
    return null;
  }

  const project = task.projectId
    ? await (await getProjectRepository()).findOneBy({ id: task.projectId })
    : null;
  const targetPath = task.worktreePath || project?.repoPath || null;
  if (!targetPath) {
    return null;
  }

  return { targetPath, sshHost: task.sshHost || project?.sshHost || null };
}

/** 작업 폴더를 로컬 VS Code로 연다. 원격 작업이면 Remote-SSH로 붙어서 연다 */
export async function openTaskInVsCode(taskId: string): Promise<EditorOpenResult> {
  const target = await resolveTaskEditorTarget(taskId);
  if (!target) {
    return { ok: false, error: "no-target-path" };
  }

  return new Promise((resolve) => {
    execFile(
      VS_CODE_COMMAND,
      buildVsCodeOpenArgs(target.targetPath, target.sshHost),
      { env: createLocalShellEnvironment(), timeout: VS_CODE_LAUNCH_TIMEOUT_MS },
      (error) => {
        if (error) {
          console.error("VS Code 실행 실패:", error);
          resolve({ ok: false, error: error.message });
          return;
        }

        resolve({ ok: true });
      },
    );
  });
}
