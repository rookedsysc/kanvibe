import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { buildCurlAuthHeader } from "@/lib/hookAuth";
import { pathExists, readTextFile } from "@/lib/hostFileAccess";
import { KANVIBE_TASK_ID_RELATIVE_PATH, buildShellTaskIdResolver, readHookTaskIdFile, writeHookTaskIdFile } from "@/lib/hookTaskBinding";

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: UserPromptSubmit
# 사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"progress\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** Stop hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: Stop
# AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"review\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** PreToolUse(AskUserQuestion) hook bash 스크립트를 생성한다 */
export function generateQuestionHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: PreToolUse (AskUserQuestion)
# Claude가 사용자에게 질문할 때 현재 task를 PENDING으로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"pending\\\"}" \\
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
async function readSettingsJson(settingsPath: string, sshHost?: string | null): Promise<ClaudeSettings> {
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
    const typed = entry as HookEntry;
    return typed.hooks?.some((h) => h.command?.includes(scriptName));
  });
}

/**
 * 지정된 repo에 Claude Code hooks를 설정한다.
 * 기존 settings.json이 있으면 kanvibe hooks만 추가하고 나머지는 보존한다.
 */
export async function setupClaudeHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  authToken?: string,
): Promise<void> {
  const claudeDir = path.join(repoPath, ".claude");
  const hooksDir = path.join(claudeDir, "hooks");
  const settingsPath = path.join(claudeDir, "settings.json");

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = path.join(hooksDir, "kanvibe-stop-hook.sh");
  const questionScriptPath = path.join(hooksDir, "kanvibe-question-hook.sh");

  await writeHookTaskIdFile(repoPath, taskId);
  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await writeFile(questionScriptPath, generateQuestionHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await chmod(promptScriptPath, 0o755);
  await chmod(stopScriptPath, 0o755);
  await chmod(questionScriptPath, 0o755);

  const settings = await readSettingsJson(settingsPath);
  if (!settings.hooks) {
    settings.hooks = {};
  }

  const hooks = settings.hooks as Record<string, unknown[]>;

  if (!hooks.UserPromptSubmit) {
    hooks.UserPromptSubmit = [];
  }
  if (!hasKanvibeHook(hooks.UserPromptSubmit, "kanvibe-prompt-hook.sh")) {
    (hooks.UserPromptSubmit as HookEntry[]).push({
      hooks: [
        {
          type: "command",
          command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh',
          timeout: 10,
        },
      ],
    });
  }

  if (!hooks.PreToolUse) {
    hooks.PreToolUse = [];
  }
  if (!hasKanvibeHook(hooks.PreToolUse, "kanvibe-question-hook.sh")) {
    (hooks.PreToolUse as MatcherHookEntry[]).push({
      matcher: "AskUserQuestion",
      hooks: [
        {
          type: "command",
          command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh',
          timeout: 10,
        },
      ],
    });
  }

  if (!hooks.PostToolUse) {
    hooks.PostToolUse = [];
  }
  if (!hasKanvibeHook(hooks.PostToolUse, "kanvibe-prompt-hook.sh")) {
    (hooks.PostToolUse as MatcherHookEntry[]).push({
      matcher: "AskUserQuestion",
      hooks: [
        {
          type: "command",
          command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh',
          timeout: 10,
        },
      ],
    });
  }

  if (!hooks.Stop) {
    hooks.Stop = [];
  }
  if (!hasKanvibeHook(hooks.Stop, "kanvibe-stop-hook.sh")) {
    (hooks.Stop as HookEntry[]).push({
      hooks: [
        {
          type: "command",
          command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh',
          timeout: 10,
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

export interface ClaudeHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasQuestionHook: boolean;
  hasSettingsEntry: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusMappings?: boolean;
  boundTaskId?: string | null;
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

  const promptScriptExists = await pathExists(promptScriptPath, sshHost);
  const stopScriptExists = await pathExists(stopScriptPath, sshHost);
  const questionScriptExists = await pathExists(questionScriptPath, sshHost);

  const [promptContent, stopContent, questionContent] = await Promise.all([
    promptScriptExists ? readTextFile(promptScriptPath, sshHost) : Promise.resolve(""),
    stopScriptExists ? readTextFile(stopScriptPath, sshHost) : Promise.resolve(""),
    questionScriptExists ? readTextFile(questionScriptPath, sshHost) : Promise.resolve(""),
  ]);

  const scriptContents = [promptContent, stopContent, questionContent];
  const boundTaskId = await readHookTaskIdFile(repoPath, sshHost);
  const hasTaskIdBinding = scriptContents.every((content) => hasTaskIdPayloadBinding(content, taskId, boundTaskId));
  const hasStatusMappings =
    promptContent.includes('\\\"status\\\": \\\"progress\\\"') &&
    stopContent.includes('\\\"status\\\": \\\"review\\\"') &&
    questionContent.includes('\\\"status\\\": \\\"pending\\\"');

  let hasSettingsEntry = false;
  try {
    const settings = await readSettingsJson(settingsPath, sshHost);
    const hooks = settings.hooks as Record<string, unknown[]> | undefined;
    if (hooks) {
      const hasPrompt = hasKanvibeHook(hooks.UserPromptSubmit || [], "kanvibe-prompt-hook.sh");
      const hasStop = hasKanvibeHook(hooks.Stop || [], "kanvibe-stop-hook.sh");
      const hasQuestion = hasKanvibeHook(hooks.PreToolUse || [], "kanvibe-question-hook.sh");
      const hasAnswerResume = hasKanvibeHook(hooks.PostToolUse || [], "kanvibe-prompt-hook.sh");
      hasSettingsEntry = hasPrompt && hasStop && hasQuestion && hasAnswerResume;
    }
  } catch {
    /* settings.json 없음 */
  }

  const installed = promptScriptExists && stopScriptExists && questionScriptExists && hasSettingsEntry && hasTaskIdBinding && hasStatusMappings;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasStopHook: stopScriptExists,
    hasQuestionHook: questionScriptExists,
    hasSettingsEntry,
    hasTaskIdBinding,
    hasStatusMappings,
    boundTaskId,
  };
}
