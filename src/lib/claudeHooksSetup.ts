import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { readTextFile, readTextFiles } from "@/lib/hostFileAccess";
import { extractShellHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import { buildShellTaskIdResolver, extractShellTaskId } from "@/lib/hookTaskBinding";

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: UserPromptSubmit
# 사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"progress\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** Stop hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: Stop
# AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"review\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** PreToolUse(AskUserQuestion) hook bash 스크립트를 생성한다 */
export function generateQuestionHookScript(kanvibeUrl: string, taskId: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: PreToolUse (AskUserQuestion)
# Claude가 사용자에게 질문할 때 현재 task를 PENDING으로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"pending\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

interface ClaudeSettings {
  hooks?: Record<string, unknown[]>;
  [key: string]: unknown;
}

interface HookEntry {
  hooks: { type: string; command: string; timeout: number }[];
}

interface MatcherHookEntry extends HookEntry {
  matcher: string;
}

const CLAUDE_PROMPT_COMMAND = '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh';
const CLAUDE_STOP_COMMAND = '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh';
const CLAUDE_QUESTION_COMMAND = '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh';

function hasTaskIdPayloadBinding(content: string, taskId?: string): boolean {
  const boundTaskId = extractShellTaskId(content);
  const hasTaskIdPayload = content.includes("taskId") && content.includes("${TASK_ID}");
  if (!hasTaskIdPayload) return false;

  if (!taskId) {
    return boundTaskId !== null;
  }

  return boundTaskId === taskId;
}

function hasLegacyBranchPayloadBinding(content: string): boolean {
  return content.includes('BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD')
    && content.includes('PROJECT_NAME="')
    && content.includes('\\\"branchName\\\": \\\"${BRANCH_NAME}\\\"')
    && content.includes('\\\"projectName\\\": \\\"${PROJECT_NAME}\\\"');
}

/** 기존 settings.json을 읽거나 빈 객체를 반환한다 */
async function readSettingsJson(settingsPath: string, sshHost?: string | null): Promise<ClaudeSettings> {
  return parseSettingsJson(await readTextFile(settingsPath, sshHost));
}

function parseSettingsJson(content: string): ClaudeSettings {
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content);
  } catch {
    return {};
  }
}

/** 현재 bucket에 원하는 command hook이 정확히 등록되어 있는지 확인한다 */
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

/**
 * 지정된 repo에 Claude Code hooks를 설정한다.
 * 기존 settings.json이 있으면 kanvibe hooks만 추가하고 나머지는 보존한다.
 */
