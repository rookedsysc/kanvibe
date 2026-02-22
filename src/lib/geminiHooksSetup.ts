import { readFile, writeFile, mkdir, chmod, access } from "fs/promises";
import path from "path";

/**
 * Gemini CLI hooks는 stdout에 반드시 JSON만 출력해야 한다.
 * curl 결과는 /dev/null로 보내고, 마지막에 '{}' JSON을 출력한다.
 */

/** BeforeAgent hook bash 스크립트를 생성한다 */
function generatePromptHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Gemini CLI Hook: BeforeAgent
# 사용자가 prompt를 입력하면 현재 브랜치의 작업을 PROGRESS로 변경한다.
# Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  echo '{}'
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"branchName\\": \\"\${BRANCH_NAME}\\", \\"projectName\\": \\"\${PROJECT_NAME}\\", \\"status\\": \\"progress\\"}" \\
  > /dev/null 2>&1

echo '{}'
exit 0
`;
}

/** AfterAgent hook bash 스크립트를 생성한다 */
function generateStopHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Gemini CLI Hook: AfterAgent
# AI 응답이 완료되면 현재 브랜치의 작업을 REVIEW로 변경한다.
# Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  echo '{}'
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
  -d "{\\"branchName\\": \\"\${BRANCH_NAME}\\", \\"projectName\\": \\"\${PROJECT_NAME}\\", \\"status\\": \\"review\\"}" \\
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

/** 기존 settings.json을 읽거나 빈 객체를 반환한다 */
async function readSettingsJson(settingsPath: string): Promise<GeminiSettings> {
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
  projectName: string,
  kanvibeUrl: string
): Promise<void> {
  const geminiDir = path.join(repoPath, ".gemini");
  const hooksDir = path.join(geminiDir, "hooks");
  const settingsPath = path.join(geminiDir, "settings.json");

  await mkdir(hooksDir, { recursive: true });

  const promptScriptPath = path.join(hooksDir, "kanvibe-prompt-hook.sh");
  const stopScriptPath = path.join(hooksDir, "kanvibe-stop-hook.sh");

  await writeFile(promptScriptPath, generatePromptHookScript(kanvibeUrl, projectName), "utf-8");
  await writeFile(stopScriptPath, generateStopHookScript(kanvibeUrl, projectName), "utf-8");
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
          name: "kanvibe-prompt",
          type: "command",
          command: "$GEMINI_PROJECT_DIR/.gemini/hooks/kanvibe-prompt-hook.sh",
          timeout: 5000,
          description: "KanVibe: 작업 시작 시 status를 progress로 변경",
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
          name: "kanvibe-stop",
          type: "command",
          command: "$GEMINI_PROJECT_DIR/.gemini/hooks/kanvibe-stop-hook.sh",
          timeout: 5000,
          description: "KanVibe: 작업 완료 시 status를 review로 변경",
        },
      ],
    });
  }

  await writeFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", "utf-8");
}

export interface GeminiHooksStatus {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasSettingsEntry: boolean;
}

/** 지정된 repo의 Gemini CLI hooks 설치 상태를 확인한다 */
export async function getGeminiHooksStatus(repoPath: string): Promise<GeminiHooksStatus> {
  const geminiDir = path.join(repoPath, ".gemini");
  const hooksDir = path.join(geminiDir, "hooks");
  const settingsPath = path.join(geminiDir, "settings.json");

  const promptScriptExists = await access(path.join(hooksDir, "kanvibe-prompt-hook.sh"))
    .then(() => true)
    .catch(() => false);
  const stopScriptExists = await access(path.join(hooksDir, "kanvibe-stop-hook.sh"))
    .then(() => true)
    .catch(() => false);

  let hasSettingsEntry = false;
  try {
    const settings = await readSettingsJson(settingsPath);
    const hooks = settings.hooks as Record<string, unknown[]> | undefined;
    if (hooks) {
      const hasPrompt = hasKanvibeHook(hooks.BeforeAgent || [], "kanvibe-prompt-hook.sh");
      const hasStop = hasKanvibeHook(hooks.AfterAgent || [], "kanvibe-stop-hook.sh");
      hasSettingsEntry = hasPrompt && hasStop;
    }
  } catch {
    /* settings.json 없음 */
  }

  const installed = promptScriptExists && stopScriptExists && hasSettingsEntry;

  return {
    installed,
    hasPromptHook: promptScriptExists,
    hasStopHook: stopScriptExists,
    hasSettingsEntry,
  };
}
