import { buildHerdrCommand, quoteForPosixShell } from "@/lib/worktree";

/**
 * 모바일 클라이언트에 pane 하나를 그대로 비추기 위한 명령 빌더와 출력 파서.
 *
 * `terminalTabs.ts`가 다루는 탭 축은 tmux window와 zellij tab이고, 이 파일은 그 아래 pane 축만 다룬다.
 * 두 축을 갈라 둔 이유는 조작 대상이 다르기 때문이다. 탭 축은 사용자가 만들고 지우고 이름을 바꾸는 대상이지만,
 * pane 축은 읽어서 비추기만 하는 대상이다.
 *
 * 읽기 명령은 포커스를 바꾸지 않는다. 모바일 viewport 명령만 연결 수명 동안 크기를 바꾸고 원래 배치를 복원한다.
 * tmux grouped session은 window 객체를 공유해서 `select-pane`이나 `resize-pane -Z`가 데스크탑 화면까지 움직이고,
 * zellij도 pane id를 주지 않으면 포커스된 pane을 대상으로 삼는다. 그래서 모든 명령이 대상을 명시적으로 받는다.
 *
 * 모든 함수는 부수효과가 없다. 실제 실행은 `mobileBridgeService`가 맡는다.
 */

/** 비출 pane 하나. tmux·zellij·Herdr pane을 같은 모양으로 맞춘다 */
export interface MirrorPane {
  /** 멀티플렉서가 주는 안정적인 pane 식별자. tmux는 `%17`, zellij는 `terminal_1`, Herdr는 `w1:p1` */
  id: string;
  /** 이 pane이 속한 탭(tmux window / zellij tab / Herdr tab)의 식별자 */
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

export interface TmuxViewportState {
  width: number;
  height: number;
  layout: string;
  windowSize: string | null;
  zoomed: boolean;
}

export function buildTmuxReadViewportCommand(windowId: string): string {
  const target = quoteForPosixShell(windowId);
  return `tmux display-message -p -t ${target} '#{window_width}\t#{window_height}\t#{window_layout}\t#{window_zoomed_flag}' && tmux show-options -wqv -t ${target} window-size`;
}

export function parseTmuxViewportState(output: string): TmuxViewportState {
  const [geometry, windowSize] = output.trimEnd().split("\n");
  const [width, height, layout, zoomed] = geometry.split("\t");
  if (!Number.isInteger(Number(width)) || Number(width) < 1 || !Number.isInteger(Number(height)) || Number(height) < 1 || !layout) {
    throw new Error("tmux 창 크기를 읽지 못했습니다.");
  }
  return { width: Number(width), height: Number(height), layout, windowSize: windowSize || null, zoomed: zoomed === "1" };
}

export function buildTmuxReadPaneGeometryCommand(windowId: string, paneId: string): string {
  const window = quoteForPosixShell(windowId);
  const unzoom = quoteForPosixShell(`resize-pane -Z -t ${window}`);
  return `tmux if-shell -F -t ${window} '#{window_zoomed_flag}' ${unzoom} && tmux display-message -p -t ${quoteForPosixShell(paneId)} '#{window_width} #{window_height} #{pane_width} #{pane_height}'`;
}

export function buildTmuxResizeViewportCommand(paneId: string, geometry: string, cols: number, rows: number): string {
  const [windowWidth, windowHeight, paneWidth, paneHeight] = geometry.trim().split(/\s+/).map(Number);
  if (![windowWidth, windowHeight, paneWidth, paneHeight].every((value) => Number.isInteger(value) && value > 0)) {
    throw new Error("tmux pane 크기를 읽지 못했습니다.");
  }
  const target = quoteForPosixShell(paneId);
  const width = windowWidth - paneWidth + cols;
  const height = windowHeight - paneHeight + rows;
  return `tmux resize-window -t ${target} -x ${width} -y ${height} && tmux resize-pane -t ${target} -x ${cols} -y ${rows}`;
}

export function buildTmuxRestoreViewportCommand(windowId: string, state: TmuxViewportState): string {
  const target = quoteForPosixShell(windowId);
  const policy = state.windowSize === null
    ? `tmux set-option -w -u -t ${target} window-size`
    : `tmux set-option -w -t ${target} window-size ${quoteForPosixShell(state.windowSize)}`;
  const zoom = state.zoomed
    ? `tmux if-shell -F -t ${target} '#{!:#{window_zoomed_flag}}' ${quoteForPosixShell(`resize-pane -Z -t ${target}`)} || exit $?; `
    : "";
  // PC에서 pane을 추가하면 저장한 layout을 적용할 수 없어도 임시 manual 정책은 반드시 풀어야 한다.
  return `kanvibe_restore_status=0; tmux resize-window -t ${target} -x ${state.width} -y ${state.height} && tmux select-layout -t ${target} ${quoteForPosixShell(state.layout)} || kanvibe_restore_status=$?; ${policy} || exit $?; ${zoom}exit "$kanvibe_restore_status"`;
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

/** pane 보이는 영역 안의 칸 좌표(0부터)와 그 영역의 높이 */
export interface TerminalCursorPosition {
  x: number;
  y: number;
  paneHeight: number;
}

/** 커서와 접속 전에 활성화된 화면·마우스 모드를 함께 읽는다. */
export function buildTmuxPaneStateCommand(paneId: string): string {
  return `tmux display-message -p -t ${quoteForPosixShell(paneId)} '#{cursor_x} #{cursor_y} #{pane_height} #{alternate_on} #{mouse_standard_flag} #{mouse_button_flag} #{mouse_all_flag} #{mouse_sgr_flag} #{mouse_utf8_flag}'`;
}

export function parseTmuxPaneState(output: string): { cursor: TerminalCursorPosition; restoreSequences: string } | null {
  const match = output.trim().match(/^(\d+) (\d+) (\d+) ([01]) ([01]) ([01]) ([01]) ([01]) ([01])$/);
  if (!match) return null;
  const [, x, y, paneHeight, alternate, standard, button, all, sgr, utf8] = match;
  const mouseMode = all === "1" ? 1003 : button === "1" ? 1002 : standard === "1" ? 1000 : null;
  const mouseEncoding = sgr === "1" ? 1006 : utf8 === "1" ? 1005 : null;
  let restoreSequences = `\x1b[?1049${alternate === "1" ? "h" : "l"}\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l`;
  if (mouseMode !== null) restoreSequences += `\x1b[?${mouseMode}h`;
  if (mouseEncoding !== null) restoreSequences += `\x1b[?${mouseEncoding}h`;
  return { cursor: { x: Number(x), y: Number(y), paneHeight: Number(paneHeight) }, restoreSequences };
}

const MAX_MIRROR_LINES = 2000;

/** 화면, 스크롤백을 지우고 커서를 맨 위로 보낸다. 덤프를 받을 때마다 이어 붙이지 않고 다시 그리기 위해서다 */
const CLEAR_SCREEN_AND_SCROLLBACK = "\x1b[H\x1b[2J\x1b[3J";

/**
 * 떠 온 화면 텍스트를 터미널이 처음부터 다시 그리는 출력으로 바꾼다.
 *
 * `capture-pane`과 `dump-screen`은 줄 끝에 LF만 붙인다. 터미널은 LF를 "아래로 한 칸"으로만 읽으므로 그대로 쓰면
 * 다음 줄이 앞 줄이 끝난 열에서 시작해 화면이 계단처럼 밀린다. 줄마다 CR을 붙여 줄 처음으로 돌려보낸다.
 */
export function buildScreenRedraw(screen: string, cursor: TerminalCursorPosition | null): string {
  const lines = screen.split(/\r?\n/).slice(-MAX_MIRROR_LINES).join("\r\n");
  return `${CLEAR_SCREEN_AND_SCROLLBACK}${lines}${cursor ? buildCursorMoveFromLastLine(cursor) : ""}`;
}

/**
 * 덤프 앞에는 스크롤백이 붙고 모바일 화면 높이도 pane과 다를 수 있어, 절대 좌표로는 커서가 놓일 줄을 알 수 없다.
 * 덤프의 마지막 줄은 언제나 pane 보이는 영역의 맨 아랫줄이므로, 다 쓴 뒤 그 줄에서 위로 올라가 커서 줄을 찾는다.
 */
function buildCursorMoveFromLastLine(cursor: TerminalCursorPosition): string {
  const rowsAboveLastLine = cursor.paneHeight - 1 - cursor.y;
  const moveUp = rowsAboveLastLine > 0 ? `\x1b[${rowsAboveLastLine}A` : "";
  return `${moveUp}\x1b[${cursor.x + 1}G`;
}

/**
 * pane의 현재 화면을 ANSI를 살린 채로 한 번 떠 온다.
 * 스트림은 구독 시점 이후만 흘러오므로 첫 화면은 이 스냅샷으로 채워야 빈 터미널이 보이지 않는다.
 */
export function buildTmuxCapturePaneCommand(paneId: string): string {
  return `tmux capture-pane -pe -S -${MAX_MIRROR_LINES} -t ${quoteForPosixShell(paneId)}`;
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
  const target = quoteForPosixShell(paneId);
  if (!input.includes("\0")) {
    return `tmux send-keys -t ${target} -l -- ${quoteForPosixShell(input)}`;
  }

  // 프로세스 인자는 NUL을 담을 수 없으므로 그 바이트만 tmux의 이름 있는 키로 전한다.
  const commands: string[] = [];
  for (const [index, text] of input.split("\0").entries()) {
    if (index > 0) commands.push(`tmux send-keys -t ${target} -- C-Space`);
    if (text) commands.push(`tmux send-keys -t ${target} -l -- ${quoteForPosixShell(text)}`);
  }
  return commands.join(" && ");
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

export interface ZellijViewportState {
  tabId: string;
  cols: number;
  rows: number;
  paneCols: number;
  paneRows: number;
  viewportCols: number;
  viewportRows: number;
  displayCols: number;
  displayRows: number;
  floating?: { left: number; top: number };
}

export function buildZellijReadViewportCommand(sessionName: string): string {
  return `printf '[' && ${buildZellijListMirrorPanesCommand(sessionName)} && printf ',' && zellij --session ${quoteForPosixShell(sessionName)} action list-tabs --json -a && printf ']'`;
}

export function buildZellijSelectTabCommand(sessionName: string, tabId: string): string {
  return `zellij --session ${quoteForPosixShell(sessionName)} action go-to-tab-by-id ${quoteForPosixShell(tabId)}`;
}

export function buildZellijResizeFloatingPaneCommand(
  sessionName: string,
  paneId: string,
  geometry: { left: number; top: number; cols: number; rows: number },
): string {
  return `zellij --session ${quoteForPosixShell(sessionName)} action change-floating-pane-coordinates --pane-id ${quoteForPosixShell(paneId)} --x ${geometry.left} --y ${geometry.top} --width ${geometry.cols} --height ${geometry.rows}`;
}

export function parseZellijViewportState(output: string, paneId: string): ZellijViewportState | null {
  try {
    const [panes, tabs] = JSON.parse(output);
    if (!Array.isArray(panes) || !Array.isArray(tabs)) return null;
    const pane = panes.find((entry) => entry?.is_plugin === false && formatZellijPaneId(entry.id) === paneId);
    const tab = tabs.find((entry) => entry?.tab_id === pane?.tab_id);
    if (!pane || !tab || !Number.isInteger(tab.tab_id) || tab.tab_id < 0) return null;
    const state = {
      cols: pane.pane_content_columns, rows: pane.pane_content_rows,
      paneCols: pane.pane_columns, paneRows: pane.pane_rows,
      viewportCols: tab.viewport_columns, viewportRows: tab.viewport_rows,
      displayCols: tab.display_area_columns, displayRows: tab.display_area_rows,
    };
    if (!Object.values(state).every((size) => Number.isInteger(size) && size > 0)) return null;
    if (pane.is_floating === true) {
      if (![pane.pane_x, pane.pane_y].every((position) => Number.isInteger(position) && position >= 0)) return null;
      return { ...state, tabId: String(tab.tab_id), floating: { left: pane.pane_x, top: pane.pane_y } };
    }
    return { ...state, tabId: String(tab.tab_id) };
  } catch {
    return null;
  }
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
  // 원격 전송량과 exec 버퍼를 제한하되 dump 실패를 tail의 성공으로 덮지 않는다.
  return `kanvibe_mobile_dump=$(zellij --session ${session} action dump-screen -p ${quoteForPosixShell(paneId)} -a --full) && printf '%s\\n' "$kanvibe_mobile_dump" | tail -n ${MAX_MIRROR_LINES}`;
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


export function buildTmuxPaneShortcutCommand(paneId: string, action: string): string | null {
  const target = quoteForPosixShell(paneId);
  switch (action) {
    case "split-right": return `tmux split-window -h -d -t ${target}`;
    case "split-down": return `tmux split-window -v -d -t ${target}`;
    case "toggle-fullscreen": return `tmux resize-pane -Z -t ${target}`;
    case "copy-mode": return `tmux copy-mode -t ${target}`;
    case "scroll-up": return `tmux copy-mode -t ${target} && tmux send-keys -X -t ${target} page-up`;
    case "scroll-down": return `tmux copy-mode -t ${target} && tmux send-keys -X -t ${target} page-down`;
    case "scroll-bottom": return `tmux copy-mode -t ${target} && tmux send-keys -X -t ${target} cancel`;
    default: return null;
  }
}

/**
 * pane 하나를 종료한다. 단축 동작 빌더에 넣지 않은 것은 그 경로가 마지막 pane 보호 없이 HTTP로도 열려 있기 때문이다.
 * 이 빌더들은 마지막 pane을 막는 `closeMobilePane`만 부른다.
 */
export function buildTmuxClosePaneCommand(paneId: string): string {
  return `tmux kill-pane -t ${quoteForPosixShell(paneId)}`;
}

export function buildZellijPaneShortcutCommand(sessionName: string, paneId: string, tabId: string, action: string): string | null {
  const prefix = `zellij --session ${quoteForPosixShell(sessionName)} action`;
  if (action === "split-right" || action === "split-down") {
    // --pane-id는 in-place 교체 전용이므로 분할은 검증한 탭에 만들고 다른 클라이언트의 포커스를 유지한다.
    const direction = action === "split-right" ? "right" : "down";
    return `${prefix} new-pane --direction ${direction} --tab-id ${quoteForPosixShell(tabId)} --no-focus`;
  }
  let command: string;
  switch (action) {
    case "toggle-fullscreen": command = "toggle-fullscreen"; break;
    case "toggle-floating": command = "toggle-pane-embed-or-floating"; break;
    case "scroll-up": command = "page-scroll-up"; break;
    case "scroll-down": command = "page-scroll-down"; break;
    case "scroll-bottom": command = "scroll-to-bottom"; break;
    default: return null;
  }
  return `${prefix} ${command} --pane-id ${quoteForPosixShell(paneId)}`;
}

export function buildZellijClosePaneCommand(sessionName: string, paneId: string): string {
  return `zellij --session ${quoteForPosixShell(sessionName)} action close-pane --pane-id ${quoteForPosixShell(paneId)}`;
}

/**
 * 지나간 출력까지 담은 Herdr pane 내용을 ANSI를 살린 채로 읽는다.
 * 커서 위치는 주지 않으므로 화면과 커서는 뒤따르는 observe의 전체 프레임이 맞춘다.
 */
export function buildHerdrRecentScreenCommand(sessionName: string, paneId: string): string {
  return buildHerdrCommand(sessionName, ["pane", "read", paneId, "--source", "recent", "--lines", String(MAX_MIRROR_LINES), "--format", "ansi"]);
}

/**
 * observe는 크기를 주지 않으면 pane과 무관하게 120x40으로 그린다.
 * 이 크기는 관찰자 전용 격자라 pane에서 도는 프로그램의 크기를 바꾸지 않는다.
 */
export function buildHerdrObservePaneCommand(sessionName: string, paneId: string, cols: number, rows: number): string {
  return buildHerdrCommand(sessionName, ["terminal", "session", "observe", paneId, "--cols", String(cols), "--rows", String(rows)]);
}

export function buildHerdrControlPaneCommand(sessionName: string, paneId: string, cols: number, rows: number): string {
  return buildHerdrCommand(sessionName, ["terminal", "session", "control", paneId, "--cols", String(cols), "--rows", String(rows)]);
}

export type HerdrObserveRecord =
  | { type: "frame"; isFullScreen: boolean; output: string; width?: number; height?: number }
  | { type: "closed" };

/** observe가 내보내는 NDJSON 한 줄을 읽는다. 모르는 레코드는 null이다 */
export function parseHerdrObserveRecord(line: string): HerdrObserveRecord | null {
  let record: { type?: unknown; bytes?: unknown; full?: unknown; width?: unknown; height?: unknown };
  try {
    record = JSON.parse(line);
  } catch {
    return null;
  }
  if (record.type === "terminal.closed") {
    return { type: "closed" };
  }
  if (record.type !== "terminal.frame" || typeof record.bytes !== "string") {
    return null;
  }
  return {
    type: "frame",
    isFullScreen: record.full === true,
    output: Buffer.from(record.bytes, "base64").toString("utf8"),
    ...(typeof record.width === "number" && typeof record.height === "number" ? { width: record.width, height: record.height } : {}),
  };
}

/**
 * 모바일에서 누른 키를 Herdr pane에 넣는다.
 * `send-text`는 제어 문자와 `--help`처럼 `-`로 시작하는 입력도 원문 그대로 전달하지만,
 * 프로세스 인자는 NUL을 담을 수 없어 그 바이트만 이름 있는 키로 전한다.
 */
export function buildHerdrWritePaneCommand(sessionName: string, paneId: string, input: string): string {
  const commands: string[] = [];
  for (const [index, text] of input.split("\0").entries()) {
    if (index > 0) commands.push(buildHerdrCommand(sessionName, ["pane", "send-keys", paneId, "ctrl+space"]));
    if (text) commands.push(buildHerdrCommand(sessionName, ["pane", "send-text", paneId, text]));
  }
  return commands.join(" && ");
}

/** Herdr에는 스크롤·복사 모드 명령이 없어 분할과 확대만 지원한다. 지난 출력은 모바일 화면을 손가락으로 밀어 본다 */
export function buildHerdrPaneShortcutCommand(sessionName: string, paneId: string, action: string): string | null {
  switch (action) {
    case "split-right": return buildHerdrCommand(sessionName, ["pane", "split", paneId, "--direction", "right", "--no-focus"]);
    case "split-down": return buildHerdrCommand(sessionName, ["pane", "split", paneId, "--direction", "down", "--no-focus"]);
    case "toggle-fullscreen": return buildHerdrCommand(sessionName, ["pane", "zoom", paneId, "--toggle"]);
    default: return null;
  }
}

export function buildHerdrClosePaneCommand(sessionName: string, paneId: string): string {
  return buildHerdrCommand(sessionName, ["pane", "close", paneId]);
}
