import path from "path";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";
import { execGit } from "@/lib/gitOperations";
import { getEffectivePaneLayout } from "@/app/actions/paneLayout";

interface WorktreeSession {
  worktreePath: string;
  sessionName: string;
}

/** branchName을 tmux window / zellij tab 이름으로 변환한다 */
export function formatWindowName(branchName: string): string {
  return ` ${branchName.replace(/\//g, "-")}`;
}

/**
 * tmux window에 pane 레이아웃을 적용하고 각 pane에 시작 명령어를 실행한다.
 * 분할 실패 시에도 기본 window는 유지된다 (graceful fallback).
 */
async function applyPaneLayout(
  sessionName: string,
  windowName: string,
  layoutType: PaneLayoutType,
  panes: PaneCommand[],
  worktreePath: string,
): Promise<void> {
  const target = `"${sessionName}":"${windowName}"`;

  /** 레이아웃 타입에 따른 tmux split 명령어 시퀀스 */
  const splitCommands: Record<PaneLayoutType, string[]> = {
    [PaneLayoutType.SINGLE]: [],
    [PaneLayoutType.HORIZONTAL_2]: [
      `tmux split-window -v -t ${target} -c "${worktreePath}"`,
    ],
    [PaneLayoutType.VERTICAL_2]: [
      `tmux split-window -h -t ${target} -c "${worktreePath}"`,
    ],
    [PaneLayoutType.LEFT_RIGHT_TB]: [
      `tmux split-window -h -t ${target} -c "${worktreePath}"`,
      `tmux split-window -v -t ${target}.1 -c "${worktreePath}"`,
    ],
    [PaneLayoutType.LEFT_TB_RIGHT]: [
      `tmux split-window -h -t ${target} -c "${worktreePath}"`,
      `tmux split-window -v -t ${target}.0 -c "${worktreePath}"`,
    ],
    [PaneLayoutType.QUAD]: [
      `tmux split-window -h -t ${target} -c "${worktreePath}"`,
      `tmux split-window -v -t ${target}.0 -c "${worktreePath}"`,
      `tmux split-window -v -t ${target}.2 -c "${worktreePath}"`,
    ],
  };

  const commands = splitCommands[layoutType];
  for (const cmd of commands) {
    await execGit(cmd);
  }

  /** 각 pane에 시작 명령어 전송 */
  for (const pane of panes) {
    if (pane.command.trim()) {
      await execGit(
        `tmux send-keys -t ${target}.${pane.position} "${pane.command}" Enter`,
      );
    }
  }
}

/**
 * pane 레이아웃을 백그라운드에서 적용한다.
 * task 생성 흐름을 차단하지 않으며, 실패해도 기본 window는 유지된다.
 */
function applyPaneLayoutAsync(
  sessionName: string,
  windowName: string,
  worktreePath: string,
  projectId?: string,
): void {
  (async () => {
    try {
      const layoutConfig = await getEffectivePaneLayout(projectId);
      if (layoutConfig && layoutConfig.layoutType !== PaneLayoutType.SINGLE) {
        await applyPaneLayout(
          sessionName,
          windowName,
          layoutConfig.layoutType as PaneLayoutType,
          layoutConfig.panes,
          worktreePath,
        );
      }
    } catch (error) {
      console.error("Pane 레이아웃 적용 실패 (기본 window 유지):", error);
    }
  })();
}

/**
 * git worktree를 생성하고 메인 세션에 tmux window / zellij tab을 추가한다.
 * 메인 세션이 없으면 자동 생성한다. sshHost가 지정되면 원격에서 실행한다.
 * tmux인 경우 projectId를 기반으로 pane 레이아웃 설정을 적용한다.
 */
export async function createWorktreeWithSession(
  projectPath: string,
  branchName: string,
  baseBranch: string,
  sessionType: SessionType,
  sshHost?: string | null,
  projectId?: string | null,
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

    /** 로컬 tmux인 경우 pane 레이아웃을 백그라운드로 적용 (task 생성을 차단하지 않음) */
    if (!sshHost) {
      applyPaneLayoutAsync(
        sessionName,
        windowName,
        worktreePath,
        projectId ?? undefined,
      );
    }
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
 * 기존 디렉토리에 tmux window / zellij tab을 생성한다.
 * worktree를 생성하지 않고, 지정된 작업 디렉토리를 사용한다.
 */
export async function createSessionWithoutWorktree(
  projectPath: string,
  branchName: string,
  sessionType: SessionType,
  sshHost?: string | null,
  workingDir?: string,
): Promise<{ sessionName: string }> {
  const sessionName = path.basename(projectPath);
  const windowName = formatWindowName(branchName);
  const cwd = workingDir || projectPath;

  if (sessionType === SessionType.TMUX) {
    await execGit(
      `tmux has-session -t "${sessionName}" 2>/dev/null || tmux new-session -d -s "${sessionName}"`,
      sshHost,
    );
    await execGit(
      `tmux new-window -t "${sessionName}" -n "${windowName}" -c "${cwd}"`,
      sshHost,
    );
  } else {
    try {
      await execGit(
        `zellij list-sessions 2>/dev/null | grep -q "^${sessionName}$"`,
        sshHost,
      );
    } catch {
      await execGit(
        `cd "${cwd}" && zellij --session "${sessionName}" &`,
        sshHost,
      );
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
    await execGit(
      `zellij action --session "${sessionName}" new-tab --name "${windowName}" --cwd "${cwd}"`,
      sshHost,
    );
  }

  return { sessionName };
}

/** tmux window / zellij tab만 제거한다. worktree와 브랜치는 삭제하지 않는다 */
export async function removeSessionOnly(
  sessionType: SessionType,
  sessionName: string,
  branchName: string,
  sshHost?: string | null,
): Promise<void> {
  const windowName = formatWindowName(branchName);

  try {
    if (sessionType === SessionType.TMUX) {
      await execGit(
        `tmux kill-window -t "${sessionName}:${windowName}"`,
        sshHost,
      );
    } else {
      await execGit(
        `zellij action --session "${sessionName}" go-to-tab-name "${windowName}" && zellij action --session "${sessionName}" close-tab`,
        sshHost,
      );
    }
  } catch {
    // window/tab이 이미 종료된 경우 무시
  }
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
  return windows.some((w) => w.trimEnd() === windowName);
}
