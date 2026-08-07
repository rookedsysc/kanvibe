import type { TerminalTab } from "@/desktop/shared/terminalTabs";
import { quoteForPosixShell } from "@/lib/worktree";

/**
 * tmux window와 zellij tab을 KanVibe 탭 바가 쓰는 모양으로 바꾸는 명령 빌더와 출력 파서.
 *
 * 세션 생성·정리는 `worktree.ts`가 담당하고 이 파일은 탭 축만 다룬다.
 * 두 축은 변경 출처가 달라서(세션 수명 vs 탭 조작) 같은 파일에 두면 함께 커진다.
 *
 * 모든 함수는 부수효과가 없다. 실제 실행은 `terminalTabService`가 맡는다.
 */

/**
 * 탭 이름을 마지막 필드에 두고 탭 문자로 구분한다.
 * tmux는 format 출력에서 이름의 제어문자를 이스케이프하므로 이름 안에 실제 탭 문자는 들어오지 않지만,
 * 파서는 남는 조각을 이름으로 되붙여 그 가정이 깨져도 이름이 잘리지 않게 한다.
 */
const TMUX_WINDOW_LIST_FORMAT = "#{window_id}\t#{window_index}\t#{window_active}\t#{window_name}";

const TMUX_WINDOW_LIST_FIELD_COUNT = 4;

/** tmux window 목록을 조회하는 명령 */
export function buildTmuxListWindowsCommand(sessionName: string): string {
  return [
    "tmux list-windows -t",
    quoteForPosixShell(sessionName),
    "-F",
    quoteForPosixShell(TMUX_WINDOW_LIST_FORMAT),
  ].join(" ");
}

/** `buildTmuxListWindowsCommand` 출력을 탭 목록으로 바꾼다 */
export function parseTmuxWindowList(listWindowsOutput: string): TerminalTab[] {
  return listWindowsOutput
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line.length > 0)
    .map(parseTmuxWindowLine)
    .filter((tab): tab is TerminalTab => tab !== null);
}

function parseTmuxWindowLine(line: string): TerminalTab | null {
  const fields = line.split("\t");
  if (fields.length < TMUX_WINDOW_LIST_FIELD_COUNT) {
    return null;
  }

  const [windowId, windowIndex, windowActive, ...nameFields] = fields;
  const parsedIndex = Number.parseInt(windowIndex, 10);
  if (!windowId || !Number.isInteger(parsedIndex)) {
    return null;
  }

  return {
    id: windowId,
    nativeIndex: parsedIndex,
    name: nameFields.join("\t"),
    isActive: windowActive === "1",
  };
}

/**
 * tmux window id는 서버 전역에서 유일하므로 세션 접두사 없이 그대로 대상으로 쓴다.
 * 인덱스를 쓰면 다른 window가 닫힐 때 번호가 밀려 엉뚱한 탭을 조작하게 된다.
 */
export function buildTmuxSelectWindowCommand(windowId: string): string {
  return `tmux select-window -t ${quoteForPosixShell(windowId)}`;
}

/** 세션 뒤의 콜론은 "이 세션의 새 window"를 뜻한다 */
export function buildTmuxNewWindowCommand(sessionName: string, workingDir?: string | null): string {
  const target = `${quoteForPosixShell(sessionName)}:`;
  const createArgument = `tmux new-window -t ${target} -P -F ${quoteForPosixShell("#{window_id}")}`;
  return workingDir
    ? `${createArgument} -c ${quoteForPosixShell(workingDir)}`
    : createArgument;
}

export function buildTmuxKillWindowCommand(windowId: string): string {
  return `tmux kill-window -t ${quoteForPosixShell(windowId)}`;
}

export function buildTmuxRenameWindowCommand(windowId: string, name: string): string {
  return `tmux rename-window -t ${quoteForPosixShell(windowId)} -- ${quoteForPosixShell(name)}`;
}

/**
 * window를 목표 인덱스로 옮긴다.
 *
 * `-k`는 쓰지 않는다. tmux에서 `-k`는 자리를 비켜 주는 옵션이 아니라 목표 인덱스에 있던 window를
 * 죽이는 옵션이라, 탭을 끌어다 놓을 때마다 놓인 자리의 탭과 그 안에서 돌던 프로세스가 사라진다.
 * 대신 목표 window의 앞(`-b`)이나 뒤(`-a`)에 끼워 넣어 나머지를 밀어낸다.
 *
 * 인덱스는 배열 위치가 아니라 `TerminalTab.nativeIndex`, 즉 tmux가 매긴 window_index를 받는다.
 * 끼워 넣은 뒤 남는 번호 구멍은 `-r`로 다시 채워 탭 순서와 인덱스를 일치시킨다.
 */
export function buildTmuxMoveWindowCommands(
  sessionName: string,
  windowId: string,
  currentWindowIndex: number,
  targetWindowIndex: number,
): string[] {
  if (currentWindowIndex === targetWindowIndex) {
    return [];
  }

  const quotedSessionName = quoteForPosixShell(sessionName);
  const insertSide = targetWindowIndex > currentWindowIndex ? "-a" : "-b";

  return [
    `tmux move-window ${insertSide} -s ${quoteForPosixShell(windowId)} -t ${quotedSessionName}:${targetWindowIndex}`,
    `tmux move-window -r -t ${quotedSessionName}`,
  ];
}

