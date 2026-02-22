import { readFile, writeFile, mkdir, chmod, access } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";

/**
 * Codex CLI는 현재 notify 설정의 agent-turn-complete 이벤트만 지원한다.
 * config.toml에 notify 명령을 등록하고, hook 스크립트를 생성한다.
 */

/** notify hook bash 스크립트를 생성한다 (agent-turn-complete → REVIEW) */
function generateNotifyHookScript(kanvibeUrl: string, projectName: string): string {
  return `#!/bin/bash

# KanVibe Codex CLI Hook: notify (agent-turn-complete)
# Codex 응답이 완료되면 현재 브랜치의 작업을 REVIEW로 변경한다.
# Codex notify 스크립트는 첫 번째 인자로 JSON payload를 받는다.

KANVIBE_URL="${kanvibeUrl}"
PROJECT_NAME="${projectName}"

JSON_PAYLOAD="$1"

# agent-turn-complete 이벤트만 처리
EVENT_TYPE=$(echo "$JSON_PAYLOAD" | grep -o '"type":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ "$EVENT_TYPE" != "agent-turn-complete" ]; then
  exit 0
fi

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

const HOOK_SCRIPT_NAME = "kanvibe-notify-hook.sh";
const CONFIG_FILE_NAME = "config.toml";

/** 기존 config.toml 내용을 읽거나 빈 문자열을 반환한다 */
async function readConfigToml(configPath: string): Promise<string> {
  try {
    return await readFile(configPath, "utf-8");
  } catch {
    return "";
  }
}

/** config.toml에 kanvibe notify hook이 등록되어 있는지 확인한다 */
function hasKanvibeNotify(configContent: string): boolean {
  return configContent.includes(HOOK_SCRIPT_NAME);
}

/**
 * 지정된 repo에 Codex CLI hooks를 설정한다.
 * config.toml의 notify 설정에 hook 스크립트를 등록한다.
 */
export async function setupCodexHooks(
  repoPath: string,
  projectName: string,
  kanvibeUrl: string
): Promise<void> {
  const codexDir = path.join(repoPath, ".codex");
  const hooksDir = path.join(codexDir, "hooks");
  const configPath = path.join(codexDir, CONFIG_FILE_NAME);

  await mkdir(hooksDir, { recursive: true });

  const notifyScriptPath = path.join(hooksDir, HOOK_SCRIPT_NAME);
  await writeFile(notifyScriptPath, generateNotifyHookScript(kanvibeUrl, projectName), "utf-8");
  await chmod(notifyScriptPath, 0o755);

  const configContent = await readConfigToml(configPath);

  if (!hasKanvibeNotify(configContent)) {
    const notifyLine = `notify = [".codex/hooks/${HOOK_SCRIPT_NAME}"]\n`;

    if (configContent.trim().length === 0) {
      await writeFile(configPath, notifyLine, "utf-8");
    } else {
      /** 기존 notify 설정이 있으면 교체, 없으면 추가한다 */
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
}

/** 지정된 repo의 Codex CLI hooks 설치 상태를 확인한다 */
export async function getCodexHooksStatus(repoPath: string): Promise<CodexHooksStatus> {
  const codexDir = path.join(repoPath, ".codex");
  const hooksDir = path.join(codexDir, "hooks");
  const configPath = path.join(codexDir, CONFIG_FILE_NAME);

  const notifyScriptExists = await access(path.join(hooksDir, HOOK_SCRIPT_NAME))
    .then(() => true)
    .catch(() => false);

  let hasConfigEntry = false;
  try {
    const configContent = await readConfigToml(configPath);
    hasConfigEntry = hasKanvibeNotify(configContent);
  } catch {
    /* config.toml 없음 */
  }

  const installed = notifyScriptExists && hasConfigEntry;

  return {
    installed,
    hasNotifyHook: notifyScriptExists,
    hasConfigEntry,
  };
}
