import { withCodexAppServer, type CodexAppServerConnection } from "./codexAppServer";
import { codexMainThreadSource } from "./codexThreadMetadata";
import { buildPaneChatModelCommand, type PaneChatModelSettings, type PaneChatModelSettingChange } from "./paneChatModelSettings";
import type { AiSessionReaderContext } from "./types";

export interface CodexThreadSettingsTarget {
  context: AiSessionReaderContext;
  socket?: string | null;
  threadId: string;
  sourceRef: string;
  beforeWrite?: () => Promise<void>;
}
interface ApiModel {
  model: string;
  displayName: string;
  hidden: boolean;
  supportedReasoningEfforts: { reasoningEffort: string; description: string }[];
  serviceTiers: { id: string }[];
}

async function readSettings(connection: CodexAppServerConnection, target: CodexThreadSettingsTarget): Promise<PaneChatModelSettings> {
  const { thread } = await connection.request<{ thread: Record<string, unknown> }>("thread/read", { threadId: target.threadId, includeTurns: false });
  if (codexMainThreadSource(thread, target.context, connection.codexHome, target.threadId) !== target.sourceRef) throw new Error("현재 CLI의 Codex 대화를 확인하지 못했습니다.");
  const models: ApiModel[] = [];
  let cursor: string | null = null;
  do {
    const page: { data: ApiModel[]; nextCursor: string | null } = await connection.request("model/list", { cursor, limit: 100, includeHidden: false, ...(typeof thread.modelProvider === "string" ? { modelProvider: thread.modelProvider } : {}) });
    models.push(...page.data);
    if (page.nextCursor === cursor && cursor !== null) throw new Error("Codex 모델 목록을 끝까지 읽지 못했습니다.");
    cursor = page.nextCursor;
  } while (cursor);
  return {
    model: typeof thread.model === "string" ? thread.model : null,
    effort: typeof thread.reasoningEffort === "string" ? thread.reasoningEffort : null,
    isFast: null,
    models: models.filter((model) => !model.hidden).map((model) => ({ id: model.model, label: model.displayName, effortLevels: model.supportedReasoningEfforts.map((level) => ({ id: level.reasoningEffort, label: level.reasoningEffort })), supportsFast: model.serviceTiers?.some((tier) => tier.id === "priority") ?? false })),
  };
}

/** 기존 CLI 계정의 실제 모델 목록과 현재 대화 설정을 API에서 읽는다. */
export function readCodexThreadSettings(target: CodexThreadSettingsTarget): Promise<PaneChatModelSettings> {
  return withCodexAppServer(target.context, target.socket, (connection) => readSettings(connection, target));
}

/** 같은 대화의 이후 턴 설정만 바꾼다. 권한·로그인·작업 디렉터리·구독은 변경하지 않는다. */
export function updateCodexThreadSettings(target: CodexThreadSettingsTarget, change: PaneChatModelSettingChange): Promise<void> {
  return withCodexAppServer(target.context, target.socket, async (connection) => {
    const settings = await readSettings(connection, target);
    if (!buildPaneChatModelCommand("codex", settings, change)) throw new Error("현재 모델에서 지원하지 않는 설정입니다.");
    const params = change.kind === "model" ? { model: change.model } : change.kind === "effort" ? { effort: change.effort } : change.kind === "fast" ? { serviceTier: change.isFast ? "priority" : null } : { effort: null };
    await target.beforeWrite?.();
    await connection.request("thread/settings/update", { threadId: target.threadId, ...params });
  });
}
