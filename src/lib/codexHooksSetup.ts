import { writeFile, mkdir, chmod } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { buildCurlAuthHeader } from "@/lib/hookAuth";
import { pathExists, readTextFile } from "@/lib/hostFileAccess";
import { extractShellHookServerUrl, validateHookServerConfiguration } from "@/lib/hookServerStatus";
import { buildShellTaskIdResolver, extractShellTaskId } from "@/lib/hookTaskBinding";

/**
 * Codex CLI는 현재 notify 설정의 agent-turn-complete 이벤트만 지원한다.
 * config.toml에 notify 명령을 등록하고, hook 스크립트를 생성한다.
 */

/** notify hook bash 스크립트를 생성한다 (agent-turn-complete → REVIEW) */
export function generateNotifyHookScript(kanvibeUrl: string, taskId: string, authToken?: string): string {
  return `#!/bin/bash

# KanVibe Codex CLI Hook: notify (agent-turn-complete)
# Codex 응답이 완료되면 현재 task를 REVIEW로 변경한다.
# Codex notify 스크립트는 첫 번째 인자로 JSON payload를 받는다.

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}

JSON_PAYLOAD="$1"

# agent-turn-complete 이벤트만 처리
EVENT_TYPE=$(echo "$JSON_PAYLOAD" | grep -o '"type":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ "$EVENT_TYPE" != "agent-turn-complete" ]; then
  exit 0
fi

curl -s -X POST "\${KANVIBE_URL}/api/hooks/status" \\
  -H "Content-Type: application/json" \\
${buildCurlAuthHeader(authToken)}  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"review\\\"}" \\
  > /dev/null 2>&1

exit 0
`;
}

export const HOOK_SCRIPT_NAME = "kanvibe-notify-hook.sh";
export const CONFIG_FILE_NAME = "config.toml";

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

/** 기존 config.toml 내용을 읽거나 빈 문자열을 반환한다 */
async function readConfigToml(configPath: string, sshHost?: string | null): Promise<string> {
  return readTextFile(configPath, sshHost);
}

/** config.toml에 kanvibe notify hook이 등록되어 있는지 확인한다 */
function hasKanvibeNotify(configContent: string): boolean {
  return /^notify\s*=\s*\["\.codex\/hooks\/kanvibe-notify-hook\.sh"\]$/m.test(configContent);
}

/**
 * 지정된 repo에 Codex CLI hooks를 설정한다.
 * config.toml의 notify 설정에 hook 스크립트를 등록한다.
 */
export async function setupCodexHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  authToken?: string,
): Promise<void> {
  const codexDir = path.join(repoPath, ".codex");
  const hooksDir = path.join(codexDir, "hooks");
  const configPath = path.join(codexDir, CONFIG_FILE_NAME);

  await mkdir(hooksDir, { recursive: true });

  const notifyScriptPath = path.join(hooksDir, HOOK_SCRIPT_NAME);
  await writeFile(notifyScriptPath, generateNotifyHookScript(kanvibeUrl, taskId, authToken), "utf-8");
  await chmod(notifyScriptPath, 0o755);

  const configContent = await readConfigToml(configPath);

  if (!hasKanvibeNotify(configContent)) {
    const notifyLine = `notify = [".codex/hooks/${HOOK_SCRIPT_NAME}"]\n`;

    if (configContent.trim().length === 0) {
      await writeFile(configPath, notifyLine, "utf-8");
    } else {
      if (/^notify\s*=/m.test(configContent)) {
        const updated = configContent.replace(/^notify\s*=.*$/m, `notify = [".codex/hooks/${HOOK_SCRIPT_NAME}"]`);
        await writeFile(configPath, updated, "utf-8");
      } else {
        await writeFile(configPath, configContent.trimEnd() + "\n" + notifyLine, "utf-8");
      }
    }
  }

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface CodexHooksStatus {
  installed: boolean;
  hasNotifyHook: boolean;
  hasConfigEntry: boolean;
  hasTaskIdBinding?: boolean;
  hasReviewStatus?: boolean;
  hasAgentTurnCompleteFilter?: boolean;
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
  const notifyScriptPath = pathModule.join(hooksDir, HOOK_SCRIPT_NAME);

  const notifyScriptExists = await pathExists(notifyScriptPath, sshHost);

  const notifyContent = notifyScriptExists
    ? await readTextFile(notifyScriptPath, sshHost)
    : "";
  const boundTaskId = extractShellTaskId(notifyContent);
  const hasTaskIdBinding = hasTaskIdPayloadBinding(notifyContent, taskId);
  const hasReviewStatus = notifyContent.includes('\\\"status\\\": \\\"review\\\"');
  const hasAgentTurnCompleteFilter = notifyContent.includes("EVENT_TYPE") && notifyContent.includes("agent-turn-complete");
  const hookServerValidation = await validateHookServerConfiguration(
    [extractShellHookServerUrl(notifyContent)],
    Boolean(taskId),
    sshHost,
  );

  let hasConfigEntry = false;
  try {
    const configContent = await readConfigToml(configPath, sshHost);
    hasConfigEntry = hasKanvibeNotify(configContent);
  } catch {
    /* config.toml 없음 */
  }

  const installed = notifyScriptExists
    && hasConfigEntry
    && hasTaskIdBinding
    && hasReviewStatus
    && hasAgentTurnCompleteFilter
    && hookServerValidation.hasExpectedHookServerUrl
    && hookServerValidation.hasReachableHookServer;

  return {
    installed,
    hasNotifyHook: notifyScriptExists,
    hasConfigEntry,
    hasTaskIdBinding,
    hasReviewStatus,
    hasAgentTurnCompleteFilter,
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
  };
}
