import { writeFile, mkdir, access, readFile } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { buildFetchAuthHeaders } from "@/lib/hookAuth";
import { KANVIBE_TASK_ID_RELATIVE_PATH, readHookTaskIdFile, writeHookTaskIdFile } from "@/lib/hookTaskBinding";

/**
 * OpenCode는 `.opencode/plugins/` 디렉토리에 TypeScript 플러그인을 배치하여 hooks를 등록한다.
 * message.updated(user) → progress, question.asked → pending,
 * question.replied → progress, session.idle → review, session.deleted → done 상태를 전송한다.
 */

export const PLUGIN_FILE_NAME = "kanvibe-plugin.ts";
export const PLUGIN_DIR_NAME = "plugins";

/** OpenCode plugin TypeScript 파일 내용을 생성한다 */
export function generatePluginScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `import { readFile } from "node:fs/promises";
import type { Plugin } from "@opencode-ai/plugin";

/**
 * KanVibe OpenCode Plugin
 * message.updated(user) → progress, question.asked → pending,
 * question.replied → progress, session.idle → review, session.deleted → done 상태 변경
 */
export const KanvibePlugin: Plugin = async ({ client }) => {
  const KANVIBE_URL = "${kanvibeUrl}";
  const DEFAULT_TASK_ID = "${taskId}";
  const TASK_ID_FILE = "${KANVIBE_TASK_ID_RELATIVE_PATH}";
  const lastStatusBySession = new Map<string, string>();
  const lastUserMessageBySession = new Map<string, string>();

  async function resolveTaskId(): Promise<string> {
    try {
      const taskId = (await readFile(TASK_ID_FILE, "utf-8")).trim();
      if (taskId.length > 0) {
        return taskId;
      }
    } catch {
      /* taskId 파일이 없으면 기본값 사용 */
    }

    return DEFAULT_TASK_ID;
  }

  function getSessionID(source: any): string | undefined {
    return (
      source?.sessionID ??
      source?.sessionId ??
      source?.id ??
      source?.session?.id ??
      source?.info?.sessionID ??
      source?.info?.sessionId ??
      source?.info?.id
    );
  }

  function buildMessageSignature(source: any): string | undefined {
    const parts = [
      source?.messageID,
      source?.messageId,
      source?.id,
      source?.timeCreated,
      source?.time_created,
      source?.createdAt,
      source?.updatedAt,
      source?.timestamp,
      typeof source?.content === "string" ? source.content : undefined,
    ].filter((value): value is string | number => value !== undefined && value !== null);

    if (parts.length === 0) return undefined;
    return parts.join(":");
  }

  async function updateStatus(source: any, status: string, options?: { dedupeMessage?: boolean }): Promise<void> {
    const sessionID = getSessionID(source);

    if (options?.dedupeMessage && sessionID) {
      const signature = buildMessageSignature(source);
      if (signature) {
        if (lastUserMessageBySession.get(sessionID) === signature) {
          return;
        }
        lastUserMessageBySession.set(sessionID, signature);
      }
    }

    if (sessionID && lastStatusBySession.get(sessionID) === status) {
      return;
    }

    try {
      const resolvedTaskId = await resolveTaskId();
      await fetch(\`\${KANVIBE_URL}/api/hooks/status\`, {
        method: "POST",
        headers: ${buildFetchAuthHeaders(authToken)},
        body: JSON.stringify({ taskId: resolvedTaskId, status }),
      });
      if (sessionID) {
        lastStatusBySession.set(sessionID, status);
      }
    } catch {
      /* 네트워크 에러 무시 */
    }
  }

  const sessionCache = new Map<string, boolean>();

  function getParentSessionID(source: any): string | null | undefined {
    return (
      source?.parentID ??
      source?.parentId ??
      source?.session?.parentID ??
      source?.session?.parentId ??
      source?.info?.parentID ??
      source?.info?.parentId
    );
  }

  async function isMainSession(source: any): Promise<boolean> {
    const sessionID = getSessionID(source);
    if (!sessionID) return false;

    const parentSessionID = getParentSessionID(source);
    if (parentSessionID !== undefined) {
      const isMain = !parentSessionID;
      sessionCache.set(sessionID, isMain);
      return isMain;
    }

    if (sessionCache.has(sessionID)) return sessionCache.get(sessionID)!;

    try {
      const result = await client.session.get({
        path: { id: sessionID },
      });

      if (result.error) return false;

      const isMain = !result.data?.parentID;
      sessionCache.set(sessionID, isMain);

      return isMain;
    } catch {
      return sessionCache.get(sessionID) ?? false;
    }
  }

  return {
    event: async ({ event }) => {
      if (event.type === "message.updated") {
        const message =
          (event as any).properties?.info ?? (event as any).properties?.message;

        if (message?.role === "user" && (await isMainSession(message))) {
          await updateStatus(message, "progress", { dedupeMessage: true });
        }
      }
      if (event.type === "question.asked") {
        if (!(await isMainSession(event.properties))) {
          return;
        }

        await updateStatus(event.properties, "pending");
      }
      if (event.type === "question.replied") {
        if (!(await isMainSession(event.properties))) {
          return;
        }

        await updateStatus(event.properties, "progress");
      }
      if (event.type === "session.idle") {
        if (!(await isMainSession(event.properties))) {
          return;
        }

        await updateStatus(event.properties, "review");
      }
      if (event.type === "session.deleted") {
        if (!(await isMainSession(event.properties))) {
          return;
        }

        await updateStatus(event.properties, "done");
      }
    },
  };
};
`;
}

