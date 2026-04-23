import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { buildCurlAuthHeader } from "@/lib/hookAuth";
import { pathExists, readTextFile } from "@/lib/hostFileAccess";
import { extractShellHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import { KANVIBE_TASK_ID_RELATIVE_PATH, buildShellTaskIdResolver, readHookTaskIdFile, writeHookTaskIdFile } from "@/lib/hookTaskBinding";

/**
 * Codex hooks는 `.codex/hooks.json`과 `[features].codex_hooks`를 사용한다.
 * UserPromptSubmit은 PROGRESS, Stop은 REVIEW 상태로 매핑한다.
 */

export const PROMPT_HOOK_SCRIPT_NAME = "kanvibe-prompt-hook.sh";
export const STOP_HOOK_SCRIPT_NAME = "kanvibe-stop-hook.sh";
export const HOOKS_FILE_NAME = "hooks.json";
export const CONFIG_FILE_NAME = "config.toml";

const LEGACY_NOTIFY_SCRIPT_NAME = "kanvibe-notify-hook.sh";
const CODEX_PROMPT_COMMAND = `/usr/bin/env bash "$(git rev-parse --show-toplevel)/.codex/hooks/${PROMPT_HOOK_SCRIPT_NAME}"`;
const CODEX_STOP_COMMAND = `/usr/bin/env bash "$(git rev-parse --show-toplevel)/.codex/hooks/${STOP_HOOK_SCRIPT_NAME}"`;

interface CodexHooksFile {
  hooks?: Record<string, unknown[]>;
  [key: string]: unknown;
}

interface CodexCommandHook {
  type: string;
  command: string;
  timeout: number;
  statusMessage?: string;
}

interface CodexHookEntry {
  hooks: CodexCommandHook[];
}

function generateStatusHookScript(
  hookName: string,
  status: "progress" | "review",
  kanvibeUrl: string,
  taskId: string,
  authToken?: string,
): string {
  return `#!/bin/bash

# KanVibe Codex Hook: ${hookName}
# Codex hook 이벤트에 맞춰 현재 task를 ${status.toUpperCase()}로 변경한다.
# Codex command hooks는 stdin으로 JSON payload를 받지만 이 스크립트는 상태 갱신만 수행한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"${status}\\\"}" \\
  > /dev/null 2>&1

printf '{"continue":true}\\n'
exit 0
`;
}

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return generateStatusHookScript("UserPromptSubmit", "progress", kanvibeUrl, taskId, authToken);
}

/** Stop hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return generateStatusHookScript("Stop", "review", kanvibeUrl, taskId, authToken);
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

function parseCodexHooksJson(content: string): CodexHooksFile {
  if (!content.trim()) {
    return {};
  }

  try {
    return JSON.parse(content) as CodexHooksFile;
  } catch {
    return {};
  }
}

function readHookEntries(config: CodexHooksFile, eventName: string): unknown[] {
  const hooks = config.hooks;
  if (!hooks || typeof hooks !== "object" || Array.isArray(hooks)) {
    return [];
  }

  const entries = hooks[eventName];
  return Array.isArray(entries) ? entries : [];
}

function hasCodexCommandHook(hookEntries: unknown[], command: string): boolean {
  return hookEntries.some((entry) => {
    const typed = entry as CodexHookEntry;
    return typed.hooks?.some((hook) => hook.type === "command" && hook.command === command);
  });
}

function referencesScriptName(entry: unknown, scriptName: string): boolean {
  return JSON.stringify(entry).includes(scriptName);
}

function upsertHookEntries(
  hookEntries: unknown[] | undefined,
  scriptName: string,
  nextEntry: CodexHookEntry,
): CodexHookEntry[] {
  const preservedEntries = Array.isArray(hookEntries)
    ? hookEntries.filter((entry) => !referencesScriptName(entry, scriptName)) as CodexHookEntry[]
    : [];
  preservedEntries.push(nextEntry);
  return preservedEntries;
}

/** 기존 hooks.json 내용에 KanVibe Codex hooks를 멱등적으로 반영한다 */
export function buildCodexHooksJsonContent(hooksJsonContent: string): string {
  const config = parseCodexHooksJson(hooksJsonContent);
  if (!config.hooks || typeof config.hooks !== "object" || Array.isArray(config.hooks)) {
    config.hooks = {};
  }

  const hooks = config.hooks as Record<string, unknown[]>;

  hooks.UserPromptSubmit = upsertHookEntries(hooks.UserPromptSubmit, PROMPT_HOOK_SCRIPT_NAME, {
    hooks: [
      {
        type: "command",
        command: CODEX_PROMPT_COMMAND,
        timeout: 10,
        statusMessage: "Updating KanVibe task status",
      },
    ],
  });

  hooks.Stop = upsertHookEntries(hooks.Stop, STOP_HOOK_SCRIPT_NAME, {
    hooks: [
      {
        type: "command",
        command: CODEX_STOP_COMMAND,
        timeout: 10,
        statusMessage: "Updating KanVibe task status",
      },
    ],
  });

  return JSON.stringify(config, null, 2) + "\n";
}

