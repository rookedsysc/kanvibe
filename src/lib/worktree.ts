import path from "path";
import { writeFile } from "fs/promises";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";
import { execGit } from "@/lib/gitOperations";
import { ensureRemoteSessionDependency } from "@/lib/remoteSessionDependency";
import { getEffectivePaneLayout } from "@/desktop/main/services/paneLayoutService";

interface WorktreeSession {
  worktreePath: string;
  sessionName: string;
}

export interface TmuxPaneLayoutConfig {
  layoutType: PaneLayoutType;
  panes: PaneCommand[];
}

/** branchName을 세션 이름으로 변환한다. `/`를 `-`로 치환한다 */
export function formatSessionName(projectName: string, branchName: string): string {
  return `${projectName}-${branchName}`.replace(/\//g, "-");
}

export function buildManagedWorktreePath(projectPath: string, branchName: string): string {
  const projectName = path.basename(projectPath);
  const worktreeBase = path.posix.join(
    path.dirname(projectPath),
    `${projectName}__worktrees`,
  );

  return path.posix.join(
    worktreeBase,
    branchName.replace(/\//g, "-"),
  );
}

/** zellij 세션 이름을 소켓 경로 108바이트 제한에 맞게 truncate한다 */
const ZELLIJ_SESSION_NAME_MAX_LENGTH = 60;
export function sanitizeZellijSessionName(sessionName: string): string {
  if (sessionName.length <= ZELLIJ_SESSION_NAME_MAX_LENGTH) return sessionName;
  return sessionName.slice(0, ZELLIJ_SESSION_NAME_MAX_LENGTH);
}

function buildTmuxCreateSessionCommand(sessionName: string, workingDir: string): string {
  return `tmux new-session -d -s "${sessionName}" -c "${workingDir}"`;
}

function buildTmuxTarget(sessionName: string): string {
  return `"${sessionName}":0`;
}

export function buildTmuxPaneLayoutCommands(
  sessionName: string,
  layoutType: PaneLayoutType,
  panes: PaneCommand[],
  worktreePath: string,
): string[] {
  const target = buildTmuxTarget(sessionName);

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

  const sendKeysCommands = panes
    .filter((pane) => pane.command.trim())
    .map((pane) => (
      `tmux send-keys -t ${target}.${pane.position} "${pane.command}" Enter`
    ));

  return [
    ...splitCommands[layoutType],
    ...sendKeysCommands,
  ];
}

export function buildTmuxSessionBootstrapCommands(
  sessionName: string,
  workingDir: string,
  paneLayout?: TmuxPaneLayoutConfig | null,
): string[] {
  return [
    buildTmuxCreateSessionCommand(sessionName, workingDir),
    ...(paneLayout && paneLayout.layoutType !== PaneLayoutType.SINGLE
      ? buildTmuxPaneLayoutCommands(
          sessionName,
          paneLayout.layoutType,
          paneLayout.panes,
          workingDir,
        )
      : []),
  ];
}

async function createTmuxSession(
  sessionName: string,
  workingDir: string,
  sshHost?: string | null,
): Promise<void> {
  await execGit(buildTmuxCreateSessionCommand(sessionName, workingDir), sshHost);
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
  sshHost?: string | null,
): Promise<void> {
  for (const command of buildTmuxPaneLayoutCommands(sessionName, layoutType, panes, worktreePath)) {
    await execGit(command, sshHost);
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

function quoteForPosixShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

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
  await ensureRemoteSessionDependency(sessionType, sshHost);

  const projectName = path.basename(projectPath);
  const worktreePath = buildManagedWorktreePath(projectPath, branchName);
  const sessionName = formatSessionName(projectName, branchName);

  await execGit(
    `git -C "${projectPath}" worktree add "${worktreePath}" -b "${branchName}" "${baseBranch}"`,
    sshHost,
  );

  try {
    if (sessionType === SessionType.TMUX) {
      /**
       * 원격 호스트에서는 비대화형 SSH로 tmux 서버를 시작할 수 없으므로
       * 세션 생성을 터미널 연결 시 PTY가 확보된 후로 미룬다.
       */
      if (!sshHost) {
        const hasSession = await isSessionAlive(sessionType, sessionName, sshHost);
        if (!hasSession) {
          await createTmuxSession(sessionName, worktreePath, sshHost);
        }
        applyPaneLayoutAsync(sessionName, worktreePath, projectId ?? undefined);
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
  } catch (sessionError) {
    /** 세션 생성이 실패하면 이미 생성된 worktree와 브랜치를 정리해 다음 시도가 막히지 않도록 한다 */
    await removeWorktreeAndBranch(projectPath, branchName, sshHost);
    throw sessionError;
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
  await ensureRemoteSessionDependency(sessionType, sshHost);

  const projectName = path.basename(projectPath);
  const sessionName = formatSessionName(projectName, branchName);
  const cwd = workingDir || projectPath;

  if (sessionType === SessionType.TMUX) {
    if (!sshHost) {
      const hasSession = await isSessionAlive(sessionType, sessionName, sshHost);
      if (!hasSession) {
        await createTmuxSession(sessionName, cwd, sshHost);
      }
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

interface ResourceCleanupOptions {
  throwOnError?: boolean;
}

/** worktree와 브랜치를 삭제한다. 세션은 건드리지 않는다 */
export async function removeWorktreeAndBranch(
  projectPath: string,
  branchName: string,
  sshHost?: string | null,
  options: ResourceCleanupOptions = {},
): Promise<void> {
  const worktreePath = buildManagedWorktreePath(projectPath, branchName);
  const worktreeCommand = options.throwOnError
    ? `if git -C "${projectPath}" worktree list --porcelain | grep -Fxq "worktree ${worktreePath}"; then git -C "${projectPath}" worktree remove "${worktreePath}" --force; fi`
    : `git -C "${projectPath}" worktree remove "${worktreePath}" --force`;
  const branchCommand = options.throwOnError
    ? `if git -C "${projectPath}" show-ref --verify --quiet "refs/heads/${branchName}"; then git -C "${projectPath}" branch -D "${branchName}"; fi`
    : `git -C "${projectPath}" branch -D "${branchName}"`;

  try {
    await execGit(worktreeCommand, sshHost);
  } catch {
    if (options.throwOnError) {
      throw new Error(`worktree 정리 실패: ${worktreePath}`);
    }
    // worktree가 이미 삭제된 경우 무시
  }

  try {
    await execGit(branchCommand, sshHost);
  } catch {
    if (options.throwOnError) {
      throw new Error(`브랜치 정리 실패: ${branchName}`);
    }
    // 브랜치가 이미 삭제된 경우 무시
  }
}

/** 브랜치별 독립 세션을 종료한다. worktree와 브랜치는 삭제하지 않는다 */
export async function removeSessionOnly(
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null,
  options: ResourceCleanupOptions = {},
): Promise<void> {
  try {
    if (sessionType === SessionType.TMUX) {
      await execGit(
        buildTmuxSessionCleanupCommand(sessionName, options.throwOnError === true),
        sshHost,
      );
    } else {
      await execGit(
        buildZellijSessionCleanupCommand(sessionName, options.throwOnError === true),
        sshHost,
      );
    }
  } catch {
    if (options.throwOnError) {
      throw new Error(`세션 정리 실패: ${sessionName}`);
    }
    // 세션이 이미 종료된 경우 무시
  }
}

function buildTmuxSessionCleanupCommand(sessionName: string, verifyCleanup: boolean): string {
  const target = quoteForPosixShell(sessionName);

  if (!verifyCleanup) {
    return `tmux kill-session -t ${target} 2>/dev/null || true`;
  }

  return [
    "command -v tmux >/dev/null 2>&1 || exit 1",
    `tmux kill-session -t ${target} 2>/dev/null || true`,
    `if tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -Fx -- ${target} >/dev/null; then exit 1; fi`,
  ].join("; ");
}

function buildZellijSessionCleanupCommand(sessionName: string, verifyCleanup: boolean): string {
  const target = quoteForPosixShell(sessionName);
  const commands = [
    `zellij kill-sessions ${target} 2>/dev/null || true`,
    `zellij delete-session ${target} 2>/dev/null || true`,
  ];

  if (!verifyCleanup) {
    return commands.join("; ");
  }

  return [
    "command -v zellij >/dev/null 2>&1 || exit 1",
    ...commands,
    `if zellij list-sessions 2>/dev/null | awk '{ if ($1 == "EXITED:") print $2; else print $1 }' | grep -Fx -- ${target} >/dev/null; then exit 1; fi`,
  ].join("; ");
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
