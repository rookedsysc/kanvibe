import type { AiSessionReaderContext } from "./types";
import { withCodexAppServer } from "./codexAppServer";
import { codexMainThreadSource } from "./codexThreadMetadata";

type CodexUserInput = { type: "text"; text: string } | { type: "localImage"; path: string };

export interface CodexThreadInput {
  context: AiSessionReaderContext;
  socket?: string | null;
  threadId: string;
  sourceRef: string;
  input: CodexUserInput[];
  clientUserMessageId: string;
  beforeSend(): Promise<void>;
  onWrite(): void;
}

/** 대상을 주소로 고정한다. clientUserMessageId는 표시 연결용이며 서버 중복 방지나 재시도 권한이 아니다 */
export async function sendCodexThreadInput(options: CodexThreadInput): Promise<void> {
  const { context, threadId, sourceRef, input, clientUserMessageId } = options;
  if (!input.length || !clientUserMessageId) throw new Error("보낼 Codex 입력이 없습니다.");
  await withCodexAppServer(context, options.socket, async (connection) => {
    const { thread } = await connection.request<{ thread: Record<string, unknown> }>("thread/read", { threadId, includeTurns: false });
    if (codexMainThreadSource(thread, context, connection.codexHome, threadId) !== sourceRef) throw new Error("CLI와 같은 작업의 Codex 대화를 확인하지 못했습니다.");
    if (!["idle", "active"].includes(String((thread.status as { type?: string } | undefined)?.type))) throw new Error("Codex가 입력을 받을 수 없는 상태입니다.");
    await options.beforeSend();
    const result = await connection.request<{ turn?: { id?: string } }>("turn/start", { threadId, input, clientUserMessageId }, options.onWrite);
    if (!result.turn?.id) throw new Error("Codex 입력 수락 결과를 확인하지 못했습니다.");
  });
}
