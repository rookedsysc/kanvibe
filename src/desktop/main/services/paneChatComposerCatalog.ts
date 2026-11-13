import { open, readdir, realpath, stat } from "node:fs/promises";
import path from "node:path";
import { getHomeDirectory } from "@/lib/hostFileAccess";
import { execGit } from "@/lib/gitOperations";
import { quoteForPosixShell } from "@/lib/worktree";
import { MAX_PANE_CHAT_SUGGESTIONS, type PaneChatSuggestion, type PaneChatSuggestionKind } from "@/desktop/shared/paneChat";

const MAX_CATALOG_FILES = 512;
const MAX_CATALOG_DEPTH = 8;
const MAX_CATALOG_DIRECTORIES = 2048;
const CATALOG_READ_CONCURRENCY = 32;
const MAX_SKILL_BYTES = 64 * 1024;
const CATALOG_TIMEOUT_MS = 5_000;
const SKILL_NAME = /^[A-Za-z0-9][A-Za-z0-9_.:-]*$/;
interface CatalogContext {
  provider: "claude" | "codex";
  cwd: string;
  sshHost?: string | null;
  homeDirectory?: string;
  providerHome?: string;
  processId?: number;
  workspacePath?: string;
}
interface CatalogRoot { directory: string; commands: boolean }

