import { readFile, writeFile, mkdir, chmod, access } from "fs/promises";
import path from "path";

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
function generatePromptHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: UserPromptSubmit
# 사용자가 prompt를 입력하면 현재 브랜치의 작업을 PROGRESS로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"branchName\\": \\"\${BRANCH_NAME}\\", \\"projectName\\": \\"\${PROJECT_NAME}\\", \\"status\\": \\"progress\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** Stop hook bash 스크립트를 생성한다 */
function generateStopHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: Stop
# AI 응답이 완료되면 현재 브랜치의 작업을 REVIEW로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"branchName\\": \\"\${BRANCH_NAME}\\", \\"projectName\\": \\"\${PROJECT_NAME}\\", \\"status\\": \\"review\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

/** PreToolUse(AskUserQuestion) hook bash 스크립트를 생성한다 */
function generateQuestionHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Claude Code Hook: PreToolUse (AskUserQuestion)
# Claude가 사용자에게 질문할 때 현재 브랜치의 작업을 PENDING으로 변경한다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"branchName\\": \\"\${BRANCH_NAME}\\", \\"projectName\\": \\"\${PROJECT_NAME}\\", \\"status\\": \\"pending\\"}" \\
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

/** 기존 settings.json을 읽거나 빈 객체를 반환한다 */
async function readSettingsJson(settingsPath: string): Promise<ClaudeSettings> {
  try {
    const content = await readFile(settingsPath, "utf-8");
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
  projectName: string,
  kanvibeUrl: string
): Promise<void> {
  const claudeDir = path.join(repoPath, ".claude");
  const hooksDir = path.join(claudeDir, "hooks");
  const settingsPath = path.join(claudeDir, "settings.json");

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = path.join(hooksDir, "kanvibe-stop-hook.sh");
  const questionScriptPath = path.join(hooksDir, "kanvibe-question-hook.sh");

  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, projectName), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, projectName), "utf-8");
  await writeFile(questionScriptPath, generateQuestionHookScript(kanvibeUrl, projectName), "utf-8");
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
          command: ".claude/hooks/kanvibe-prompt-hook.sh",
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
          command: ".claude/hooks/kanvibe-question-hook.sh",
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
          command: ".claude/hooks/kanvibe-prompt-hook.sh",
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
          command: ".claude/hooks/kanvibe-stop-hook.sh",
          timeout: 10,
        },
      ],
    });
  }

  await writeFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", "utf-8");
}

export interface ClaudeHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasQuestionHook: boolean;
  hasSettingsEntry: boolean;
}

/** 지정된 repo의 Claude Code hooks 설치 상태를 확인한다 */
export async function getClaudeHooksStatus(repoPath: string): Promise<ClaudeHooksStatus> {
  const claudeDir = path.join(repoPath, ".claude");
  const hooksDir = path.join(claudeDir, "hooks");
  const settingsPath = path.join(claudeDir, "settings.json");

  const promptScriptExists = await access(path.join(hooksDir, "kanvibe-prompt-hook.sh"))
    .then(() => true)
    .catch(() => false);
  const stopScriptExists = await access(path.join(hooksDir, "kanvibe-stop-hook.sh"))
    .then(() => true)
    .catch(() => false);
  const questionScriptExists = await access(path.join(hooksDir, "kanvibe-question-hook.sh"))
    .then(() => true)
    .catch(() => false);

  let hasSettingsEntry = false;
  try {
    const settings = await readSettingsJson(settingsPath);
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

  const installed = promptScriptExists && stopScriptExists && questionScriptExists && hasSettingsEntry;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasStopHook: stopScriptExists,
    hasQuestionHook: questionScriptExists,
    hasSettingsEntry,
  };
}
