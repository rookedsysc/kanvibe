import { quoteForPosixShell } from "@/lib/worktree";

/**
 * 모바일 클라이언트에 pane 하나를 그대로 비추기 위한 명령 빌더와 출력 파서.
 *
 * `terminalTabs.ts`가 다루는 탭 축은 tmux window와 zellij tab이고, 이 파일은 그 아래 pane 축만 다룬다.
 * 두 축을 갈라 둔 이유는 조작 대상이 다르기 때문이다. 탭 축은 사용자가 만들고 지우고 이름을 바꾸는 대상이지만,
 * pane 축은 읽어서 비추기만 하는 대상이다.
 *
 * 여기 있는 명령은 어느 것도 포커스나 크기를 바꾸지 않는다. 데스크탑 화면을 건드리지 않고 읽는 것이 이 파일의 전제다.
 * tmux grouped session은 window 객체를 공유해서 `select-pane`이나 `resize-pane -Z`가 데스크탑 화면까지 움직이고,
 * zellij도 pane id를 주지 않으면 포커스된 pane을 대상으로 삼는다. 그래서 모든 명령이 대상을 명시적으로 받는다.
 *
 * 모든 함수는 부수효과가 없다. 실제 실행은 `mobileBridgeService`가 맡는다.
 */

/** 비출 pane 하나. tmux pane과 zellij pane을 같은 모양으로 맞춘다 */
export interface MirrorPane {
  /** 멀티플렉서가 주는 안정적인 pane 식별자. tmux는 `%17`, zellij는 `terminal_1` */
  id: string;
  /** 이 pane이 속한 탭(tmux window / zellij tab)의 식별자 */
  tabId: string;
  tabName: string;
  /** pane에서 돌고 있는 명령. 알 수 없으면 빈 문자열 */
  command: string;
  /** 탭 안에서의 좌표와 크기. 태블릿이 window 하나를 통째로 조립할 때 쓴다 */
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * 이름을 마지막 필드에 두고 탭 문자로 구분한다.
 * tmux는 format 출력에서 이름의 제어문자를 이스케이프하지만, 파서는 남는 조각을 되붙여 그 가정이 깨져도 이름이 잘리지 않게 한다.
 */
const TMUX_MIRROR_PANE_FORMAT = [
  "#{pane_id}",
  "#{window_id}",
  "#{pane_current_command}",
  "#{pane_left}",
  "#{pane_top}",
  "#{pane_width}",
  "#{pane_height}",
  "#{window_name}",
].join("\t");

const TMUX_MIRROR_PANE_FIELD_COUNT = 8;

/**
 * 세션 하나의 모든 pane을 window 순서대로 조회하는 명령.
 * `buildTmuxListPanesCommand`는 에이전트 탐지용이라 서버 전체를 훑고 좌표를 담지 않으므로 여기서는 쓰지 않는다.
 */
export function buildTmuxListMirrorPanesCommand(sessionName: string): string {
  const target = quoteForPosixShell(sessionName);
  return `tmux list-panes -s -t ${target} -F ${quoteForPosixShell(TMUX_MIRROR_PANE_FORMAT)}`;
}

/** `buildTmuxListMirrorPanesCommand` 출력을 pane 목록으로 바꾼다 */
export function parseTmuxMirrorPaneList(listPanesOutput: string): MirrorPane[] {
  return listPanesOutput
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line.length > 0)
    .map(parseTmuxMirrorPaneLine)
    .filter((pane): pane is MirrorPane => pane !== null);
}

function parseTmuxMirrorPaneLine(line: string): MirrorPane | null {
  const fields = line.split("\t");
  if (fields.length < TMUX_MIRROR_PANE_FIELD_COUNT) {
    return null;
  }

  const [paneId, windowId, command, left, top, width, height, ...nameFields] = fields;
  const geometry = parseGeometry({ left, top, width, height });
  if (!paneId || !windowId || geometry === null) {
    return null;
  }

  return {
    id: paneId,
    tabId: windowId,
    tabName: nameFields.join("\t"),
    command,
    ...geometry,
  };
}