/**
 * zellij는 CLI 이벤트 스트림이 없어 조회도 명령 실행으로만 가능하다.
 * 세션 지정은 `zellij --session <name> action ...` 형태다.
 */
function buildZellijActionCommand(sessionName: string, actionArguments: string): string {
  return `zellij --session ${quoteForPosixShell(sessionName)} action ${actionArguments}`;
}

/**
 * 탭 id·이름·활성 여부를 한 번에 주는 `list-tabs --json`은 zellij 0.44.0에서 추가됐다.
 * 그 이전 버전은 이름 목록과 레이아웃 덤프를 합쳐 같은 정보를 만든다.
 */
const ZELLIJ_TAB_ID_MINIMUM_VERSION = [0, 44, 0] as const;

export function buildZellijVersionCommand(): string {
  return "zellij --version";
}

/** `zellij --version` 출력이 탭 id CLI를 지원하는 버전인지 판정한다. 판정 불가면 구버전으로 본다 */
export function supportsZellijTabIdCommands(versionOutput: string): boolean {
  const versionMatch = versionOutput.match(/(\d+)\.(\d+)\.(\d+)/);
  if (!versionMatch) {
    return false;
  }

  const parsedVersion = [
    Number.parseInt(versionMatch[1], 10),
    Number.parseInt(versionMatch[2], 10),
    Number.parseInt(versionMatch[3], 10),
  ];

  for (let position = 0; position < parsedVersion.length; position += 1) {
    if (parsedVersion[position] !== ZELLIJ_TAB_ID_MINIMUM_VERSION[position]) {
      return parsedVersion[position] > ZELLIJ_TAB_ID_MINIMUM_VERSION[position];
    }
  }

  return true;
}

export function buildZellijListTabsCommand(sessionName: string): string {
  return buildZellijActionCommand(sessionName, "list-tabs --json");
}

interface ZellijTabJsonEntry {
  position?: unknown;
  tab_id?: unknown;
  name?: unknown;
  active?: unknown;
}

/**
 * `list-tabs --json` 출력을 탭 목록으로 바꾼다.
 *
 * `position`은 화면 순서라 탭을 닫으면 다시 매겨지지만 `tab_id`는 고정이다.
 * `*-by-id` 명령이 받는 값도 `tab_id`이므로 식별자는 반드시 이쪽을 쓴다.
 * position을 id로 쓰면 가운데 탭을 닫은 뒤 엉뚱한 탭을 조작하게 된다.
 */
export function parseZellijTabList(listTabsJsonOutput: string): TerminalTab[] {
  let parsedEntries: unknown;
  try {
    parsedEntries = JSON.parse(listTabsJsonOutput);
  } catch {
    return [];
  }

  if (!Array.isArray(parsedEntries)) {
    return [];
  }

  return parsedEntries
    .map((entry: ZellijTabJsonEntry) => {
      const position = entry?.position;
      const tabId = entry?.tab_id;
      const name = entry?.name;
      if (
        typeof position !== "number" || !Number.isInteger(position)
        || typeof tabId !== "number" || !Number.isInteger(tabId)
        || typeof name !== "string"
      ) {
        return null;
      }

      return {
        id: String(tabId),
        nativeIndex: position,
        name,
        isActive: entry?.active === true,
      };
    })
    .filter((tab): tab is TerminalTab => tab !== null);
}

export function buildZellijQueryTabNamesCommand(sessionName: string): string {
  return buildZellijActionCommand(sessionName, "query-tab-names");
}

export function buildZellijDumpLayoutCommand(sessionName: string): string {
  return buildZellijActionCommand(sessionName, "dump-layout");
}

/** KDL 레이아웃에서 `focus=true`가 붙은 tab 노드의 이름을 찾는다. 없으면 null */
export function parseZellijFocusedTabName(dumpLayoutOutput: string): string | null {
  for (const line of dumpLayoutOutput.split("\n")) {
    const tabNodeMatch = line.match(/^\s*tab\b(.*)$/);
    if (!tabNodeMatch) {
      continue;
    }

    const tabAttributes = tabNodeMatch[1];
    if (!/\bfocus=true\b/.test(tabAttributes)) {
      continue;
    }

    const nameMatch = tabAttributes.match(/\bname="((?:[^"\\]|\\.)*)"/);
    if (nameMatch) {
      return nameMatch[1].replace(/\\(.)/g, "$1");
    }
  }

  return null;
}

/**
 * 구버전 zellij용 폴백. 이름 목록에 레이아웃 덤프의 활성 탭을 얹는다.
 * 레이아웃 덤프가 비어도 목록 자체는 쓸 수 있어야 하므로 그때는 첫 탭을 활성으로 본다.
 *
 * 이름이 같은 탭은 구분하지 못한다. 이 경로에는 이름 말고 식별자가 없어서 활성 판정도,
 * 이름으로 대상을 겨냥하는 `go-to-tab-name`·`close-tab`·`rename-tab`도 모두 앞의 탭을 집는다.
 * zellij 0.44.0 이상은 `tab_id`를 주는 `parseZellijTabList` 경로를 타므로 이 한계가 없다.
 */
