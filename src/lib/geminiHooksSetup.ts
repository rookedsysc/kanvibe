import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { buildCurlAuthHeader } from "@/lib/hookAuth";
import { pathExists, readTextFile } from "@/lib/hostFileAccess";
import { extractShellHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import { KANVIBE_TASK_ID_RELATIVE_PATH, buildShellTaskIdResolver, readHookTaskIdFile, writeHookTaskIdFile } from "@/lib/hookTaskBinding";

/**
 * Gemini CLI hooks는 stdout에 반드시 JSON만 출력해야 한다.
 * curl 결과는 /dev/null로 보내고, 마지막에 '{}' JSON을 출력한다.
 */

/** BeforeAgent hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Gemini CLI Hook: BeforeAgent
# 사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.
# Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"progress\\\"}" \\
  > /dev/null 2>&1

echo '{}'
exit 0
`;
}

/** AfterAgent hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Gemini CLI Hook: AfterAgent
# AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.
# Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"review\\\"}" \\
  > /dev/null 2>&1

echo '{}'
exit 0
`;
}

interface GeminiSettings {
  hooks?: Record<string, unknown[]>;
  [key: string]: unknown;
}

interface GeminiHookConfig {
  name?: string;
  type: string;
  command: string;
  timeout: number;
  description?: string;
}

interface GeminiHookEntry {
  matcher: string;
  hooks: GeminiHookConfig[];
}

function hasTaskIdPayloadBinding(content: string, taskId?: string, boundTaskId?: string | null): boolean {
  const hasDynamicTaskIdResolver = content.includes(`TASK_ID_FILE="${KANVIBE_TASK_ID_RELATIVE_PATH}"`);
  const hasTaskIdPayload = content.includes("taskId") && content.includes("${TASK_ID}");
  if (!hasTaskIdPayload) return false;

  if (!taskId) {
    return hasDynamicTaskIdResolver || content.includes("TASK_ID=");
  }

  if (hasDynamicTaskIdResolver) {
    return boundTaskId === taskId;
  }

  return content.includes(`TASK_ID="${taskId}"`);
}

function hasLegacyBranchPayloadBinding(content: string): boolean {
  return content.includes('BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD')
    && content.includes('PROJECT_NAME="')
    && content.includes('\\\"branchName\\\": \\\"${BRANCH_NAME}\\\"')
    && content.includes('\\\"projectName\\\": \\\"${PROJECT_NAME}\\\"');
}

/** 기존 settings.json을 읽거나 빈 객체를 반환한다 */
async function readSettingsJson(settingsPath: string, sshHost?: string | null): Promise<GeminiSettings> {
  const content = await readTextFile(settingsPath, sshHost);
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content);
  } catch {
    return {};
  }
}

/** kanvibe hook이 이미 등록되어 있는지 확인한다 */
function hasKanvibeHook(hookEntries: unknown[], scriptName: string): boolean {
  if (!Array.isArray(hookEntries)) return false;
  return hookEntries.some((entry) => {
    const typed = entry as GeminiHookEntry;
    return typed.hooks?.some((h) => h.command?.includes(scriptName));
  });
}

/**
 * 지정된 repo에 Gemini CLI hooks를 설정한다.
 * 기존 settings.json이 있으면 kanvibe hooks만 추가하고 나머지는 보존한다.
 */
export async function setupGeminiHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  authToken?: string,
): Promise<void> {
  const geminiDir = path.join(repoPath, ".gemini");
  const hooksDir = path.join(geminiDir, "hooks");
  const settingsPath = path.join(geminiDir, "settings.json");

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = path.join(hooksDir, "kanvibe-stop-hook.sh");

  await writeHookTaskIdFile(repoPath, taskId);
  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await chmod(promptScriptPath, 0o755);
  await chmod(stopScriptPath, 0o755);

  const settings = await readSettingsJson(settingsPath);
  if (!settings.hooks) {
    settings.hooks = {};
  }

  const hooks = settings.hooks as Record<string, unknown[]>;

  if (!hooks.BeforeAgent) {
    hooks.BeforeAgent = [];
  }
  if (!hasKanvibeHook(hooks.BeforeAgent, "kanvibe-prompt-hook.sh")) {
    (hooks.BeforeAgent as GeminiHookEntry[]).push({
      matcher: "*",
      hooks: [
        {
          type: "command",
          command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh',
          timeout: 10000,
        },
      ],
    });
  }

  if (!hooks.AfterAgent) {
    hooks.AfterAgent = [];
  }
  if (!hasKanvibeHook(hooks.AfterAgent, "kanvibe-stop-hook.sh")) {
    (hooks.AfterAgent as GeminiHookEntry[]).push({
      matcher: "*",
      hooks: [
        {
          type: "command",
          command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh',
          timeout: 10000,
        },
      ],
    });
  }

  await writeFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface GeminiHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasSettingsEntry: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusMappings?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}

/** 지정된 repo의 Gemini CLI hooks 설치 상태를 확인한다 */
export async function getGeminiHooksStatus(repoPath: string, taskId?: string, sshHost?: string | null): Promise<GeminiHooksStatus> {
  const pathModule = sshHost ? path.posix : path;
  const geminiDir = pathModule.join(repoPath, ".gemini");
  const hooksDir = pathModule.join(geminiDir, "hooks");
  const settingsPath = pathModule.join(geminiDir, "settings.json");
  const promptScriptPath = pathModule.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = pathModule.join(hooksDir, "kanvibe-stop-hook.sh");

  const promptScriptExists = await pathExists(promptScriptPath, sshHost);
  const stopScriptExists = await pathExists(stopScriptPath, sshHost);

  const [promptContent, stopContent] = await Promise.all([
    promptScriptExists ? readTextFile(promptScriptPath, sshHost) : Promise.resolve(""),
    stopScriptExists ? readTextFile(stopScriptPath, sshHost) : Promise.resolve(""),
  ]);

  const scriptContents = [promptContent, stopContent];
  const boundTaskId = await readHookTaskIdFile(repoPath, sshHost);
  const hasTaskIdBinding = scriptContents.every((content) => hasTaskIdPayloadBinding(content, taskId, boundTaskId));
  const hookServerValidation = await validateHookServerConfiguration(
    scriptContents.map(extractShellHookServerUrl),
    Boolean(taskId),
    sshHost,
  );
  const hasStatusMappings =
    promptContent.includes('\\\"status\\\": \\\"progress\\\"') &&
    stopContent.includes('\\\"status\\\": \\\"review\\\"');

  let hasSettingsEntry = false;
  try {
    const settings = await readSettingsJson(settingsPath, sshHost);
    const hooks = settings.hooks as Record<string, unknown[]> | undefined;
    if (hooks) {
      const hasPrompt = hasKanvibeHook(hooks.BeforeAgent || [], "kanvibe-prompt-hook.sh");
      const hasStop = hasKanvibeHook(hooks.AfterAgent || [], "kanvibe-stop-hook.sh");
      hasSettingsEntry = hasPrompt && hasStop;
    }
  } catch {
    /* settings.json 없음 */
  }

  const installed = promptScriptExists
    && stopScriptExists
    && hasSettingsEntry
    && hasTaskIdBinding
    && hasStatusMappings
    && hookServerValidation.hasExpectedHookServerUrl
    && hookServerValidation.hasReachableHookServer;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasStopHook: stopScriptExists,
    hasSettingsEntry,
    hasTaskIdBinding,
    hasStatusMappings,
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
  };
}