function removeLegacyNotifyConfig(configContent: string): string {
  return configContent
    .split(/\r?\n/)
    .filter((line) => !new RegExp(`^\\s*notify\\s*=\\s*\\[\\s*["']\\.codex/hooks/${LEGACY_NOTIFY_SCRIPT_NAME}["']\\s*\\]\\s*$`).test(line))
    .join("\n");
}

function findTomlSectionRange(lines: string[], sectionName: string) {
  const start = lines.findIndex((line) => new RegExp(`^\\s*\\[${sectionName}\\]\\s*$`).test(line));
  if (start === -1) {
    return null;
  }

  const nextSection = lines.findIndex((line, index) => index > start && /^\s*\[[^\]]+\]\s*$/.test(line));
  return {
    start,
    end: nextSection === -1 ? lines.length : nextSection,
  };
}

function upsertCodexHooksFeatureFlag(configContent: string): string {
  const trimmedContent = configContent.trimEnd();
  if (!trimmedContent) {
    return "[features]\ncodex_hooks = true\n";
  }

  const lines = trimmedContent.split(/\r?\n/);
  const featuresRange = findTomlSectionRange(lines, "features");
  if (!featuresRange) {
    return `${trimmedContent}\n\n[features]\ncodex_hooks = true\n`;
  }

  const flagIndex = lines.findIndex((line, index) => (
    index > featuresRange.start &&
    index < featuresRange.end &&
    /^\s*codex_hooks\s*=/.test(line)
  ));

  if (flagIndex === -1) {
    lines.splice(featuresRange.start + 1, 0, "codex_hooks = true");
  } else {
    lines[flagIndex] = "codex_hooks = true";
  }

  return lines.join("\n").trimEnd() + "\n";
}

/** 기존 config.toml 내용에 Codex hooks feature flag를 반영하고 구형 KanVibe notify 설정은 제거한다 */
export function buildCodexConfigToml(configContent: string): string {
  return upsertCodexHooksFeatureFlag(removeLegacyNotifyConfig(configContent));
}

function hasCodexHooksFeatureFlag(configContent: string): boolean {
  const lines = configContent.split(/\r?\n/);
  const featuresRange = findTomlSectionRange(lines, "features");
  if (!featuresRange) {
    return false;
  }

  return lines.some((line, index) => (
    index > featuresRange.start &&
    index < featuresRange.end &&
    /^\s*codex_hooks\s*=\s*true\s*$/.test(line)
  ));
}

/** 기존 config.toml 내용을 읽거나 빈 문자열을 반환한다 */
async function readConfigToml(configPath: string, sshHost?: string | null): Promise<string> {
  return readTextFile(configPath, sshHost);
}

