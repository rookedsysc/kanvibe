export type PaneChatProvider = "claude" | "codex" | "opencode" | "gemini";

export interface PaneChatProcess {
  pid: number;
  processStartTime: string;
  provider: PaneChatProvider;
  /** 명시된 Unix app-server 주소. 다른 종류의 --remote 주소이면 null이며 기본 소켓으로 대체하지 않는다 */
  codexServerSocket?: string | null;
}

interface ProcessRow {
  pid: number;
  parentPid: number;
  group: number;
  foregroundGroup: number;
  tty: string;
  processStartTime: string;
  command: string;
  args: string;
}

const PROVIDERS = new Set<PaneChatProvider>(["claude", "codex", "opencode", "gemini"]);
const INTERPRETERS = /^(?:node|nodejs|bun|python(?:\d+(?:\.\d+)?)?|ruby|bash|sh|zsh)$/;

function interpreterScript(args: string): string {
  const tokens = args.match(/"(?:[^"\\]|\\.)*"|'[^']*'|\S+/g)?.map((token) => token.replace(/^["']|["']$/g, "")) ?? [];
  for (let index = 1; index < tokens.length; index++) {
    const token = tokens[index];
    if (token === "--") return tokens[index + 1] ?? "";
    if (/^(?:-[epc].*|--(?:eval|print|command)(?:=.*)?)$/.test(token)) return "";
    if (/^(?:-r|--require|--import|--loader|--experimental-loader|--max-old-space-size|--max_old_space_size)$/.test(token)) { index++; continue; }
    if (token.startsWith("-")) continue;
    return token;
  }
  return "";
}

function detectProvider(row: Pick<ProcessRow, "command" | "args">): PaneChatProvider | null {
  const executable = row.command.split("/").at(-1)?.replace(/\.exe$/i, "").toLowerCase() ?? "";
  if (PROVIDERS.has(executable as PaneChatProvider)) return executable as PaneChatProvider;
  const nativeExecutable = row.args.match(/^\S+/)?.[0] ?? "";
  // 공식 native 설치는 ucomm에 버전 파일명이 드러나므로 argv의 실행 경로까지 확인한다.
  if (/^\d+\.\d+\.\d+$/.test(executable) && (
    nativeExecutable === "claude" || /^\/(?:[^/\s]+\/)*claude$/.test(nativeExecutable)
    || nativeExecutable.endsWith(`/.local/share/claude/versions/${executable}`)
  )) return "claude";
  if (!INTERPRETERS.test(executable)) return null;
  const script = interpreterScript(row.args);
  const basename = script.split("/").at(-1) ?? "";
  if (PROVIDERS.has(basename as PaneChatProvider)) return basename as PaneChatProvider;
  if (/\/@anthropic-ai\/claude-code\/.*(?:cli|index)\.js$/.test(script)) return "claude";
  if (/\/@google\/gemini-cli\/(?:.*\/)?(?:cli|index|gemini)\.js$/.test(script)) return "gemini";
  if (/\/@openai\/codex\/.*codex\.js$/.test(script)) return "codex";
  return null;
}

export function isPaneChatProviderProcess(command: string, args: string, provider: PaneChatProvider): boolean {
  return detectProvider({ command, args }) === provider;
}

export function findPaneChatProcess(output: string, rootPid: number): PaneChatProcess | null {
  const rows: ProcessRow[] = output.split("\n").flatMap((line) => {
    const fields = line.trim().split(/\s+/);
    if (fields.length < 11 || !fields.slice(0, 4).every((field) => /^-?\d+$/.test(field))) return [];
    return [{ pid: Number(fields[0]), parentPid: Number(fields[1]), group: Number(fields[2]), foregroundGroup: Number(fields[3]), tty: fields[4], processStartTime: fields.slice(5, 10).join(" "), command: fields[10], args: fields.slice(11).join(" ") }];
  });
  const byPid = new Map(rows.map((row) => [row.pid, row]));
  const root = byPid.get(rootPid);
  if (!root || root.tty === "??" || root.tty === "?" || root.foregroundGroup < 1) return null;
  const isDescendant = (pid: number, ancestor: number) => {
    const visited = new Set<number>();
    let current = pid;
    while (current > 0 && !visited.has(current)) {
      if (current === ancestor) return true;
      visited.add(current);
      current = byPid.get(current)?.parentPid ?? 0;
    }
    return false;
  };
  const candidates = rows.filter((row) => row.tty === root.tty && row.group === root.foregroundGroup && isDescendant(row.pid, rootPid) && detectProvider(row));
  if (new Set(candidates.map(detectProvider)).size !== 1) return null;
  let row: ProcessRow;
  if (candidates.length === 1) row = candidates[0];
  else {
    const geminiWrapper = candidates.find((candidate) => /^(?:node|nodejs)$/.test(candidate.command.split("/").at(-1) ?? "")
      && candidates.some((child) => child.parentPid === candidate.pid && detectProvider(child) === "gemini"
        && /(?:^|\s)--max-old-space-size(?:=|\s)/.test(child.args) && interpreterScript(child.args) === interpreterScript(candidate.args)));
    if (candidates.length === 2 && geminiWrapper && detectProvider(geminiWrapper) === "gemini") {
      const child = candidates.find((candidate) => candidate.parentPid === geminiWrapper.pid)!;
      return { pid: child.pid, processStartTime: child.processStartTime, provider: "gemini" };
    }
    const wrapper = candidates.find((candidate) => /^(?:node|nodejs)$/i.test(candidate.command.split("/").at(-1) ?? ""));
    const native = candidates.find((candidate) => candidate.parentPid === wrapper?.pid && candidate.command.split("/").at(-1) === "codex");
    const knownWrapper = wrapper && /\/@openai\/codex\/(?:bin\/)?codex\.js(?:\s|$)/.test(wrapper.args);
    const knownNative = native && /(?:^|\s)\S*\/@openai\/codex(?:-[^/]+)?\/\S*\/codex(?:\.exe)?(?:\s|$)/.test(native.args);
    // npm wrapper만 stdin을 native 자식에게 잇는다. 에이전트가 실행한 동종 CLI는 입력 소유자가 아니다.
    if (candidates.length !== 2 || !wrapper || !native || (!knownWrapper && !knownNative)) return null;
    row = native;
  }
  const provider = detectProvider(row)!;
  const remote = provider === "codex" ? row.args.match(/(?:^|\s)--remote(?:=|\s+)(\S+)/)?.[1] : undefined;
  const codexServerSocket = remote?.startsWith("unix:///") ? remote.slice("unix://".length) : null;
  return { pid: row.pid, processStartTime: row.processStartTime, provider, ...(remote ? { codexServerSocket } : {}) };
}
