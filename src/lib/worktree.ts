import path from "path";
import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";

interface WorktreeSession {
  worktreePath: string;
  sessionName: string;
}

/** branchName을 tmux window / zellij tab 이름으로 변환한다 */
export function formatWindowName(branchName: string): string {
  return ` ${branchName.replace(/\//g, "-")}`;
}

/**
 * git worktree를 생성하고 메인 세션에 tmux window / zellij tab을 추가한다.
 * 메인 세션이 없으면 자동 생성한다. sshHost가 지정되면 원격에서 실행한다.
 */
export async function createWorktreeWithSession(
  projectPath: string,
  branchName: string,
  baseBranch: string,
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<WorktreeSession> {
  const projectName = path.basename(projectPath);
  const worktreeBase = path.posix.join(
    path.dirname(projectPath),
    `${projectName}__worktrees`,
  );
  const worktreePath = path.posix.join(
    worktreeBase,
    branchName.replace(/\//g, "-"),
  );
  const sessionName = projectName;
  const windowName = formatWindowName(branchName);

  await execGit(
    `git -C "${projectPath}" worktree add "${worktreePath}" -b "${branchName}" "${baseBranch}"`,
    sshHost,
  );

  if (sessionType === SessionType.TMUX) {
    /** 메인 tmux 세션이 없으면 자동 생성한다 */
    await execGit(
      `tmux has-session -t "${sessionName}" 2>/dev/null || tmux new-session -d -s "${sessionName}"`,
      sshHost,
    );
    /** 메인 세션에 worktree 디렉토리로 이동하는 window를 추가한다 */
    await execGit(
      `tmux new-window -t "${sessionName}" -n "${windowName}" -c "${worktreePath}"`,
      sshHost,
    );
  } else {
    /** 메인 zellij 세션이 없으면 백그라운드로 생성한다 */
    try {
      await execGit(
        `zellij list-sessions 2>/dev/null | grep -q "^${sessionName}$"`,
        sshHost,
      );
    } catch {
      await execGit(
        `cd "${worktreePath}" && zellij --session "${sessionName}" &`,
        sshHost,
      );
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
    /** 메인 세션에 worktree 디렉토리의 tab을 추가한다 */
    await execGit(
      `zellij action --session "${sessionName}" new-tab --name "${windowName}" --cwd "${worktreePath}"`,
      sshHost,
    );
  }

  return { worktreePath, sessionName };
}

/**
 * worktree와 메인 세션의 tmux window / zellij tab을 삭제한다.
 * sshHost가 지정되면 원격에서 실행한다.
 */
export async function removeWorktreeAndSession(
  projectPath: string,
  branchName: string,
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null,
): Promise<void> {
  const windowName = formatWindowName(branchName);

  try {
    if (sessionType === SessionType.TMUX) {
      /** 메인 세션에서 해당 window만 종료한다 */
      await execGit(
        `tmux kill-window -t "${sessionName}:${windowName}"`,
        sshHost,
      );
    } else {
      /** 메인 세션에서 해당 tab으로 이동 후 닫는다 */
      await execGit(
        `zellij action --session "${sessionName}" go-to-tab-name "${windowName}" && zellij action --session "${sessionName}" close-tab`,
        sshHost,
      );
    }
  } catch {
    // window/tab이 이미 종료된 경우 무시
  }

  try {
    const projectName = path.basename(projectPath);
    const worktreeBase = path.posix.join(
      path.dirname(projectPath),
      `${projectName}__worktrees`,
    );
    const worktreePath = path.posix.join(
      worktreeBase,
      branchName.replace(/\//g, "-"),
    );
    await execGit(
      `git -C "${projectPath}" worktree remove "${worktreePath}" --force`,
      sshHost,
    );
  } catch {
    // worktree가 이미 삭제된 경우 무시
  }

  try {
    await execGit(`git -C "${projectPath}" branch -D "${branchName}"`, sshHost);
  } catch {
    // 브랜치가 이미 삭제된 경우 무시
  }
}

/** 메인 세션 내의 활성 window/tab 목록을 반환한다 */
export async function listActiveWindows(
  sessionType: SessionType,
  mainSession: string,
  sshHost?: string | null,
): Promise<string[]> {
  try {
    if (sessionType === SessionType.TMUX) {
      const output = await execGit(
        `tmux list-windows -t "${mainSession}" -F '#{window_name}'`,
        sshHost,
      );
      return output.split("\n").filter(Boolean);
    } else {
      /** zellij는 외부에서 탭 목록을 조회할 수 없으므로 세션 목록을 반환한다 */
      const output = await execGit("zellij list-sessions", sshHost);
      return output.split("\n").filter(Boolean);
    }
  } catch {
    return [];
  }
}

/** window/tab이 활성 상태인지 확인한다 */
export async function isWindowAlive(
  sessionType: SessionType,
  mainSession: string,
  windowName: string,
  sshHost?: string | null,
): Promise<boolean> {
  const windows = await listActiveWindows(sessionType, mainSession, sshHost);
  return windows.some((w) => w.includes(windowName));
}
