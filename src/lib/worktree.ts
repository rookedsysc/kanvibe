import path from "path";
import { writeFile } from "fs/promises";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";
import { execGit } from "@/lib/gitOperations";
import { getEffectivePaneLayout } from "@/app/actions/paneLayout";

interface WorktreeSession {
  worktreePath: string;
  sessionName: string;
}

/** branchName을 세션 이름으로 변환한다. `/`를 `-`로 치환한다 */
export function formatSessionName(projectName: string, branchName: string): string {
  return `${projectName}-${branchName}`.replace(/\//g, "-");
}

/** zellij 세션 이름을 소켓 경로 108바이트 제한에 맞게 truncate한다 */
const ZELLIJ_SESSION_NAME_MAX_LENGTH = 60;
export function sanitizeZellijSessionName(sessionName: string): string {
  if (sessionName.length <= ZELLIJ_SESSION_NAME_MAX_LENGTH) return sessionName;
  return sessionName.slice(0, ZELLIJ_SESSION_NAME_MAX_LENGTH);
}


/**
 * tmux 세션에 pane 레이아웃을 적용하고 각 pane에 시작 명령어를 실행한다.
 * 분할 실패 시에도 기본 window는 유지된다 (graceful fallback).
 */
async function applyPaneLayout(
  sessionName: string,
  layoutType: PaneLayoutType,
  panes: PaneCommand[],
  worktreePath: string,
): Promise<void> {
  const target = `"${sessionName}":0`;

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

/** KDL 문자열 내 특수문자를 이스케이프한다 */
function escapeKdl(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

/**
 * PaneLayoutType과 PaneCommand[]를 기반으로 Zellij KDL 레이아웃 문자열을 생성한다.
 * 세션 생성 시 --layout 플래그로 전달하여 pane 분할과 명령어 실행을 원자적으로 처리한다.
 */
export function generateZellijLayoutKdl(
  layoutType: PaneLayoutType,
  panes: PaneCommand[],
  worktreePath: string,
): string {
  const paneMap = new Map(panes.map((p) => [p.position, p.command]));
  const cwdEscaped = escapeKdl(worktreePath);

  /** position에 해당하는 pane의 KDL 노드를 생성한다 */
  function renderPane(position: number, indent: string): string {
    const command = paneMap.get(position)?.trim();
    if (!command) {
      return `${indent}pane cwd="${cwdEscaped}"`;
    }
    return [
      `${indent}pane command="bash" {`,
      `${indent}    args "-c" "${escapeKdl(command)}"`,
      `${indent}    cwd "${cwdEscaped}"`,
      `${indent}}`,
    ].join("\n");
  }

  switch (layoutType) {
    case PaneLayoutType.SINGLE:
      return ["layout {", renderPane(0, "    "), "}"].join("\n");

    case PaneLayoutType.HORIZONTAL_2:
      return [
        "layout {",
        renderPane(0, "    "),
        renderPane(1, "    "),
        "}",
      ].join("\n");

    case PaneLayoutType.VERTICAL_2:
      return [
        "layout {",
        '    pane split_direction="vertical" {',
        renderPane(0, "        "),
        renderPane(1, "        "),
        "    }",
        "}",
      ].join("\n");

    case PaneLayoutType.LEFT_RIGHT_TB:
      return [
        "layout {",
        '    pane split_direction="vertical" {',
        renderPane(0, "        "),
        "        pane {",
        renderPane(1, "            "),
        renderPane(2, "            "),
        "        }",
        "    }",
        "}",
      ].join("\n");

    case PaneLayoutType.LEFT_TB_RIGHT:
      return [
        "layout {",
        '    pane split_direction="vertical" {',
        "        pane {",
        renderPane(0, "            "),
        renderPane(1, "            "),
        "        }",
        renderPane(2, "        "),
        "    }",
        "}",
      ].join("\n");

    case PaneLayoutType.QUAD:
      return [
        "layout {",
        '    pane split_direction="vertical" {',
        "        pane {",
        renderPane(0, "            "),
        renderPane(2, "            "),
        "        }",
        "        pane {",
        renderPane(1, "            "),
        renderPane(3, "            "),
        "        }",
        "    }",
        "}",
      ].join("\n");
  }
}

/** Zellij KDL 레이아웃 파일의 기본 파일명 */
export const ZELLIJ_LAYOUT_FILENAME = ".zellij-layout.kdl";

/**
 * KDL 레이아웃 파일을 worktree 디렉토리에 저장한다.
 * 터미널 연결 시 node-pty가 이 파일을 --layout 플래그로 사용한다.
 */
async function writeLayoutToWorktree(
  worktreePath: string,
  kdlContent: string,
): Promise<void> {
  const layoutPath = path.join(worktreePath, ZELLIJ_LAYOUT_FILENAME);
  await writeFile(layoutPath, kdlContent, "utf-8");
}

/**
 * pane 레이아웃을 백그라운드에서 적용한다.
 * task 생성 흐름을 차단하지 않으며, 실패해도 기본 window는 유지된다.
 */
function applyPaneLayoutAsync(
  sessionName: string,
  worktreePath: string,
  projectId?: string,
): void {
  (async () => {
    try {
      const layoutConfig = await getEffectivePaneLayout(projectId);
      if (layoutConfig && layoutConfig.layoutType !== PaneLayoutType.SINGLE) {
        await applyPaneLayout(
          sessionName,
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
 * git worktree를 생성하고 브랜치별 독립 세션을 생성한다.
 * 세션이 없으면 자동 생성한다. sshHost가 지정되면 원격에서 실행한다.
 * 로컬 세션인 경우 projectId를 기반으로 pane 레이아웃 설정을 적용한다.
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
  const sessionName = formatSessionName(projectName, branchName);

  await execGit(
    `git -C "${projectPath}" worktree add "${worktreePath}" -b "${branchName}" "${baseBranch}"`,
    sshHost,
  );

  if (sessionType === SessionType.TMUX) {
    /** 동일 이름의 세션이 없을 때만 생성한다 */
    const hasSession = await isSessionAlive(sessionType, sessionName, sshHost);
    if (!hasSession) {
      await execGit(
        `tmux new-session -d -s "${sessionName}" -c "${worktreePath}"`,
        sshHost,
      );
    }

    /** 로컬 tmux인 경우 pane 레이아웃을 백그라운드로 적용 (task 생성을 차단하지 않음) */
    if (!sshHost) {
      applyPaneLayoutAsync(
        sessionName,
        worktreePath,
        projectId ?? undefined,
      );
    }

    return { worktreePath, sessionName };
  } else {
    /**
     * Zellij는 TTY 없이 실행 불가하므로 서버에서 세션을 직접 시작하지 않는다.
     * 세션 이름과 레이아웃 파일만 준비하고, 실제 세션 생성은
     * 터미널 연결 시 node-pty가 PTY를 제공하며 처리한다.
     */
    const zellijSessionName = sanitizeZellijSessionName(sessionName);

    /** 로컬 세션인 경우 KDL 레이아웃 파일을 worktree 디렉토리에 저장한다 */
    if (!sshHost) {
      try {
        const layoutConfig = await getEffectivePaneLayout(projectId ?? undefined);
        if (layoutConfig && layoutConfig.layoutType !== PaneLayoutType.SINGLE) {
          const kdl = generateZellijLayoutKdl(
            layoutConfig.layoutType as PaneLayoutType,
            layoutConfig.panes,
            worktreePath,
          );
          await writeLayoutToWorktree(worktreePath, kdl);
        }
      } catch (error) {
        console.error("Zellij 레이아웃 파일 생성 실패 (레이아웃 없이 세션 생성 예정):", error);
      }
    }

    return { worktreePath, sessionName: zellijSessionName };
  }
}

/**
 * 기존 디렉토리에 브랜치별 독립 세션을 생성한다.
 * worktree를 생성하지 않고, 지정된 작업 디렉토리를 사용한다.
 */
export async function createSessionWithoutWorktree(
  projectPath: string,
  branchName: string,
  sessionType: SessionType,
  sshHost?: string | null,
  workingDir?: string,
): Promise<{ sessionName: string }> {
  const projectName = path.basename(projectPath);
  const sessionName = formatSessionName(projectName, branchName);
  const cwd = workingDir || projectPath;

  if (sessionType === SessionType.TMUX) {
    const hasSession = await isSessionAlive(sessionType, sessionName, sshHost);
    if (!hasSession) {
      await execGit(
        `tmux new-session -d -s "${sessionName}" -c "${cwd}"`,
        sshHost,
      );
    }

    return { sessionName };
  } else {
    /**
     * Zellij는 TTY 없이 실행 불가하므로 세션 이름만 반환한다.
     * 실제 세션 생성은 터미널 연결 시 node-pty가 처리한다.
     */
    const zellijSessionName = sanitizeZellijSessionName(sessionName);
    return { sessionName: zellijSessionName };
  }
}

/** worktree와 브랜치를 삭제한다. 세션은 건드리지 않는다 */
export async function removeWorktreeAndBranch(
  projectPath: string,
  branchName: string,
  sshHost?: string | null,
): Promise<void> {
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

/** 브랜치별 독립 세션을 종료한다. worktree와 브랜치는 삭제하지 않는다 */
export async function removeSessionOnly(
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null,
): Promise<void> {
  try {
    if (sessionType === SessionType.TMUX) {
      await execGit(
        `tmux kill-session -t "${sessionName}"`,
        sshHost,
      );
    } else {
      await execGit(
        `zellij kill-session "${sessionName}" 2>/dev/null || zellij delete-session "${sessionName}" 2>/dev/null`,
        sshHost,
      );
    }
  } catch {
    // 세션이 이미 종료된 경우 무시
  }
}

/**
 * worktree와 브랜치별 독립 세션을 삭제한다.
 * sshHost가 지정되면 원격에서 실행한다.
 */
export async function removeWorktreeAndSession(
  projectPath: string,
  branchName: string,
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null,
): Promise<void> {
  await removeSessionOnly(sessionType, sessionName, sshHost);
  await removeWorktreeAndBranch(projectPath, branchName, sshHost);
}

/** 활성 tmux/zellij 세션 이름 목록을 반환한다 */
export async function listActiveSessions(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<string[]> {
  try {
    if (sessionType === SessionType.TMUX) {
      const output = await execGit(
        `tmux list-sessions -F '#{session_name}'`,
        sshHost,
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
  sshHost?: string | null,
): Promise<boolean> {
  try {
    if (sessionType === SessionType.TMUX) {
      await execGit(`tmux has-session -t "${sessionName}" 2>/dev/null`, sshHost);
      return true;
    } else {
      const output = await execGit("zellij list-sessions", sshHost);
      return output.split("\n").some((s) => s.trim().startsWith(sessionName));
    }
  } catch {
    return false;
  }
}