/** 지정된 repo에 Codex hooks를 설정한다 */
export async function setupCodexHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  authToken?: string,
): Promise<void> {
  const codexDir = path.join(repoPath, ".codex");
  const hooksDir = path.join(codexDir, "hooks");
  const configPath = path.join(codexDir, CONFIG_FILE_NAME);
  const hooksJsonPath = path.join(codexDir, HOOKS_FILE_NAME);

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME);
  const stopScriptPath = path.join(hooksDir, STOP_HOOK_SCRIPT_NAME);
  await writeHookTaskIdFile(repoPath, taskId);
  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await chmod(promptScriptPath, 0o755);
  await chmod(stopScriptPath, 0o755);

  const hooksJsonContent = await readTextFile(hooksJsonPath);
  await writeFile(hooksJsonPath, buildCodexHooksJsonContent(hooksJsonContent), "utf-8");

  const configContent = await readConfigToml(configPath);
  await writeFile(configPath, buildCodexConfigToml(configContent), "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface CodexHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasHooksJsonEntry: boolean;
  hasFeatureFlag: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusMappings?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}

/** 지정된 repo의 Codex hooks 설치 상태를 확인한다 */
export async function getCodexHooksStatus(repoPath: string, taskId?: string, sshHost?: string | null): Promise<CodexHooksStatus> {
  const pathModule = sshHost ? path.posix : path;
  const codexDir = pathModule.join(repoPath, ".codex");
  const hooksDir = pathModule.join(codexDir, "hooks");
  const configPath = pathModule.join(codexDir, CONFIG_FILE_NAME);
  const hooksJsonPath = pathModule.join(codexDir, HOOKS_FILE_NAME);
  const promptScriptPath = pathModule.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME);
  const stopScriptPath = pathModule.join(hooksDir, STOP_HOOK_SCRIPT_NAME);

  const promptScriptExists = await pathExists(promptScriptPath, sshHost);
  const stopScriptExists = await pathExists(stopScriptPath, sshHost);

  const [promptContent, stopContent] = await Promise.all([
    promptScriptExists ? readTextFile(promptScriptPath, sshHost) : Promise.resolve(""),
    stopScriptExists ? readTextFile(stopScriptPath, sshHost) : Promise.resolve(""),
  ]);

  const scriptContents = [promptContent, stopContent];
  const boundTaskId = await readHookTaskIdFile(repoPath, sshHost);
  const hasTaskIdBinding = scriptContents.every((content) => hasTaskIdPayloadBinding(content, taskId, boundTaskId));
  const hasStatusMappings =
    promptContent.includes('\\\"status\\\": \\\"progress\\\"') &&
    stopContent.includes('\\\"status\\\": \\\"review\\\"');
  const hookServerValidation = await validateHookServerConfiguration(
    scriptContents.map(extractShellHookServerUrl),
    Boolean(taskId),
    sshHost,
  );

  let hasHooksJsonEntry = false;
  let hasFeatureFlag = false;
  try {
    const hooksJson = parseCodexHooksJson(await readTextFile(hooksJsonPath, sshHost));
    hasHooksJsonEntry =
      hasCodexCommandHook(readHookEntries(hooksJson, "UserPromptSubmit"), CODEX_PROMPT_COMMAND) &&
      hasCodexCommandHook(readHookEntries(hooksJson, "Stop"), CODEX_STOP_COMMAND);
  } catch {
    /* hooks.json 없음 */
  }

  try {
    hasFeatureFlag = hasCodexHooksFeatureFlag(await readConfigToml(configPath, sshHost));
  } catch {
    /* config.toml 없음 */
  }

  const installed = promptScriptExists
    && stopScriptExists
    && hasHooksJsonEntry
    && hasFeatureFlag
    && hasTaskIdBinding
    && hasStatusMappings
    && hookServerValidation.hasExpectedHookServerUrl
    && hookServerValidation.hasReachableHookServer;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasStopHook: stopScriptExists,
    hasHooksJsonEntry,
    hasFeatureFlag,
    hasTaskIdBinding,
    hasStatusMappings,
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
  };
}