/** 플러그인 파일에 kanvibe 관련 코드가 포함되어 있는지 확인한다 */
function hasKanvibePlugin(pluginContent: string): boolean {
  return pluginContent.includes("KanvibePlugin") && pluginContent.includes("/api/hooks/status");
}

/**
 * 지정된 repo에 OpenCode plugin을 설정한다.
 * `.opencode/plugins/kanvibe-plugin.ts` 파일을 생성한다.
 */
export async function setupOpenCodeHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  authToken?: string,
): Promise<void> {
  const openCodeDir = path.join(repoPath, ".opencode");
  const pluginsDir = path.join(openCodeDir, PLUGIN_DIR_NAME);

  await mkdir(pluginsDir, { recursive: true });

  const pluginPath = path.join(pluginsDir, PLUGIN_FILE_NAME);
  await writeHookTaskIdFile(repoPath, taskId);
  await writeFile(pluginPath, generatePluginScript(kanvibeUrl, taskId, authToken), "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface OpenCodeHooksStatus {
  installed: boolean;
  hasPlugin: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusEndpoint?: boolean;
  hasEventMappings?: boolean;
  hasMainSessionGuard?: boolean;
  hasDuplicateProgressGuard?: boolean;
  boundTaskId?: string | null;
}

/** 지정된 repo의 OpenCode plugin 설치 상태를 확인한다 */
export async function getOpenCodeHooksStatus(repoPath: string, taskId?: string): Promise<OpenCodeHooksStatus> {
  const openCodeDir = path.join(repoPath, ".opencode");
  const pluginsDir = path.join(openCodeDir, PLUGIN_DIR_NAME);
  const pluginPath = path.join(pluginsDir, PLUGIN_FILE_NAME);

  const pluginExists = await access(pluginPath)
    .then(() => true)
    .catch(() => false);

  let hasPlugin = false;
  const boundTaskId = await readHookTaskIdFile(repoPath);
  let hasTaskIdBinding = !taskId;
  let hasStatusEndpoint = false;
  let hasEventMappings = false;
  let hasMainSessionGuard = false;
  let hasDuplicateProgressGuard = false;
  if (pluginExists) {
    try {
      const content = await readFile(pluginPath, "utf-8");
      hasPlugin = hasKanvibePlugin(content);
      hasTaskIdBinding =
        !taskId || (
          content.includes(`const DEFAULT_TASK_ID = \"${taskId}\";`) &&
          content.includes(`const TASK_ID_FILE = \"${KANVIBE_TASK_ID_RELATIVE_PATH}\";`) &&
          content.includes("resolveTaskId") &&
          content.includes("taskId: resolvedTaskId") &&
          boundTaskId === taskId
        );
      hasStatusEndpoint = content.includes("/api/hooks/status");
      hasEventMappings = ["progress", "pending", "review", "done", "message.updated", "question.asked", "question.replied", "session.idle", "session.deleted"].every((fragment) => content.includes(fragment));
      hasMainSessionGuard = content.includes("isMainSession(message)") && content.includes("isMainSession(event.properties)");
      hasDuplicateProgressGuard = content.includes("lastUserMessageBySession") && content.includes("buildMessageSignature") && content.includes("dedupeMessage: true");
    } catch {
      /* 파일 읽기 실패 */
    }
  }

  const installed = pluginExists && hasPlugin && hasTaskIdBinding && hasStatusEndpoint && hasEventMappings && hasMainSessionGuard && hasDuplicateProgressGuard;

  return {
    installed,
    hasPlugin,
    hasTaskIdBinding,
    hasStatusEndpoint,
    hasEventMappings,
    hasMainSessionGuard,
    hasDuplicateProgressGuard,
    boundTaskId,
  };
}
