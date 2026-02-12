import path from "path";
import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";

interface WorktreeSession {
  worktreePath: string;
  sessionName: string;
}

/**
 * git worktree를 생성하고 tmux/zellij 세션을 시작한다.
 * sshHost가 지정되면 원격에서 실행한다.
 */
export async function createWorktreeWithSession(
  projectPath: string,
  branchName: string,
  baseBranch: string,
  sessionType: SessionType,
  sshHost?: string | null
): Promise<WorktreeSession> {
  const projectName = path.basename(projectPath);
  const worktreeBase = path.posix.join(path.dirname(projectPath), `${projectName}__worktrees`);
  const worktreePath = path.posix.join(worktreeBase, branchName.replace(/\//g, "-"));
  const sessionName = `${projectName}-${branchName.replace(/\//g, "-")}`;

  await execGit(
    `git -C "${projectPath}" worktree add "${worktreePath}" -b "${branchName}" "${baseBranch}"`,
    sshHost
  );

  if (sessionType === SessionType.TMUX) {
    await execGit(
      `tmux new-session -d -s "${sessionName}" -c "${worktreePath}"`,
      sshHost
    );
  } else {
    await execGit(
      `cd "${worktreePath}" && zellij --session "${sessionName}" &`,
      sshHost
    );
  }

  return { worktreePath, sessionName };
}

/**
 * worktree와 tmux/zellij 세션을 삭제한다.
 * sshHost가 지정되면 원격에서 실행한다.
 */
export async function removeWorktreeAndSession(
  projectPath: string,
  branchName: string,
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null
): Promise<void> {
  try {
    if (sessionType === SessionType.TMUX) {
      await execGit(`tmux kill-session -t "${sessionName}"`, sshHost);
    } else {
      await execGit(`zellij delete-session "${sessionName}"`, sshHost);
    }
  } catch {
    // 세션이 이미 종료된 경우 무시
  }

  try {
    const worktreeDir = `kanvibe-${branchName.replace(/\//g, "-")}`;
    const worktreePath = path.posix.join(path.dirname(projectPath), worktreeDir);
    await execGit(
      `git -C "${projectPath}" worktree remove "${worktreePath}" --force`,
      sshHost
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

/** 활성 세션 목록을 반환한다 */
export async function listActiveSessions(
  sessionType: SessionType,
  sshHost?: string | null
): Promise<string[]> {
  try {
    if (sessionType === SessionType.TMUX) {
      const output = await execGit(
        "tmux list-sessions -F '#{session_name}'",
        sshHost
      );
      return output.split("\n").filter(Boolean);
    } else {
      const output = await execGit("zellij list-sessions", sshHost);
      return output.split("\n").filter(Boolean);
    }
  } catch {
    return [];
  }
}

/** 세션이 활성 상태인지 확인한다 */
export async function isSessionAlive(
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null
): Promise<boolean> {
  const sessions = await listActiveSessions(sessionType, sshHost);
  return sessions.some((s) => s.includes(sessionName));
}