export async function setupClaudeHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
): Promise<void> {
  const claudeDir = path.join(repoPath, ".claude");
  const hooksDir = path.join(claudeDir, "hooks");
  const settingsPath = path.join(claudeDir, "settings.json");

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = path.join(hooksDir, "kanvibe-stop-hook.sh");
  const questionScriptPath = path.join(hooksDir, "kanvibe-question-hook.sh");

  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, taskId), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, taskId), "utf-8");
  await writeFile(questionScriptPath, generateQuestionHookScript(kanvibeUrl, taskId), "utf-8");
  await chmod(promptScriptPath, 0o755);
  await chmod(stopScriptPath, 0o755);
  await chmod(questionScriptPath, 0o755);

  const settings = await readSettingsJson(settingsPath);
  if (!settings.hooks) {
    settings.hooks = {};
  }

  const hooks = settings.hooks as Record<string, unknown[]>;

  hooks.UserPromptSubmit = upsertHookEntries<HookEntry>(hooks.UserPromptSubmit, "kanvibe-prompt-hook.sh", {
    hooks: [
      {
        type: "command",
        command: CLAUDE_PROMPT_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.PreToolUse = upsertHookEntries<MatcherHookEntry>(hooks.PreToolUse, "kanvibe-question-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [
      {
        type: "command",
        command: CLAUDE_QUESTION_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.PostToolUse = upsertHookEntries<MatcherHookEntry>(hooks.PostToolUse, "kanvibe-prompt-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [
      {
        type: "command",
        command: CLAUDE_PROMPT_COMMAND,
        timeout: 10,
      },
    ],
  });

  hooks.Stop = upsertHookEntries<HookEntry>(hooks.Stop, "kanvibe-stop-hook.sh", {
    hooks: [
      {
        type: "command",
        command: CLAUDE_STOP_COMMAND,
        timeout: 10,
      },
    ],
  });

  await writeFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface ClaudeHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasQuestionHook: boolean;
  hasSettingsEntry: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusMappings?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}

/** 지정된 repo의 Claude Code hooks 설치 상태를 확인한다 */
export async function getClaudeHooksStatus(repoPath: string, taskId?: string, sshHost?: string | null): Promise<ClaudeHooksStatus> {
  const pathModule = sshHost ? path.posix : path;
  const claudeDir = pathModule.join(repoPath, ".claude");
  const hooksDir = pathModule.join(claudeDir, "hooks");
  const settingsPath = pathModule.join(claudeDir, "settings.json");
  const promptScriptPath = pathModule.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = pathModule.join(hooksDir, "kanvibe-stop-hook.sh");
  const questionScriptPath = pathModule.join(hooksDir, "kanvibe-question-hook.sh");

  const files = await readTextFiles([
    promptScriptPath,
    stopScriptPath,
    questionScriptPath,
    settingsPath,
  ], sshHost);
  const promptScript = files.get(promptScriptPath) ?? { exists: false, content: "" };
  const stopScript = files.get(stopScriptPath) ?? { exists: false, content: "" };
  const questionScript = files.get(questionScriptPath) ?? { exists: false, content: "" };
  const settingsFile = files.get(settingsPath) ?? { exists: false, content: "" };
  const promptContent = promptScript.content;
  const stopContent = stopScript.content;
  const questionContent = questionScript.content;

  const scriptContents = [promptContent, stopContent, questionContent];
  const boundTaskIds = scriptContents.map(extractShellTaskId).filter((value): value is string => value !== null);
  const boundTaskId = boundTaskIds.length > 0 && boundTaskIds.every((value) => value === boundTaskIds[0])
    ? boundTaskIds[0]
    : null;
  const hasTaskIdBinding = scriptContents.every((content) => hasTaskIdPayloadBinding(content, taskId));
  const hookServerValidation = await validateHookServerConfiguration(
    scriptContents.map(extractShellHookServerUrl),
    Boolean(taskId),
    sshHost,
  );
  const hasStatusMappings =
    promptContent.includes('\\\"status\\\": \\\"progress\\\"') &&
    stopContent.includes('\\\"status\\\": \\\"review\\\"') &&
    questionContent.includes('\\\"status\\\": \\\"pending\\\"');

  let hasSettingsEntry = false;
  try {
    const settings = parseSettingsJson(settingsFile.content);
    const hooks = settings.hooks as Record<string, unknown[]> | undefined;
    if (hooks) {
      const hasPrompt = hasCommandHook(hooks.UserPromptSubmit || [], CLAUDE_PROMPT_COMMAND);
      const hasStop = hasCommandHook(hooks.Stop || [], CLAUDE_STOP_COMMAND);
      const hasQuestion = hasMatcherCommandHook(hooks.PreToolUse || [], "AskUserQuestion", CLAUDE_QUESTION_COMMAND);
      const hasAnswerResume = hasMatcherCommandHook(hooks.PostToolUse || [], "AskUserQuestion", CLAUDE_PROMPT_COMMAND);
      hasSettingsEntry = hasPrompt && hasStop && hasQuestion && hasAnswerResume;
    }
  } catch {
    /* settings.json 없음 */
  }

  const installed = promptScript.exists
    && stopScript.exists
    && questionScript.exists
    && hasSettingsEntry
    && hasTaskIdBinding
    && hasStatusMappings
    && hookServerValidation.hasExpectedHookServerUrl;

  return {
    installed,
    hasPromptHook: promptScript.exists,
    hasStopHook: stopScript.exists,
    hasQuestionHook: questionScript.exists,
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
