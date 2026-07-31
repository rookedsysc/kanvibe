import { pathToFileURL } from "node:url";
import {
  KANVIBE_GIT_EXCLUDE_MARKER,
  KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
} from "@/lib/gitExclude";
import { readTextFiles } from "@/lib/hostFileAccess";
import { writeHookProviderFiles } from "@/lib/hookFileWriter";
import { extractPluginHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import {
  registerKanvibeHookTarget,
  verifyHookTargetRegistration,
  type HookTargetRegistrationStatus,
} from "@/lib/hookTargetRegistration";
import { getKanvibeTargetsPath } from "@/lib/kanvibeProjectState";
import { getOpenCodeRegisteredKanvibePluginUrls } from "@/lib/openCodePluginRegistry";
import { resolvePathModule, type ShellHookProviderFile } from "@/lib/shellHookProvider";

/**
 * OpenCode는 `.opencode/plugins/` 디렉토리에 TypeScript 플러그인을 배치하여 hooks를 등록한다.
 * message.updated(user) → progress, question.asked → pending,
 * question.replied → progress, session.idle → review, session.deleted → done 상태를 전송한다.
 */

export const PLUGIN_FILE_NAME = "kanvibe-plugin.ts";
export const PLUGIN_DIR_NAME = "plugins";
const CONFIG_DIR_NAME = ".opencode";

/** OpenCode plugin TypeScript 파일 내용을 생성한다 */
export function generatePluginScript(kanvibeUrl: string, taskId: string): string {
  return `import type { Plugin } from "@opencode-ai/plugin";
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { execFileSync } from "child_process";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

/**
 * KanVibe OpenCode Plugin
 * message.updated(user) → progress, question.asked → pending,
 * question.replied → progress, session.idle → review, session.deleted → done 상태 변경
 */
export const KanvibePlugin: Plugin = async ({ client }) => {
  const KANVIBE_URL = "${kanvibeUrl}";
  const TASK_ID = ${JSON.stringify(taskId)};
  const KANVIBE_REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
  const KANVIBE_STATE_DIR = resolve(KANVIBE_REPO_ROOT, ".kanvibe");
  const KANVIBE_STATUS_FILE = resolve(KANVIBE_STATE_DIR, "status.json");
  const KANVIBE_TARGETS_FILE = resolve(KANVIBE_STATE_DIR, "targets.json");
  const KANVIBE_STATE_DIR_EXCLUDE_PATTERN = ${JSON.stringify(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)};
  const KANVIBE_GIT_EXCLUDE_MARKER = ${JSON.stringify(KANVIBE_GIT_EXCLUDE_MARKER)};
  const KANVIBE_PROJECT_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;
  const lastStatusBySession = new Map<string, string>();
  const lastUserMessageBySession = new Map<string, string>();

  function ensureKanvibeStatusExcluded(): void {
    try {
      const gitCommonDir = execFileSync(
        "git",
        ["-C", KANVIBE_REPO_ROOT, "rev-parse", "--path-format=absolute", "--git-common-dir"],
        { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
      ).trim();
      if (!gitCommonDir) return;

      const excludeFile = resolve(gitCommonDir, "info", "exclude");
      mkdirSync(dirname(excludeFile), { recursive: true });

      const content = existsSync(excludeFile) ? readFileSync(excludeFile, "utf8") : "";
      const lines = content.split(/\\r?\\n/);
      if (lines.includes(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)) return;

      const markerPrefix = lines.includes(KANVIBE_GIT_EXCLUDE_MARKER)
        ? ""
        : "\\n" + KANVIBE_GIT_EXCLUDE_MARKER + "\\n";
      appendFileSync(excludeFile, markerPrefix + KANVIBE_STATE_DIR_EXCLUDE_PATTERN + "\\n", "utf8");
    } catch {
      /* git exclude 갱신 에러 무시 */
    }
  }

  /** 다른 KanVibe client가 기록한 프로젝트 색상은 상태를 덮어쓸 때도 보존한다 */
  function readKanvibeProjectColor(): string | null {
    try {
      const parsed = JSON.parse(readFileSync(KANVIBE_STATUS_FILE, "utf8"));
      const projectColor = typeof parsed?.projectColor === "string" ? parsed.projectColor.trim() : "";
      return KANVIBE_PROJECT_COLOR_PATTERN.test(projectColor) ? projectColor : null;
    } catch {
      return null;
    }
  }

  function writeKanvibeTaskState(status: string): void {
    try {
      ensureKanvibeStatusExcluded();
      const projectColor = readKanvibeProjectColor();
      mkdirSync(KANVIBE_STATE_DIR, { recursive: true });
      writeFileSync(
        KANVIBE_STATUS_FILE,
        JSON.stringify({
          schemaVersion: 1,
          status,
          updatedAt: new Date().toISOString(),
          ...(projectColor ? { projectColor } : {}),
        }, null, 2) + "\\n",
        "utf8",
      );
    } catch {
      /* 파일 쓰기 에러 무시 */
    }
  }

  type KanvibeTarget = { url: string; taskId: string };

  function normalizeKanvibeUrl(url: string): string {
    return url.trim().replace(/\\/+$/, "");
  }

  function getFallbackKanvibeTarget(): KanvibeTarget[] {
    return [{ url: normalizeKanvibeUrl(KANVIBE_URL), taskId: TASK_ID }];
  }

  /** targets.json은 client(url) 단위로 등록되므로 같은 url의 중복 항목만 제거한다 */
  function readKanvibeTargets(): KanvibeTarget[] {
    try {
      const parsed = JSON.parse(readFileSync(KANVIBE_TARGETS_FILE, "utf8"));
      const targets = Array.isArray(parsed?.targets) ? parsed.targets : [];
      const seenUrls = new Set<string>();
      const normalizedTargets: KanvibeTarget[] = [];

      for (const target of targets) {
        const url = typeof target?.url === "string" ? normalizeKanvibeUrl(target.url) : "";
        const taskId = typeof target?.taskId === "string" ? target.taskId.trim() : "";
        if (!url || !taskId || seenUrls.has(url)) continue;

        seenUrls.add(url);
        normalizedTargets.push({ url, taskId });
      }

      return normalizedTargets.length > 0 ? normalizedTargets : getFallbackKanvibeTarget();
    } catch {
      return getFallbackKanvibeTarget();
    }
  }

  async function postKanvibeStatus(target: KanvibeTarget, status: string): Promise<void> {
    try {
      await fetch(target.url + "/api/hooks/status", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ taskId: target.taskId, status }),
      });
    } catch {
      /* 네트워크 에러 무시 */
    }
  }

  /** 등록된 모든 client에 병렬로 통보한다. 느린 client가 다른 client를 막지 않는다 */
  async function fanoutKanvibeStatus(status: string): Promise<void> {
    await Promise.all(readKanvibeTargets().map((target) => postKanvibeStatus(target, status)));
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

    writeKanvibeTaskState(status);
    await fanoutKanvibeStatus(status);

    if (sessionID) {
      lastStatusBySession.set(sessionID, status);
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

/** OpenCode plugin은 기존 설정 파일을 병합하지 않으므로 미리 읽어야 할 파일이 없다 */
export function getOpenCodeHookInstallInputPaths(): string[] {
  return [];
}

/** OpenCode plugin 설치 산출물을 만든다 */
export function buildOpenCodeHookFiles(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  sshHost?: string | null,
): ShellHookProviderFile[] {
  return [
    {
      filePath: getOpenCodePluginPath(repoPath, sshHost),
      content: generatePluginScript(kanvibeUrl, taskId),
    },
  ];
}

/**
 * 지정된 repo에 OpenCode plugin을 설정한다.
 * `.opencode/plugins/kanvibe-plugin.ts` 파일을 생성한다.
 */
export async function setupOpenCodeHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  sshHost?: string | null,
): Promise<void> {
  await writeHookProviderFiles(buildOpenCodeHookFiles(repoPath, taskId, kanvibeUrl, sshHost), sshHost);

  await registerKanvibeHookTarget(repoPath, taskId, kanvibeUrl, sshHost);
}

export interface OpenCodeHooksStatus extends Partial<HookTargetRegistrationStatus> {
  installed: boolean;
  hasPlugin: boolean;
  hasRegisteredPlugin?: boolean;
  hasDuplicateKanvibePlugins?: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusEndpoint?: boolean;
  hasEventMappings?: boolean;
  hasStatusJsonPersistence?: boolean;
  hasProjectColorPersistence?: boolean;
  hasTargetFanout?: boolean;
  hasParallelTargetFanout?: boolean;
  hasMainSessionGuard?: boolean;
  hasDuplicateProgressGuard?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  targetPath?: string | null;
  pluginPath?: string | null;
  registeredPluginUrls?: string[];
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}

/** 지정된 repo의 OpenCode plugin 설치 상태를 확인한다 */
export async function getOpenCodeHooksStatus(
  repoPath: string,
  taskId?: string,
  sshHost?: string | null,
): Promise<OpenCodeHooksStatus> {
  const pluginPath = getOpenCodePluginPath(repoPath, sshHost);
  const targetsPath = getKanvibeTargetsPath(repoPath, sshHost);
  const files = await readTextFiles([pluginPath, targetsPath], sshHost);
  const pluginFile = files.get(pluginPath) ?? { exists: false, content: "" };
  const pluginContent = pluginFile.content;

  const boundTaskId = extractPluginTaskId(pluginContent);
  const hasTaskIdBinding = boundTaskId !== null && pluginContent.includes("taskId: TASK_ID");
  const pluginRegistration = await resolveOpenCodePluginRegistration(repoPath, pluginPath, sshHost);
  const hookServerValidation = await validateHookServerConfiguration(
    [extractPluginHookServerUrl(pluginContent)],
    Boolean(taskId),
    sshHost,
  );
  const targetRegistration = verifyHookTargetRegistration(
    files.get(targetsPath)?.content ?? "",
    taskId,
    hookServerValidation.expectedHookServerUrl,
  );

  const hasPlugin = hasKanvibePlugin(pluginContent);
  const hasStatusEndpoint = pluginContent.includes("/api/hooks/status");
  const hasEventMappings = OPEN_CODE_EVENT_FRAGMENTS.every((fragment) => pluginContent.includes(fragment));
  const hasStatusJsonPersistence = hasOpenCodeStatusJsonPersistence(pluginContent);
  const hasProjectColorPersistence = hasOpenCodeProjectColorPersistence(pluginContent);
  const hasTargetFanout = hasOpenCodeTargetFanout(pluginContent);
  const hasParallelTargetFanout = hasOpenCodeParallelTargetFanout(pluginContent);
  const hasMainSessionGuard = pluginContent.includes("isMainSession(message)")
    && pluginContent.includes("isMainSession(event.properties)");
  const hasDuplicateProgressGuard = pluginContent.includes("lastUserMessageBySession")
    && pluginContent.includes("buildMessageSignature")
    && pluginContent.includes("dedupeMessage: true");

  const installed = pluginFile.exists
    && hasPlugin
    && hasTaskIdBinding
    && targetRegistration.hasRegisteredHookTarget
    && hasStatusEndpoint
    && hasEventMappings
    && hasStatusJsonPersistence
    && hasProjectColorPersistence
    && hasTargetFanout
    && hasParallelTargetFanout
    && hasMainSessionGuard
    && hasDuplicateProgressGuard
    && pluginRegistration.hasRegisteredPlugin;

  return {
    installed,
    hasPlugin,
    hasTaskIdBinding,
    hasStatusEndpoint,
    hasEventMappings,
    hasStatusJsonPersistence,
    hasProjectColorPersistence,
    hasTargetFanout,
    hasParallelTargetFanout,
    hasMainSessionGuard,
    hasDuplicateProgressGuard,
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    targetPath: repoPath,
    pluginPath,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
    ...pluginRegistration,
    ...targetRegistration,
  };
}

const OPEN_CODE_EVENT_FRAGMENTS = [
  "progress",
  "pending",
  "review",
  "done",
  "message.updated",
  "question.asked",
  "question.replied",
  "session.idle",
  "session.deleted",
];

/** 플러그인 파일에 kanvibe 관련 코드가 포함되어 있는지 확인한다 */
function hasKanvibePlugin(pluginContent: string): boolean {
  return pluginContent.includes("KanvibePlugin") && pluginContent.includes("/api/hooks/status");
}

function hasOpenCodeStatusJsonPersistence(pluginContent: string): boolean {
  return pluginContent.includes("status.json")
    && pluginContent.includes("KANVIBE_STATE_DIR_EXCLUDE_PATTERN")
    && pluginContent.includes("--git-common-dir")
    && pluginContent.includes("includes(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)")
    && pluginContent.includes("schemaVersion: 1")
    && pluginContent.includes("updatedAt: new Date().toISOString()");
}

function hasOpenCodeProjectColorPersistence(pluginContent: string): boolean {
  return pluginContent.includes("readKanvibeProjectColor")
    && pluginContent.includes("KANVIBE_PROJECT_COLOR_PATTERN")
    && pluginContent.includes("projectColor ? { projectColor } : {}");
}

function hasOpenCodeTargetFanout(pluginContent: string): boolean {
  return pluginContent.includes("targets.json")
    && pluginContent.includes("KANVIBE_TARGETS_FILE")
    && pluginContent.includes("readKanvibeTargets")
    && pluginContent.includes("fanoutKanvibeStatus")
    && pluginContent.includes("postKanvibeStatus")
    && pluginContent.includes("taskId: target.taskId")
    && pluginContent.includes("/api/hooks/status");
}

/** fan-out이 순차 await 루프가 아니라 Promise.all 병렬 호출인지 확인한다 */
function hasOpenCodeParallelTargetFanout(pluginContent: string): boolean {
  return pluginContent.includes("await Promise.all(readKanvibeTargets().map(");
}

function extractPluginTaskId(pluginContent: string): string | null {
  const match = pluginContent.match(/const TASK_ID = ("(?:\\.|[^"\\])*");/);
  if (!match) return null;

  try {
    return JSON.parse(match[1]) as string;
  } catch {
    return null;
  }
}

async function resolveOpenCodePluginRegistration(
  repoPath: string,
  pluginPath: string,
  sshHost?: string | null,
): Promise<{ hasRegisteredPlugin: boolean; hasDuplicateKanvibePlugins: boolean; registeredPluginUrls: string[] }> {
  /** 원격 repo는 OpenCode 설정 레지스트리를 조회할 수 없으므로 등록 여부를 검증하지 않는다 */
  if (sshHost) {
    return { hasRegisteredPlugin: true, hasDuplicateKanvibePlugins: false, registeredPluginUrls: [] };
  }

  const expectedPluginUrl = pathToFileURL(pluginPath).href;
  const registeredPluginUrls = await getOpenCodeRegisteredKanvibePluginUrls(repoPath);

  return {
    hasRegisteredPlugin: registeredPluginUrls.some((value) => value === expectedPluginUrl),
    hasDuplicateKanvibePlugins: registeredPluginUrls.length > 1,
    registeredPluginUrls,
  };
}

function getOpenCodePluginPath(repoPath: string, sshHost?: string | null): string {
  const pathModule = resolvePathModule(sshHost);
  return pathModule.join(repoPath, CONFIG_DIR_NAME, PLUGIN_DIR_NAME, PLUGIN_FILE_NAME);
}