export function parseZellijTabNamesWithFocus(
  queryTabNamesOutput: string,
  dumpLayoutOutput: string,
): TerminalTab[] {
  const tabNames = queryTabNamesOutput
    .split("\n")
    .map((line) => line.replace(/\r$/, "").trim())
    .filter((name) => name.length > 0);

  if (tabNames.length === 0) {
    return [];
  }

  const focusedTabName = parseZellijFocusedTabName(dumpLayoutOutput);
  const activeIndex = focusedTabName === null ? 0 : tabNames.indexOf(focusedTabName);
  const resolvedActiveIndex = activeIndex === -1 ? 0 : activeIndex;

  return tabNames.map((name, index) => ({
    id: String(index),
    nativeIndex: index,
    name,
    isActive: index === resolvedActiveIndex,
  }));
}

export function buildZellijGoToTabByIdCommand(sessionName: string, tabId: string): string {
  return buildZellijActionCommand(sessionName, `go-to-tab-by-id ${quoteForPosixShell(tabId)}`);
}

/**
 * 구버전 폴백은 이름으로 이동한다.
 * `go-to-tab`의 인덱스가 1부터인지 0부터인지는 버전에 따라 달라서, 이름이 더 안전하다.
 */
export function buildZellijGoToTabByNameCommand(sessionName: string, tabName: string): string {
  return buildZellijActionCommand(sessionName, `go-to-tab-name ${quoteForPosixShell(tabName)}`);
}

export function buildZellijNewTabCommand(sessionName: string, workingDir?: string | null): string {
  const newTabArguments = workingDir
    ? `new-tab --cwd ${quoteForPosixShell(workingDir)}`
    : "new-tab";
  return buildZellijActionCommand(sessionName, newTabArguments);
}

export function buildZellijCloseTabByIdCommand(sessionName: string, tabId: string): string {
  return buildZellijActionCommand(sessionName, `close-tab-by-id ${quoteForPosixShell(tabId)}`);
}

/** 구버전 폴백. `close-tab`은 활성 탭만 닫으므로 먼저 대상 탭으로 이동한다 */
export function buildZellijCloseFocusedTabCommands(sessionName: string, tabName: string): string[] {
  return [
    buildZellijGoToTabByNameCommand(sessionName, tabName),
    buildZellijActionCommand(sessionName, "close-tab"),
  ];
}

export function buildZellijRenameTabByIdCommand(
  sessionName: string,
  tabId: string,
  name: string,
): string {
  return buildZellijActionCommand(
    sessionName,
    `rename-tab-by-id ${quoteForPosixShell(tabId)} ${quoteForPosixShell(name)}`,
  );
}

/** 구버전 폴백. `rename-tab`도 활성 탭만 바꾸므로 먼저 대상 탭으로 이동한다 */
export function buildZellijRenameFocusedTabCommands(
  sessionName: string,
  tabName: string,
  name: string,
): string[] {
  return [
    buildZellijGoToTabByNameCommand(sessionName, tabName),
    buildZellijActionCommand(sessionName, `rename-tab ${quoteForPosixShell(name)}`),
  ];
}

function buildZellijMoveTabSteps(currentIndex: number, targetIndex: number) {
  return {
    stepCount: Math.abs(targetIndex - currentIndex),
    direction: targetIndex > currentIndex ? "right" : "left",
  };
}

/**
 * zellij의 `move-tab`은 방향만 받으므로 목표 위치까지 한 칸씩 옮긴다.
 * `-t`로 대상 탭을 직접 겨냥해, 순서만 바꾸려는데 활성 탭까지 바뀌는 일이 없게 한다.
 */
export function buildZellijMoveTabByIdCommands(
  sessionName: string,
  tabId: string,
  currentIndex: number,
  targetIndex: number,
): string[] {
  const { stepCount, direction } = buildZellijMoveTabSteps(currentIndex, targetIndex);
  const moveCommand = buildZellijActionCommand(
    sessionName,
    `move-tab ${direction} -t ${quoteForPosixShell(tabId)}`,
  );

  return Array.from({ length: stepCount }, () => moveCommand);
}

/** 구버전 폴백. `-t`가 없어 대상 탭으로 이동한 뒤 방향 명령을 반복한다 */
export function buildZellijMoveTabCommands(
  sessionName: string,
  tabName: string,
  currentIndex: number,
  targetIndex: number,
): string[] {
  const { stepCount, direction } = buildZellijMoveTabSteps(currentIndex, targetIndex);
  if (stepCount === 0) {
    return [];
  }

  const moveCommand = buildZellijActionCommand(sessionName, `move-tab ${direction}`);

  return [
    buildZellijGoToTabByNameCommand(sessionName, tabName),
    ...Array.from({ length: stepCount }, () => moveCommand),
  ];
}
