export type PaneChatProvider = "claude" | "codex" | "opencode" | "gemini";

export interface PaneChatProcess {
  pid: number;
  processStartTime: string;
  provider: PaneChatProvider;
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
const INTERPRETERS = /^(?:node|nodejs|python(?:\d+(?:\.\d+)?)?|ruby|bash|sh|zsh)$/;

function detectProvider(row: Pick<ProcessRow, "command" | "args">): PaneChatProvider | null {
  const executable = row.command.split("/").at(-1)?.replace(/\.exe$/i, "").toLowerCase() ?? "";
  if (PROVIDERS.has(executable as PaneChatProvider)) return executable as PaneChatProvider;
  if (!INTERPRETERS.test(executable)) return null;
  const script = row.args.match(/^\S+\s+([^\s]+)/)?.[1]?.replace(/^["']|["']$/g, "") ?? "";
  const basename = script.split("/").at(-1) ?? "";
  if (PROVIDERS.has(basename as PaneChatProvider)) return basename as PaneChatProvider;
  if (/\/@anthropic-ai\/claude-code\/.*(?:cli|index)\.js$/.test(script)) return "claude";
  if (/\/@google\/gemini-cli\/.*(?:cli|index)\.js$/.test(script)) return "gemini";
  if (/\/@openai\/codex\/.*codex\.js$/.test(script)) return "codex";
  return null;
}

export function isPaneChatProviderProcess(command: string, args: string, provider: "claude" | "codex"): boolean {
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
    const wrapper = candidates.find((candidate) => /^(?:node|nodejs)$/i.test(candidate.command.split("/").at(-1) ?? ""));
    const native = candidates.find((candidate) => candidate.parentPid === wrapper?.pid && candidate.command.split("/").at(-1) === "codex");
    const knownWrapper = wrapper && /\/@openai\/codex\/(?:bin\/)?codex\.js(?:\s|$)/.test(wrapper.args);
    const knownNative = native && /(?:^|\s)\S*\/@openai\/codex(?:-[^/]+)?\/\S*\/codex(?:\.exe)?(?:\s|$)/.test(native.args);
    // npm wrapper만 stdin을 native 자식에게 잇는다. 에이전트가 실행한 동종 CLI는 입력 소유자가 아니다.
    if (candidates.length !== 2 || !wrapper || !native || (!knownWrapper && !knownNative)) return null;
    row = native;
  }
  return { pid: row.pid, processStartTime: row.processStartTime, provider: detectProvider(row)! };
}
