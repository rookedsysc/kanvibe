import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { pathExists, readTextFile } from "@/lib/hostFileAccess";
import { extractShellHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import { buildShellTaskIdResolver, extractShellTaskId } from "@/lib/hookTaskBinding";

/**
 * Codex CLI 최신 hooks 설정은 `.codex/config.toml`의 feature flag와
 * `.codex/hooks.json` 조합을 사용한다.
 */

interface CodexHooksFile {
  hooks?: Record<string, unknown[]>;
  [key: string]: unknown;
}

interface HookEntry {
  hooks: { type: string; command: string; timeout: number }[];
}

interface MatcherHookEntry extends HookEntry {
  matcher: string;
}

export const CONFIG_FILE_NAME = "config.toml";
export const HOOKS_FILE_NAME = "hooks.json";
export const PROMPT_HOOK_SCRIPT_NAME = "kanvibe-prompt-hook.sh";
export const PERMISSION_HOOK_SCRIPT_NAME = "kanvibe-permission-hook.sh";
export const PRE_TOOL_HOOK_SCRIPT_NAME = "kanvibe-pre-tool-hook.sh";
export const STOP_HOOK_SCRIPT_NAME = "kanvibe-stop-hook.sh";

const CODEX_PROMPT_COMMAND = `bash "$(git rev-parse --show-toplevel)/.codex/hooks/${PROMPT_HOOK_SCRIPT_NAME}"`;
const CODEX_PERMISSION_COMMAND = `bash "$(git rev-parse --show-toplevel)/.codex/hooks/${PERMISSION_HOOK_SCRIPT_NAME}"`;
const CODEX_PRE_TOOL_COMMAND = `bash "$(git rev-parse --show-toplevel)/.codex/hooks/${PRE_TOOL_HOOK_SCRIPT_NAME}"`;
const CODEX_STOP_COMMAND = `bash "$(git rev-parse --show-toplevel)/.codex/hooks/${STOP_HOOK_SCRIPT_NAME}"`;

function generateStatusHookScript(
  eventLabel: string,
  description: string,
  status: "progress" | "pending" | "review",
  kanvibeUrl: string,
  taskId: string,
): string {
  return `#!/bin/bash

# KanVibe Codex Hook: ${eventLabel}
# ${description}

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"${status}\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string): string {
  return generateStatusHookScript(
    "UserPromptSubmit",
    "사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.",
    "progress",
    kanvibeUrl,
    taskId,
  );
}

/** PermissionRequest(Bash) hook bash 스크립트를 생성한다 */
export function generatePermissionHookScript(kanvibeUrl: string, taskId: string): string {
  return generateStatusHookScript(
    "PermissionRequest(Bash)",
    "Codex가 Bash 실행 승인을 요청하면 현재 task를 PENDING으로 변경한다.",
    "pending",
    kanvibeUrl,
    taskId,
  );
}

/** PreToolUse(Bash) hook bash 스크립트를 생성한다 */
export function generatePreToolHookScript(kanvibeUrl: string, taskId: string): string {
  return generateStatusHookScript(
    "PreToolUse(Bash)",
    "Codex가 Bash 실행을 재개하면 현재 task를 PROGRESS로 변경한다.",
    "progress",
    kanvibeUrl,
    taskId,
  );
}

/** Stop hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string): string {
  return generateStatusHookScript(
    "Stop",
    "Codex 응답이 완료되면 현재 task를 REVIEW로 변경한다.",
    "review",
    kanvibeUrl,
    taskId,
  );
}

function hasTaskIdPayloadBinding(content: string, taskId?: string): boolean {
  const boundTaskId = extractShellTaskId(content);
  const hasTaskIdPayload = content.includes("taskId") && content.includes("${TASK_ID}");
  if (!hasTaskIdPayload) return false;

  if (!taskId) {
    return boundTaskId !== null;
  }

  return boundTaskId === taskId;
}

function parseHooksJson(content: string): CodexHooksFile {
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content) as CodexHooksFile;
  } catch {
    return {};
  }
}

function hasCommandHook(hookEntries: unknown[], command: string): boolean {
  if (!Array.isArray(hookEntries)) return false;
  return hookEntries.some((entry) => {
    const typed = entry as HookEntry;
    return typed.hooks?.some((hook) => hook.type === "command" && hook.command === command);
  });
}

function hasMatcherCommandHook(hookEntries: unknown[], matcher: string, command: string): boolean {
  if (!Array.isArray(hookEntries)) return false;
  return hookEntries.some((entry) => {
    const typed = entry as MatcherHookEntry;
    return typed.matcher === matcher && typed.hooks?.some((hook) => hook.type === "command" && hook.command === command);
  });
}

function referencesScriptName(entry: unknown, scriptName: string): boolean {
  return JSON.stringify(entry).includes(scriptName);
}

function upsertHookEntries<T>(hookEntries: unknown[] | undefined, scriptName: string, nextEntry: T): T[] {
  const preservedEntries = Array.isArray(hookEntries)
    ? hookEntries.filter((entry) => !referencesScriptName(entry, scriptName)) as T[]
    : [];
  preservedEntries.push(nextEntry);
  return preservedEntries;
}

function isSectionHeader(line: string): boolean {
  return /^\s*\[[^\]]+\]\s*$/.test(line);
}

function findFeaturesSection(lines: string[]) {
  const start = lines.findIndex((line) => /^\s*\[features\]\s*$/.test(line));
  if (start === -1) {
    return null;
  }

  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (isSectionHeader(lines[index])) {
      end = index;
      break;
    }
  }

  return { start, end };
}

function stripLegacyKanvibeNotify(configContent: string): string {
  return configContent.replace(/^notify\s*=\s*\["\.codex\/hooks\/kanvibe-notify-hook\.sh"\]\s*\n?/gm, "");
}

export function upsertCodexConfigToml(configContent: string): string {
  const normalized = stripLegacyKanvibeNotify(configContent).replace(/\r\n/g, "\n").trimEnd();
  const lines = normalized.length > 0 ? normalized.split("\n") : [];
  const featuresSection = findFeaturesSection(lines);

  if (!featuresSection) {
    const prefix = normalized.length > 0 ? `${normalized}\n\n` : "";
    return `${prefix}[features]\ncodex_hooks = true\n`;
  }

  const flagIndex = lines.findIndex(
    (line, index) => index > featuresSection.start
      && index < featuresSection.end
      && /^\s*codex_hooks\s*=/.test(line),
  );

  if (flagIndex !== -1) {
    lines[flagIndex] = "codex_hooks = true";
  } else {
    lines.splice(featuresSection.end, 0, "codex_hooks = true");
  }

  return `${lines.join("\n").trimEnd()}\n`;
}

function hasCodexFeatureFlag(configContent: string): boolean {
  const normalized = configContent.replace(/\r\n/g, "\n");
  const lines = normalized.length > 0 ? normalized.split("\n") : [];
  const featuresSection = findFeaturesSection(lines);
  if (!featuresSection) {
    return false;
  }

  return lines.some(
    (line, index) => index > featuresSection.start
      && index < featuresSection.end
      && /^\s*codex_hooks\s*=\s*true\s*$/.test(line),
  );
}

export function upsertCodexHooksJson(hooksContent: string): string {
  const settings = parseHooksJson(hooksContent);
  if (!settings.hooks) {
    settings.hooks = {};
  }

  const hooks = settings.hooks as Record<string, unknown[]>;

  hooks.UserPromptSubmit = upsertHookEntries<HookEntry>(hooks.UserPromptSubmit, PROMPT_HOOK_SCRIPT_NAME, {
    hooks: [
      {
        type: "command",
        command: CODEX_PROMPT_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.PermissionRequest = upsertHookEntries<MatcherHookEntry>(hooks.PermissionRequest, PERMISSION_HOOK_SCRIPT_NAME, {
    matcher: "Bash",
    hooks: [
      {
        type: "command",
        command: CODEX_PERMISSION_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.PreToolUse = upsertHookEntries<MatcherHookEntry>(hooks.PreToolUse, PRE_TOOL_HOOK_SCRIPT_NAME, {
    matcher: "Bash",
    hooks: [
      {
        type: "command",
        command: CODEX_PRE_TOOL_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.Stop = upsertHookEntries<HookEntry>(hooks.Stop, STOP_HOOK_SCRIPT_NAME, {
    hooks: [
      {
        type: "command",
        command: CODEX_STOP_COMMAND,
        timeout: 10,
      },
    ],
  });

  return `${JSON.stringify(settings, null, 2)}\n`;
}

/**
 * 지정된 repo에 Codex CLI hooks를 설정한다.
 * 기존 config.toml / hooks.json이 있으면 KanVibe 관련 항목만 갱신하고 나머지는 보존한다.
 */
export async function setupCodexHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
): Promise<void> {
  const codexDir = path.join(repoPath, ".codex");
  const hooksDir = path.join(codexDir, "hooks");
  const configPath = path.join(codexDir, CONFIG_FILE_NAME);
  const hooksFilePath = path.join(codexDir, HOOKS_FILE_NAME);

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME);
  const permissionScriptPath = path.join(hooksDir, PERMISSION_HOOK_SCRIPT_NAME);
  const preToolScriptPath = path.join(hooksDir, PRE_TOOL_HOOK_SCRIPT_NAME);
  const stopScriptPath = path.join(hooksDir, STOP_HOOK_SCRIPT_NAME);

  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, taskId), "utf-8");
  await writeFile(permissionScriptPath, generatePermissionHookScript(kanvibeUrl, taskId), "utf-8");
  await writeFile(preToolScriptPath, generatePreToolHookScript(kanvibeUrl, taskId), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, taskId), "utf-8");
  await chmod(promptScriptPath, 0o755);
  await chmod(permissionScriptPath, 0o755);
  await chmod(preToolScriptPath, 0o755);
  await chmod(stopScriptPath, 0o755);

  const configContent = await readTextFile(configPath);
  await writeFile(configPath, upsertCodexConfigToml(configContent), "utf-8");

  const hooksContent = await readTextFile(hooksFilePath);
  await writeFile(hooksFilePath, upsertCodexHooksJson(hooksContent), "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface CodexHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasPermissionHook: boolean;
  hasPreToolHook: boolean;
  hasStopHook: boolean;
  hasHooksFile: boolean;
  hasHookEntries: boolean;
  hasConfigEntry: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusMappings?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}

/** 지정된 repo의 Codex CLI hooks 설치 상태를 확인한다 */
export async function getCodexHooksStatus(repoPath: string, taskId?: string, sshHost?: string | null): Promise<CodexHooksStatus> {
  const pathModule = sshHost ? path.posix : path;
  const codexDir = pathModule.join(repoPath, ".codex");
  const hooksDir = pathModule.join(codexDir, "hooks");
  const configPath = pathModule.join(codexDir, CONFIG_FILE_NAME);
  const hooksFilePath = pathModule.join(codexDir, HOOKS_FILE_NAME);
  const promptScriptPath = pathModule.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME);
  const permissionScriptPath = pathModule.join(hooksDir, PERMISSION_HOOK_SCRIPT_NAME);
  const preToolScriptPath = pathModule.join(hooksDir, PRE_TOOL_HOOK_SCRIPT_NAME);
  const stopScriptPath = pathModule.join(hooksDir, STOP_HOOK_SCRIPT_NAME);

  const [
    promptScriptExists,
    permissionScriptExists,
    preToolScriptExists,
    stopScriptExists,
    hooksFileExists,
  ] = await Promise.all([
    pathExists(promptScriptPath, sshHost),
    pathExists(permissionScriptPath, sshHost),
    pathExists(preToolScriptPath, sshHost),
    pathExists(stopScriptPath, sshHost),
    pathExists(hooksFilePath, sshHost),
  ]);

  const [
    promptContent,
    permissionContent,
    preToolContent,
    stopContent,
    hooksContent,
    configContent,
  ] = await Promise.all([
    promptScriptExists ? readTextFile(promptScriptPath, sshHost) : Promise.resolve(""),
    permissionScriptExists ? readTextFile(permissionScriptPath, sshHost) : Promise.resolve(""),
    preToolScriptExists ? readTextFile(preToolScriptPath, sshHost) : Promise.resolve(""),
    stopScriptExists ? readTextFile(stopScriptPath, sshHost) : Promise.resolve(""),
    hooksFileExists ? readTextFile(hooksFilePath, sshHost) : Promise.resolve(""),
    readTextFile(configPath, sshHost),
  ]);

  const hookScripts = [promptContent, permissionContent, preToolContent, stopContent];
  const boundTaskId = hookScripts.map((content) => extractShellTaskId(content)).find((value) => value !== null) ?? null;
  const hasTaskIdBinding = hookScripts.every((content) => hasTaskIdPayloadBinding(content, taskId));
  const hasStatusMappings = promptContent.includes('\\\"status\\\": \\\"progress\\\"')
    && permissionContent.includes('\\\"status\\\": \\\"pending\\\"')
    && preToolContent.includes('\\\"status\\\": \\\"progress\\\"')
    && stopContent.includes('\\\"status\\\": \\\"review\\\"');
  const hookServerValidation = await validateHookServerConfiguration(
    hookScripts.map((content) => extractShellHookServerUrl(content)),
    Boolean(taskId),
    sshHost,
  );

  const settings = parseHooksJson(hooksContent);
  const hooks = settings.hooks || {};
  const hasHookEntries = hasCommandHook(hooks.UserPromptSubmit || [], CODEX_PROMPT_COMMAND)
    && hasMatcherCommandHook(hooks.PermissionRequest || [], "Bash", CODEX_PERMISSION_COMMAND)
    && hasMatcherCommandHook(hooks.PreToolUse || [], "Bash", CODEX_PRE_TOOL_COMMAND)
    && hasCommandHook(hooks.Stop || [], CODEX_STOP_COMMAND);
  const hasConfigEntry = hasCodexFeatureFlag(configContent);

  const installed = promptScriptExists
    && permissionScriptExists
    && preToolScriptExists
    && stopScriptExists
    && hooksFileExists
    && hasHookEntries
    && hasConfigEntry
    && hasTaskIdBinding
    && hasStatusMappings
    && hookServerValidation.hasExpectedHookServerUrl;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasPermissionHook: permissionScriptExists,
    hasPreToolHook: preToolScriptExists,
    hasStopHook: stopScriptExists,
    hasHooksFile: hooksFileExists,
    hasHookEntries,
    hasConfigEntry,
    hasTaskIdBinding,
    hasStatusMappings,
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
  };
}
