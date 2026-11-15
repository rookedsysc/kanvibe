import path from "node:path";
import { pathExists } from "@/lib/hostFileAccess";
import { determineMatchScope } from "./shared";
import { withCodexAppServer } from "./codexAppServer";
import type { AiSessionReaderContext } from "./types";

const MAIN_SOURCES = new Set(["cli", "vscode", "exec", "appServer"]);

/** 같은 계정·작업의 메인 대화만 허용하며, 본문이나 다른 대화의 경로를 입력 대상으로 사용하지 않는다 */
export function codexMainThreadSource(thread: Record<string, unknown>, context: AiSessionReaderContext, codexHome: string, threadId: string): string | null {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(threadId)
    || thread.id !== threadId || typeof thread.cwd !== "string" || !determineMatchScope(thread.cwd, context)
    || thread.parentThreadId !== null || !MAIN_SOURCES.has(String(thread.source)) || thread.ephemeral !== false
    || thread.canAcceptDirectInput !== true || typeof thread.path !== "string") return null;
  const source = path.posix.normalize(thread.path);
  return source.startsWith(`${codexHome}/sessions/`) && path.posix.basename(source).endsWith(`-${threadId}.jsonl`) ? source : null;
}

/** 첫 입력 전 JSONL이 없는 새 대화만 확인한다. 기존 TUI의 구독이나 설정을 바꾸지 않는다 */
export async function readCodexEmptyThread(context: AiSessionReaderContext, threadId: string, explicitSocket?: string | null): Promise<string | null> {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(threadId)) return null;
  try {
    return await withCodexAppServer(context, explicitSocket, async (connection) => {
      const { thread } = await connection.request<{ thread: Record<string, unknown> }>("thread/read", { threadId, includeTurns: false });
      const source = codexMainThreadSource(thread, context, connection.codexHome, threadId);
      return source && thread.preview === "" && !await pathExists(source, context.sshHost) ? source : null;
    });
  } catch { return null; }
}