function fileMention(filePath: string): string {
  return /[\s@"']/.test(filePath) ? `@"${filePath.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"` : `@${filePath}`;
}

async function providerRoots(context: CatalogContext): Promise<{ home: string; provider: string }> {
  let home = context.homeDirectory ?? await getHomeDirectory(context.sshHost);
  let configuredProvider = context.providerHome;
  if (context.processId) {
    try {
      const output = await execGit(`ps eww -p ${context.processId} -o command=`, context.sshHost, { timeoutMs: CATALOG_TIMEOUT_MS });
      const variable = context.provider === "codex" ? "CODEX_HOME" : "CLAUDE_CONFIG_DIR";
      const read = (key: string) => {
        const candidate = [...output.matchAll(new RegExp(`(?:^|\\s)${key}=([\\s\\S]*?)(?=\\s[A-Za-z_][A-Za-z0-9_]*=|$)`, "g"))].at(-1)?.[1];
        return candidate && path.posix.isAbsolute(candidate) && !/[\r\n\x00]/.test(candidate) ? candidate : null;
      };
      if (!context.homeDirectory) home = read("HOME") ?? home;
      if (!context.providerHome) configuredProvider = read(variable) ?? undefined;
    } catch {
      // 환경 조회 권한이 없으면 확인 가능한 기본 설치 경로만 제안한다.
    }
  }
  return { home, provider: configuredProvider ?? path.posix.join(home, context.provider === "codex" ? ".codex" : ".claude") };
}

async function localFiles(roots: CatalogRoot[]): Promise<{ files: { filePath: string; root: CatalogRoot }[]; truncated: boolean }> {
  const files: { filePath: string; root: CatalogRoot }[] = [];
  const visited = new Set<string>();
  let truncated = false;
  const walk = async (directory: string, root: CatalogRoot, depth: number): Promise<void> => {
    if (depth > MAX_CATALOG_DEPTH || files.length >= MAX_CATALOG_FILES || visited.size >= MAX_CATALOG_DIRECTORIES) { truncated = true; return; }
    const canonical = await realpath(directory).catch(() => null);
    if (!canonical || visited.has(canonical)) return;
    visited.add(canonical);
    const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
    for (const entry of entries) {
      if ([".git", "node_modules", ".venv"].includes(entry.name)) continue;
      const filePath = path.join(directory, entry.name);
      if (entry.isDirectory() || (entry.isSymbolicLink() && (await stat(filePath).catch(() => null))?.isDirectory())) {
        await walk(filePath, root, depth + 1);
      } else if ((root.commands && entry.name.endsWith(".md")) || (!root.commands && entry.name === "SKILL.md")) {
        files.push({ filePath, root });
        if (files.length >= MAX_CATALOG_FILES) { truncated = true; return; }
      }
    }
  };
  for (const root of roots) await walk(root.directory, root, 0);
  return { files, truncated };
}

async function catalogFiles(context: CatalogContext, roots: CatalogRoot[]): Promise<{ files: { filePath: string; root: CatalogRoot; content: string }[]; truncated: boolean }> {
  if (!context.sshHost) {
    const scanned = await localFiles(roots);
    const read = async (entry: { filePath: string; root: CatalogRoot }) => {
      const handle = await open(entry.filePath, "r").catch(() => null);
      if (!handle) return null;
      try {
        const buffer = Buffer.alloc(MAX_SKILL_BYTES);
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
        return { ...entry, content: buffer.subarray(0, bytesRead).toString("utf8") };
      } catch { return null; }
      finally { await handle.close(); }
    };
    const files: ({ filePath: string; root: CatalogRoot; content: string } | null)[] = [];
    for (let offset = 0; offset < scanned.files.length; offset += CATALOG_READ_CONCURRENCY) files.push(...await Promise.all(scanned.files.slice(offset, offset + CATALOG_READ_CONCURRENCY).map(read)));
    return { files: files.filter((entry) => entry !== null), truncated: scanned.truncated };
  }
  const found: { filePath: string; root: CatalogRoot }[] = [];
  for (const root of roots) {
    const pattern = root.commands ? "*.md" : "SKILL.md";
    const output = await execGit(`test ! -d ${quoteForPosixShell(root.directory)} || find -L ${quoteForPosixShell(root.directory)} -maxdepth ${MAX_CATALOG_DEPTH} \\( -name .git -o -name node_modules -o -name .venv \\) -prune -o -type f -name ${quoteForPosixShell(pattern)} -print0 2>/dev/null || true`, context.sshHost, { timeoutMs: CATALOG_TIMEOUT_MS });
    for (const filePath of output.split("\0").filter(Boolean)) found.push({ filePath, root });
    if (found.length >= MAX_CATALOG_FILES) break;
  }
  const selected = found.slice(0, MAX_CATALOG_FILES);
  if (!selected.length) return { files: [], truncated: false };
  const command = `for __kanvibe_skill in ${selected.map((entry) => quoteForPosixShell(entry.filePath)).join(" ")}; do printf '%s\\0' "$__kanvibe_skill"; head -c ${MAX_SKILL_BYTES} "$__kanvibe_skill" 2>/dev/null | base64 | tr -d '\\r\\n'; printf '\\0'; done`;
  const parts = (await execGit(command, context.sshHost, { timeoutMs: CATALOG_TIMEOUT_MS, maxBufferBytes: MAX_CATALOG_FILES * MAX_SKILL_BYTES * 2 })).split("\0");
  const contents = new Map<string, string>();
  for (let index = 0; index + 1 < parts.length; index += 2) if (parts[index + 1]) contents.set(parts[index], Buffer.from(parts[index + 1], "base64").toString("utf8"));
  return { files: selected.flatMap((entry) => contents.has(entry.filePath) ? [{ ...entry, content: contents.get(entry.filePath)! }] : []), truncated: found.length >= MAX_CATALOG_FILES };
}

function skillMetadata(content: string, fallbackName: string): { name: string; description: string } | null {
  if (!content.trim()) return null;
  const header = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(content)?.[1];
  if (header === undefined && content.startsWith("---")) return null;
  if (header && /^user-invocable:\s*false\s*$/m.test(header)) return null;
  const field = (key: string) => header?.match(new RegExp(`^${key}:\\s*(.*)$`, "m"))?.[1]?.trim().replace(/^(['"])(.*)\1$/, "$2");
  const name = field("name") ?? fallbackName;
  if (!SKILL_NAME.test(name)) return null;
  const description = field("description");
  return { name, description: description === ">" || description === "|" ? (header?.match(/^description:\s*[>|]\s*\n((?:[ \t]+.*\n?)*)/m)?.[1] ?? "").trim().replace(/\s+/g, " ").slice(0, 512) : (description ?? content.split("\n").find((line) => line.trim() && !line.startsWith("#")) ?? "").slice(0, 512) };
}

export async function readPaneChatSuggestions(context: CatalogContext, kind: PaneChatSuggestionKind, query: string): Promise<{ items: PaneChatSuggestion[]; truncated: boolean }> {
  if (kind === "file") {
    const workspace = context.workspacePath ?? context.cwd;
    const output = await execGit(`printf '\\0' && git -C ${quoteForPosixShell(workspace)} ls-files --cached --others --exclude-standard -z`, context.sshHost, { timeoutMs: CATALOG_TIMEOUT_MS });
    const matches = [...new Set(output.split("\0").filter((file) => file && !/[\x00-\x1f\x7f]/.test(file) && file.toLowerCase().includes(query.toLowerCase())))].sort();
    return { items: matches.slice(0, MAX_PANE_CHAT_SUGGESTIONS).map((file) => ({ id: `file:${file}`, label: file, description: "", insertText: fileMention(path.posix.join(workspace, file)) })), truncated: matches.length > MAX_PANE_CHAT_SUGGESTIONS };
  }
  const { home, provider } = await providerRoots(context);
  const projectPaths = [context.cwd];
  if (context.workspacePath && context.cwd.startsWith(`${context.workspacePath}/`)) {
    let ancestor = path.posix.dirname(context.cwd);
    while (ancestor.startsWith(`${context.workspacePath}/`)) { projectPaths.push(ancestor); ancestor = path.posix.dirname(ancestor); }
  }
  if (context.workspacePath && !projectPaths.includes(context.workspacePath)) projectPaths.push(context.workspacePath);
  const roots: CatalogRoot[] = [
    ...projectPaths.flatMap((project) => [
      { directory: path.posix.join(project, context.provider === "claude" ? ".claude/skills" : ".agents/skills"), commands: false },
      ...(context.provider === "claude" ? [{ directory: path.posix.join(project, ".claude/commands"), commands: true }] : [{ directory: path.posix.join(project, ".codex/skills"), commands: false }]),
    ]),
    { directory: path.posix.join(home, ".agents/skills"), commands: false },
    { directory: path.posix.join(provider, "skills"), commands: false },
    { directory: path.posix.join(provider, context.provider === "claude" ? "commands" : "prompts"), commands: true },
    { directory: path.posix.join(provider, "plugins/cache"), commands: false },
  ];
  const catalog = await catalogFiles(context, roots);
  const unique = new Map<string, PaneChatSuggestion>();
  for (const entry of catalog.files) {
    if (kind === "skill" && entry.root.commands) continue;
    const fallback = entry.root.commands ? path.posix.relative(entry.root.directory, entry.filePath).replace(/\.md$/, "").replaceAll("/", ":") : path.posix.basename(path.posix.dirname(entry.filePath));
    const metadata = skillMetadata(entry.content, fallback);
    if (!metadata || unique.has(metadata.name)) continue;
    const prefix = entry.root.commands || context.provider === "claude" ? "/" : "$";
    const commandName = entry.root.commands && context.provider === "codex" ? `prompts:${metadata.name}` : metadata.name;
    unique.set(metadata.name, { id: `${entry.root.commands ? "command" : "skill"}:${metadata.name}`, label: metadata.name, description: metadata.description, insertText: `${prefix}${commandName}` });
  }
  if (kind === "command") for (const command of ["model", "help"]) if (!unique.has(command)) unique.set(command, { id: `command:${command}`, label: command, description: command === "model" ? "모델 선택" : "CLI 도움말", insertText: `/${command}` });
  const matches = [...unique.values()].filter((entry) => `${entry.label} ${entry.description}`.toLowerCase().includes(query.toLowerCase())).sort((a, b) => a.label.localeCompare(b.label));
  return { items: matches.slice(0, MAX_PANE_CHAT_SUGGESTIONS), truncated: catalog.truncated || matches.length > MAX_PANE_CHAT_SUGGESTIONS };
}
