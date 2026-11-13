/** Ephemeral routing capability; never persist this with a desktop pairing. */
export interface RemoteTerminalGrant {
  version: 1;
  grantId: string;
  direct: { host: string; port: number; path: string; token: string; certificateFingerprint: string };
  expiresAt: number;
}

export interface RemoteTerminalGrantInput {
  sshHost: string;
  deviceId: string;
  taskId: string;
  sessionName: string;
  paneId: string;
  cols?: number;
  rows?: number;
}

export interface RemoteTerminalWireGrant {
  grantId: string;
  token: string;
  deviceId: string;
  taskId: string;
  paneId: string;
  targetKey: string;
  controlCommand: string;
  snapshotCommand: string;
  geometryCommand: string;
  expiresAt: number;
  cols: number;
  rows: number;
}

export const REMOTE_TERMINAL_MAX_DIMENSION = 500;
export const REMOTE_TERMINAL_INPUT_BYTES = 1024 * 1024;
// Bound a burst of paste chunks while the native controller prepares its first frame.
export const REMOTE_TERMINAL_INPUT_QUEUE_BYTES = 8 * REMOTE_TERMINAL_INPUT_BYTES;
export const REMOTE_TERMINAL_FRAME_BYTES = 8 * 1024 * 1024;
export const REMOTE_TERMINAL_ENVELOPE_BYTES = 16 * 1024 * 1024;
export const REMOTE_TERMINAL_ADMISSION_MS = 60_000;
// A direct socket can outlive a dead SSH connection; only A's control heartbeat renews ownership.
export const REMOTE_TERMINAL_OWNER_HEARTBEAT_MS = 10_000;
export const REMOTE_TERMINAL_OWNER_LEASE_MS = 30_000;
export const REMOTE_TERMINAL_COMMAND_TIMEOUT_MS = 5_000;

export function decodeRemoteTerminalBytes(value: unknown, limit: number): Buffer {
  if (typeof value !== "string" || value.length > Math.ceil(limit / 3) * 4 || value.length % 4 !== 0) {
    throw new Error("Invalid terminal payload");
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.length > limit || bytes.toString("base64") !== value) throw new Error("Terminal payload too large");
  return bytes;
}

export function isRemoteTerminalSize(cols: unknown, rows: unknown): boolean {
  return [cols, rows].every((value) => typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= REMOTE_TERMINAL_MAX_DIMENSION);
}

export type HerdrObserveRecord =
  | { type: "frame"; isFullScreen: boolean; output: string; width?: number; height?: number }
  | { type: "closed" };

export type HerdrTerminalRecord =
  | { type: "frame"; isFullScreen: boolean; bytes: Buffer; width?: number; height?: number }
  | { type: "closed" }
  | { type: "invalid" };

export function parseHerdrTerminalRecord(line: string): HerdrTerminalRecord | null {
  try {
    const record = JSON.parse(line);
    if (record.type === "terminal.closed") return { type: "closed" };
    if (record.type !== "terminal.frame") return null;
    return {
      type: "frame", isFullScreen: record.full === true,
      bytes: decodeRemoteTerminalBytes(record.bytes, REMOTE_TERMINAL_FRAME_BYTES),
      ...(typeof record.width === "number" && typeof record.height === "number" ? { width: record.width, height: record.height } : {}),
    };
  } catch { return { type: "invalid" }; }
}

/** Legacy consumers receive text, while persistent streams decode raw bytes across frame boundaries. */
export function parseHerdrObserveRecord(line: string): HerdrObserveRecord | null {
  const record = parseHerdrTerminalRecord(line);
  if (!record || record.type === "invalid") return null;
  if (record.type === "closed") return record;
  return { type: "frame", isFullScreen: record.isFullScreen, output: record.bytes.toString("utf8"),
    ...(record.width !== undefined ? { width: record.width, height: record.height } : {}) };
}

/** pane 보이는 영역 안의 칸 좌표(0부터)와 그 영역의 높이 */
export interface TerminalCursorPosition {
  x: number;
  y: number;
  paneHeight: number;
}

export const MAX_MIRROR_LINES = 2000;

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
