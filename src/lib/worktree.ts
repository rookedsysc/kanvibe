import path from "path";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";
import { execGit, listWorktrees, type WorktreeInfo } from "@/lib/gitOperations";
import { writeTextFile } from "@/lib/hostFileAccess";
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

export function formatProjectBranchSessionName(projectPath: string, branchName: string): string {
  return formatSessionName(path.basename(projectPath), branchName);
}

export function buildManagedWorktreePath(projectPath: string, branchName: string): string {
  const worktreeBase = buildManagedWorktreeBasePath(projectPath);

  return path.posix.join(
    worktreeBase,
    branchName.replace(/\//g, "-"),
  );
}

function buildManagedWorktreeBasePath(projectPath: string): string {
  const projectName = path.basename(projectPath);
  return path.posix.join(
    path.dirname(projectPath),
    `${projectName}__worktrees`,
  );
}

/** zellij 세션 이름을 소켓 경로 108바이트 제한에 맞게 truncate한다 */
const ZELLIJ_SESSION_NAME_MAX_LENGTH = 60;
export function sanitizeZellijSessionName(sessionName: string): string {
  if (sessionName.length <= ZELLIJ_SESSION_NAME_MAX_LENGTH) return sessionName;
  return sessionName.slice(0, ZELLIJ_SESSION_NAME_MAX_LENGTH);
}

export function quoteForPosixShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

/**
 * KanVibe 1.0.6까지 원격 세션이 쓰던 격리 소켓.
 * 새 세션은 기본 소켓에만 만들고, 조회와 정리에서만 이 소켓에 남은 세션을 흡수한다.
 */
const LEGACY_TMUX_SOCKET_NAME = "kanvibe";

/** 세션을 찾아야 하는 tmux 소켓 목록. 기본 소켓이 먼저다 */
function buildTmuxLookupCommands(): string[] {
  return ["tmux", `tmux -L ${quoteForPosixShell(LEGACY_TMUX_SOCKET_NAME)}`];
}

/** 작업 디렉터리를 모르는 원격 세션은 -c 없이 만들어 tmux 기본 디렉터리를 쓰게 한다 */
function buildTmuxCreateSessionArgument(sessionName: string, workingDir: string): string {
  const createArgument = `new-session -d -s ${quoteForPosixShell(sessionName)}`;
  return workingDir ? `${createArgument} -c ${quoteForPosixShell(workingDir)}` : createArgument;
}

function buildTmuxWindowTarget(sessionName: string): string {
  return quoteForPosixShell(`${sessionName}:0`);
}

function buildTmuxPaneTarget(sessionName: string, position: number): string {
  return quoteForPosixShell(`${sessionName}:0.${position}`);
}

export function buildTmuxPaneLayoutArguments(
  sessionName: string,
  layoutType: PaneLayoutType,
  panes: PaneCommand[],
  worktreePath: string,
): string[] {
  const target = buildTmuxWindowTarget(sessionName);

  const splitArguments: Record<PaneLayoutType, string[]> = {
    [PaneLayoutType.SINGLE]: [],
    [PaneLayoutType.HORIZONTAL_2]: [
      `split-window -v -t ${target} -c ${quoteForPosixShell(worktreePath)}`,
    ],
    [PaneLayoutType.VERTICAL_2]: [
      `split-window -h -t ${target} -c ${quoteForPosixShell(worktreePath)}`,
    ],
    [PaneLayoutType.LEFT_RIGHT_TB]: [
      `split-window -h -t ${target} -c ${quoteForPosixShell(worktreePath)}`,
      `split-window -v -t ${buildTmuxPaneTarget(sessionName, 1)} -c ${quoteForPosixShell(worktreePath)}`,
    ],
    [PaneLayoutType.LEFT_TB_RIGHT]: [
      `split-window -h -t ${target} -c ${quoteForPosixShell(worktreePath)}`,
      `split-window -v -t ${buildTmuxPaneTarget(sessionName, 0)} -c ${quoteForPosixShell(worktreePath)}`,
    ],
    [PaneLayoutType.QUAD]: [
      `split-window -h -t ${target} -c ${quoteForPosixShell(worktreePath)}`,
      `split-window -v -t ${buildTmuxPaneTarget(sessionName, 0)} -c ${quoteForPosixShell(worktreePath)}`,
      `split-window -v -t ${buildTmuxPaneTarget(sessionName, 2)} -c ${quoteForPosixShell(worktreePath)}`,
    ],
  };

  const sendKeysArguments = panes
    .filter((pane) => pane.command.trim())
    .map((pane) => (
      `send-keys -t ${buildTmuxPaneTarget(sessionName, pane.position)} -- ${quoteForPosixShell(pane.command)} Enter`
    ));

  return [
    ...splitArguments[layoutType],
    ...sendKeysArguments,
  ];
}

export interface TmuxSessionBootstrapOptions {
  /** 같은 tmux 호출에서 attach까지 이어간다. 원격 SSH가 단일 명령으로 세션을 열 때 사용한다 */
  attachAfterBootstrap?: boolean;
  /**
   * 사용자 tmux 설정이 세션 기동을 막을 때 `-f /dev/null`로 다시 시도하는 경로.
   * tmux는 설정 파일을 서버 기동 시 한 번만 읽으므로, 이 플래그는 서버가 아직 없을 때만 실제로 설정을 건너뛴다.
   * 사용자가 다른 세션을 쓰고 있어 서버가 살아 있으면 첫 시도와 같은 조건으로 실행된다.
   */
  withoutUserConfigFile?: boolean;
}

/**
 * KanVibe가 만드는 tmux 세션에만 적용할 안정화 옵션.
 * destroy-unattached를 끄지 않으면 사용자 설정에 따라 detached 부트스트랩 세션이 생성 직후 사라진다.
 *
 * 세션 스코프(`-t`)로 좁힐 수 있는 것만 둔다. 기본 소켓은 사용자 자신의 tmux 서버이므로,
 * 서버 스코프(`-s`) 옵션을 건드리면 KanVibe와 무관한 세션까지 함께 바뀐다.
 * tmux 안에서 애플리케이션의 OSC 52 복사가 동작하려면 서버 옵션 `set-clipboard`가 `on`이어야 하는데,
 * 그 선택은 사용자 `~/.tmux.conf`의 몫으로 남긴다.
 */
function buildTmuxSessionHardeningArguments(sessionName: string): string[] {
  const target = quoteForPosixShell(sessionName);

  return [
    `set-option -t ${target} destroy-unattached off`,
    /** 웹 터미널 크기가 다른 클라이언트에 묶이지 않도록 최근 활성 클라이언트를 기준으로 삼는다 */
    `set-option -t ${target} window-size latest`,
  ];
}

/**
 * 세션 생성과 안정화, 레이아웃, 선택적 attach를 tmux 호출 한 번으로 실행하는 명령을 만든다.
 * 한 명령 시퀀스로 묶어야 사용자 설정이 세션을 없애기 전에 안정화 옵션이 적용된다.
 */
export function buildTmuxSessionBootstrapCommand(
  sessionName: string,
  workingDir: string,
  paneLayout?: TmuxPaneLayoutConfig | null,
  options: TmuxSessionBootstrapOptions = {},
): string {
  const tmuxArguments = [
    buildTmuxCreateSessionArgument(sessionName, workingDir),
    ...buildTmuxSessionHardeningArguments(sessionName),
    /** pane 분할은 각 pane의 작업 디렉터리를 지정해야 하므로 작업 디렉터리를 아는 경우에만 적용한다 */
    ...(workingDir && paneLayout && paneLayout.layoutType !== PaneLayoutType.SINGLE
      ? buildTmuxPaneLayoutArguments(
          sessionName,
          paneLayout.layoutType,
          paneLayout.panes,
          workingDir,
        )
      : []),
    ...(options.attachAfterBootstrap
      ? [`attach-session -t ${quoteForPosixShell(sessionName)}`]
      : []),
  ];

  const tmuxCommand = options.withoutUserConfigFile ? "tmux -f /dev/null" : "tmux";
  return `${tmuxCommand} ${tmuxArguments.join(" \\; ")}`;
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
 * 터미널 연결 시 zellij가 이 파일을 --new-session-with-layout 플래그로 사용한다.
 */
async function writeLayoutToWorktree(
  worktreePath: string,
  kdlContent: string,
  sshHost?: string | null,
): Promise<void> {
  const layoutPath = sshHost
    ? path.posix.join(worktreePath, ZELLIJ_LAYOUT_FILENAME)
    : path.join(worktreePath, ZELLIJ_LAYOUT_FILENAME);
  await writeTextFile(layoutPath, kdlContent, sshHost);
}

/**
 * git worktree를 생성하고 브랜치별 독립 세션을 생성한다.
 * 세션은 터미널 연결 시점에 생성한다.
 * 로컬 Zellij 세션인 경우 연결 시점에 사용할 KDL 레이아웃 파일만 준비한다.
 */
export async function createWorktreeWithSession(
  projectPath: string,
  branchName: string,
  baseBranch: string,
  sessionType: SessionType,
  sshHost?: string | null,
  projectId?: string | null,
): Promise<WorktreeSession> {
  const worktreePath = buildManagedWorktreePath(projectPath, branchName);
  const sessionName = formatProjectBranchSessionName(projectPath, branchName);

  await execGit(
    `git -C "${projectPath}" worktree add "${worktreePath}" -b "${branchName}" "${baseBranch}"`,
    sshHost,
  );

  try {
    if (sessionType === SessionType.TMUX) {
      return { worktreePath, sessionName };
    } else {
      /**
       * Zellij는 TTY 없이 실행 불가하므로 서버에서 세션을 직접 시작하지 않는다.
       * 세션 이름과 레이아웃 파일만 준비하고, 실제 세션 생성은
       * 터미널 연결 시 node-pty가 PTY를 제공하며 처리한다.
       */
      const zellijSessionName = sanitizeZellijSessionName(sessionName);

      /** 원격도 로컬과 같은 레이아웃으로 열리도록 KDL 레이아웃 파일을 worktree 디렉토리에 저장한다 */
      try {
        const layoutConfig = await getEffectivePaneLayout(projectId ?? undefined);
        if (layoutConfig && layoutConfig.layoutType !== PaneLayoutType.SINGLE) {
          const kdl = generateZellijLayoutKdl(
            layoutConfig.layoutType as PaneLayoutType,
            layoutConfig.panes,
            worktreePath,
          );
          await writeLayoutToWorktree(worktreePath, kdl, sshHost);
        }
      } catch (error) {
        console.error("Zellij 레이아웃 파일 생성 실패 (레이아웃 없이 세션 생성 예정):", error);
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
  _sshHost?: string | null,
  _workingDir?: string,
): Promise<{ sessionName: string }> {
  void _sshHost;
  void _workingDir;

  const sessionName = formatProjectBranchSessionName(projectPath, branchName);

  if (sessionType === SessionType.TMUX) {
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
  worktreePath?: string | null;
}

interface NormalizedManagedWorktreePaths {
  projectPath: string;
  basePath: string;
  managedPath: string;
}

interface ResolvedBranchWorktree {
  path: string | null;
  isProjectRootCheckout: boolean;
  registeredWorktrees: WorktreeInfo[];
}

function normalizeGitWorktreePath(worktreePath: string): string {
  const normalized = path.posix.normalize(worktreePath);
  return normalized.replace(/\/+$/, "") || normalized;
}

function getNormalizedManagedWorktreePaths(
  projectPath: string,
  branchName: string,
): NormalizedManagedWorktreePaths {
  return {
    projectPath: normalizeGitWorktreePath(projectPath),
    basePath: normalizeGitWorktreePath(buildManagedWorktreeBasePath(projectPath)),
    managedPath: normalizeGitWorktreePath(buildManagedWorktreePath(projectPath, branchName)),
  };
}

function getManagedCleanupCandidatePaths(
  projectPath: string,
  branchName: string,
  candidates: Array<string | null | undefined>,
): string[] {
  const managedPaths = getNormalizedManagedWorktreePaths(projectPath, branchName);
  const normalizedCandidates = candidates
    .filter((candidate): candidate is string => Boolean(candidate?.trim()))
    .map(normalizeGitWorktreePath);
  const allowedPaths = new Set([managedPaths.managedPath, ...normalizedCandidates]);

  return [...allowedPaths].filter((candidatePath) => (
    candidatePath !== managedPaths.projectPath
    && candidatePath !== managedPaths.basePath
    && candidatePath.startsWith(`${managedPaths.basePath}/`)
    && (
      candidatePath === managedPaths.managedPath
      || normalizedCandidates.includes(candidatePath)
    )
  ));
}

function buildManagedWorktreeDirectoryCleanupCommand(
  projectPath: string,
  branchName: string,
  candidatePath: string,
  fallbackWorktreePath?: string | null,
): string {
  const managedPaths = getNormalizedManagedWorktreePaths(projectPath, branchName);
  const normalizedFallbackPath = fallbackWorktreePath?.trim()
    ? normalizeGitWorktreePath(fallbackWorktreePath)
    : null;
  const target = quoteForPosixShell(candidatePath);
  const projectRoot = quoteForPosixShell(managedPaths.projectPath);
  const basePrefixPattern = `${quoteForPosixShell(`${managedPaths.basePath}/`)}*`;
  const allowedPathChecks = [managedPaths.managedPath, normalizedFallbackPath]
    .filter((value): value is string => Boolean(value))
    .filter((value, index, self) => self.indexOf(value) === index)
    .map((value) => `[ ${target} = ${quoteForPosixShell(value)} ]`)
    .join(" || ");

  return [
    `test -n ${target}`,
    `if [ ${target} = ${projectRoot} ]; then exit 0; fi`,
    `case ${target} in ${basePrefixPattern}) ;; *) exit 0;; esac`,
    `if ! { ${allowedPathChecks}; }; then exit 0; fi`,
    `test -d ${target} || exit 0`,
    `test ! -L ${target}`,
    `rm -rf -- ${target}`,
    `test ! -e ${target}`,
  ].join("; ");
}

async function removeStaleManagedWorktreeDirectories(
  projectPath: string,
  branchName: string,
  candidates: Array<string | null | undefined>,
  registeredWorktrees: WorktreeInfo[],
  sshHost?: string | null,
  options: ResourceCleanupOptions = {},
): Promise<void> {
  const registeredWorktreePaths = new Set(
    registeredWorktrees.map((worktree) => normalizeGitWorktreePath(worktree.path)),
  );
  const cleanupPaths = getManagedCleanupCandidatePaths(projectPath, branchName, candidates)
    .filter((cleanupPath) => !registeredWorktreePaths.has(cleanupPath));

  for (const cleanupPath of cleanupPaths) {
    try {
      await execGit(
        buildManagedWorktreeDirectoryCleanupCommand(
          projectPath,
          branchName,
          cleanupPath,
          options.worktreePath,
        ),
        sshHost,
      );
    } catch {
      if (options.throwOnError) {
        throw new Error(`managed worktree 디렉토리 정리 실패: ${cleanupPath}`);
      }
    }
  }
}

async function resolveBranchWorktreePath(
  projectPath: string,
  branchName: string,
  fallbackWorktreePath?: string | null,
  sshHost?: string | null,
): Promise<ResolvedBranchWorktree> {
  const worktrees = await listWorktrees(projectPath, sshHost);
  const managedPaths = getNormalizedManagedWorktreePaths(projectPath, branchName);
  const normalizedFallbackWorktreePath = fallbackWorktreePath
    ? normalizeGitWorktreePath(fallbackWorktreePath)
    : null;
  const matchingWorktrees = worktrees.filter((worktree) => (
    !worktree.isBare && worktree.branch === branchName
  ));

  const linkedWorktree = matchingWorktrees.find((worktree) => (
    normalizeGitWorktreePath(worktree.path) !== managedPaths.projectPath
  ));

  const fallbackWorktree = normalizedFallbackWorktreePath
    ? worktrees.find((worktree) => (
        !worktree.isBare
        && normalizeGitWorktreePath(worktree.path) === normalizedFallbackWorktreePath
        && normalizedFallbackWorktreePath === managedPaths.managedPath
        && normalizedFallbackWorktreePath !== managedPaths.projectPath
      ))
    : null;

  return {
    path: linkedWorktree?.path ?? fallbackWorktree?.path ?? null,
    isProjectRootCheckout: matchingWorktrees.some((worktree) => (
      normalizeGitWorktreePath(worktree.path) === managedPaths.projectPath
    )),
    registeredWorktrees: worktrees,
  };
}

/** worktree와 브랜치를 삭제한다. 세션은 건드리지 않는다 */
export async function removeWorktreeAndBranch(
  projectPath: string,
  branchName: string,
  sshHost?: string | null,
  options: ResourceCleanupOptions = {},
): Promise<void> {
  const resolvedWorktree = await resolveBranchWorktreePath(
    projectPath,
    branchName,
    options.worktreePath,
    sshHost,
  );
  const worktreePath = resolvedWorktree.path;
  const branchCommand = options.throwOnError
    ? `if git -C "${projectPath}" show-ref --verify --quiet "refs/heads/${branchName}"; then git -C "${projectPath}" branch -D "${branchName}"; fi`
    : `git -C "${projectPath}" branch -D "${branchName}"`;

  if (worktreePath) {
    const worktreeCommand = options.throwOnError
      ? `if git -C "${projectPath}" worktree list --porcelain | grep -Fxq "worktree ${worktreePath}"; then git -C "${projectPath}" worktree remove "${worktreePath}" --force; fi`
      : `git -C "${projectPath}" worktree remove "${worktreePath}" --force`;

    try {
      await execGit(worktreeCommand, sshHost);
    } catch {
      if (options.throwOnError) {
        throw new Error(`worktree 정리 실패: ${worktreePath}`);
      }
      // worktree가 이미 삭제된 경우 무시
    }
  }

  await removeStaleManagedWorktreeDirectories(
    projectPath,
    branchName,
    [worktreePath, options.worktreePath, buildManagedWorktreePath(projectPath, branchName)],
    resolvedWorktree.registeredWorktrees,
    sshHost,
    options,
  );

  if (resolvedWorktree.isProjectRootCheckout) {
    return;
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

function buildTmuxSessionCleanupCommand(
  sessionName: string,
  verifyCleanup: boolean,
): string {
  const target = quoteForPosixShell(sessionName);
  const tmuxCommands = buildTmuxLookupCommands();
  const killCommands = tmuxCommands.map((tmuxCommand) => (
    `${tmuxCommand} kill-session -t ${target} 2>/dev/null || true`
  ));

  if (!verifyCleanup) {
    return killCommands.join("; ");
  }

  return [
    "command -v tmux >/dev/null 2>&1 || exit 1",
    ...killCommands,
    ...tmuxCommands.map((tmuxCommand) => (
      `if ${tmuxCommand} has-session -t ${target} 2>/dev/null; then exit 1; fi`
    )),
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

/** zellij list-sessions는 종료된 세션을 `EXITED: <name>`으로 함께 출력하므로 이름만 뽑아 살아있는 세션과 구분한다 */
export function parseAliveZellijSessionNames(listSessionsOutput: string): string[] {
  return listSessionsOutput
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("EXITED:"))
    .map((line) => line.split(/\s+/)[0])
    .filter(Boolean);
}

/** 세션이 활성 상태인지 확인한다 */
export async function isSessionAlive(
  sessionType: SessionType,
  sessionName: string,
  sshHost?: string | null,
): Promise<boolean> {
  try {
    if (sessionType === SessionType.TMUX) {
      const target = quoteForPosixShell(sessionName);
      const hasSessionChecks = buildTmuxLookupCommands().map((tmuxCommand) => (
        `${tmuxCommand} has-session -t ${target} 2>/dev/null && exit 0`
      ));
      await execGit([...hasSessionChecks, "exit 1"].join("; "), sshHost);
      return true;
    } else {
      const output = await execGit("zellij list-sessions", sshHost);
      return parseAliveZellijSessionNames(output).includes(sessionName);
    }
  } catch {
    return false;
  }
}