function parseGeometry(raw: {
  left: string;
  top: string;
  width: string;
  height: string;
}): Pick<MirrorPane, "left" | "top" | "width" | "height"> | null {
  const left = Number.parseInt(raw.left, 10);
  const top = Number.parseInt(raw.top, 10);
  const width = Number.parseInt(raw.width, 10);
  const height = Number.parseInt(raw.height, 10);

  const isParsed = [left, top, width, height].every(Number.isInteger);
  if (!isParsed || width <= 0 || height <= 0) {
    return null;
  }

  return { left, top, width, height };
}

/**
 * pane의 현재 화면을 ANSI를 살린 채로 한 번 떠 온다.
 * 스트림은 구독 시점 이후만 흘러오므로 첫 화면은 이 스냅샷으로 채워야 빈 터미널이 보이지 않는다.
 */
export function buildTmuxCapturePaneCommand(paneId: string): string {
  return `tmux capture-pane -pe -t ${quoteForPosixShell(paneId)}`;
}

/** 흘려보내기를 멈춘다. 명령 인자를 주지 않으면 tmux가 해당 pane의 파이프를 끊는다 */
export function buildTmuxCancelPipePaneCommand(paneId: string): string {
  return `tmux pipe-pane -t ${quoteForPosixShell(paneId)}`;
}

/**
 * pane의 출력을 계속 흘려보내는 명령. 표준출력으로 원문 바이트가 그대로 나온다.
 *
 * tmux의 `pipe-pane`은 자기 자식으로 명령을 띄우고 우리 프로세스의 fd를 물려주지 않으므로,
 * 중간에 임시 파일을 두고 그 파일을 `tail -f`로 따라가는 방식 말고는 출력을 받아올 방법이 없다.
 * 파일을 거치는 대신 얻는 것은, 이 한 줄이 로컬에서도 `ssh` 너머에서도 똑같이 동작한다는 점이다.
 *
 * `-o`는 같은 pane에 파이프가 이미 걸려 있으면 끄는 토글이라, 호출자는 pane마다 파이프를 하나만 유지해야 한다.
 * 구독자가 여럿이면 파이프도 하나만 걸고 받은 쪽에서 나눠 보낸다.
 *
 * `trap`이 있어야 이 프로세스가 죽을 때 pane에 걸어 둔 파이프와 임시 파일이 남지 않는다.
 * 그런데 `tail`을 마지막 명령으로 두면 셸이 그 자리에서 자신을 `exec`로 치환해 트랩째 사라진다.
 * 그래서 `tail`을 배경으로 돌리고 `wait`로 붙잡는다. 이 형태여야 종료 신호에 트랩이 실제로 실행된다.
 */
export function buildTmuxPaneStreamCommand(paneId: string): string {
  const target = quoteForPosixShell(paneId);
  return [
    "sink=$(mktemp -t kanvibe-pane.XXXXXX)",
    `trap 'tmux pipe-pane -t ${target}; rm -f "$sink"; kill $tailpid 2>/dev/null' EXIT INT TERM`,
    `tmux pipe-pane -o -t ${target} "cat >> '$sink'"`,
    'tail -n +1 -f "$sink" & tailpid=$!',
    "wait $tailpid",
  ].join("; ");
}

/**
 * 모바일에서 누른 키를 pane에 그대로 넣는다.
 * `-l`은 문자열을 키 이름으로 해석하지 않고 글자 그대로 보내라는 뜻이고, `--`가 있어야 대시로 시작하는 입력이 옵션으로 먹히지 않는다.
 */
export function buildTmuxSendKeysCommand(paneId: string, input: string): string {
  return `tmux send-keys -t ${quoteForPosixShell(paneId)} -l -- ${quoteForPosixShell(input)}`;
}

/**
 * zellij pane id는 JSON의 숫자 id에 종류 접두사를 붙인 형태다.
 * plugin pane은 zellij 자신의 UI라 비추지 않으므로 terminal만 다룬다.
 */
function formatZellijPaneId(numericId: number): string {
  return `terminal_${numericId}`;
}

/**
 * zellij 세션의 pane을 JSON으로 조회하는 명령.
 * 표 형식은 필드 구분이 공백 두 칸인데 탭 이름과 명령에 공백이 들어갈 수 있어 일반적으로 파싱할 수 없다. JSON이 아니면 안 된다.
 */
export function buildZellijListMirrorPanesCommand(sessionName: string): string {
  return `zellij --session ${quoteForPosixShell(sessionName)} action list-panes --json -a`;
}

interface ZellijPaneJsonEntry {
  id?: unknown;
  is_plugin?: unknown;
  tab_id?: unknown;
  tab_name?: unknown;
  title?: unknown;
  terminal_command?: unknown;
  pane_x?: unknown;
  pane_y?: unknown;
  pane_rows?: unknown;
  pane_columns?: unknown;
}

/** `buildZellijListMirrorPanesCommand` 출력을 pane 목록으로 바꾼다 */
export function parseZellijMirrorPaneList(listPanesJsonOutput: string): MirrorPane[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(listPanesJsonOutput);
  } catch {
    return [];
  }

  if (!Array.isArray(parsed)) {
    return [];
  }

  return parsed
    .map((entry) => parseZellijMirrorPaneEntry(entry as ZellijPaneJsonEntry))
    .filter((pane): pane is MirrorPane => pane !== null);
}

function parseZellijMirrorPaneEntry(entry: ZellijPaneJsonEntry): MirrorPane | null {
  if (entry?.is_plugin !== false || !Number.isInteger(entry?.id)) {
    return null;
  }

  const geometry = parseGeometry({
    left: String(entry.pane_x),
    top: String(entry.pane_y),
    width: String(entry.pane_columns),
    height: String(entry.pane_rows),
  });
  if (geometry === null) {
    return null;
  }

  /** 명령이 비어 있는 pane은 기본 셸이라 zellij가 붙인 제목을 대신 보여 준다 */
  const command = typeof entry.terminal_command === "string" && entry.terminal_command.length > 0
    ? entry.terminal_command
    : typeof entry.title === "string"
      ? entry.title
      : "";

  return {
    id: formatZellijPaneId(entry.id as number),
    tabId: String(entry.tab_id ?? ""),
    tabName: typeof entry.tab_name === "string" ? entry.tab_name : "",
    command,
    ...geometry,
  };
}

/**
 * zellij pane 하나의 현재 화면을 ANSI를 살린 채로 떠 온다.
 * zellij에는 tmux `pipe-pane`에 해당하는 출력 스트림이 없어서 이 명령을 주기적으로 다시 부르는 방식으로 따라간다.
 */
export function buildZellijDumpPaneCommand(sessionName: string, paneId: string): string {
  const session = quoteForPosixShell(sessionName);
  return `zellij --session ${session} action dump-screen -p ${quoteForPosixShell(paneId)} -a`;
}

/**
 * 모바일에서 누른 키를 zellij pane에 넣는다.
 * `action write`는 바이트를 십진수로 하나씩 받으므로 문자열을 UTF-8 바이트로 펼쳐 넘긴다.
 * `send-keys`가 아니라 `write`를 쓰는 이유는 키 이름 해석 없이 원문 그대로를 전달해야 하기 때문이다.
 */
export function buildZellijWritePaneCommand(sessionName: string, paneId: string, input: string): string {
  const session = quoteForPosixShell(sessionName);
  const bytes = Array.from(Buffer.from(input, "utf8")).join(" ");
  return `zellij --session ${session} action write -p ${quoteForPosixShell(paneId)} ${bytes}`;
}
