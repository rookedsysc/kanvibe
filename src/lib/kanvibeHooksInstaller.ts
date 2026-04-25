import path from "node:path";
import { execGit } from "@/lib/gitOperations";
import { setupClaudeHooks, getClaudeHooksStatus, generatePromptHookScript as generateClaudePromptHookScript, generateStopHookScript as generateClaudeStopHookScript, generateQuestionHookScript as generateClaudeQuestionHookScript, type ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import { setupGeminiHooks, getGeminiHooksStatus, generatePromptHookScript as generateGeminiPromptHookScript, generateStopHookScript as generateGeminiStopHookScript, type GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import {
  setupCodexHooks,
  getCodexHooksStatus,
  generatePromptHookScript as generateCodexPromptHookScript,
  generatePermissionHookScript as generateCodexPermissionHookScript,
  generatePreToolHookScript as generateCodexPreToolHookScript,
  generateStopHookScript as generateCodexStopHookScript,
  upsertCodexConfigToml,
  upsertCodexHooksJson,
  PROMPT_HOOK_SCRIPT_NAME,
  PERMISSION_HOOK_SCRIPT_NAME,
  PRE_TOOL_HOOK_SCRIPT_NAME,
  STOP_HOOK_SCRIPT_NAME,
  HOOKS_FILE_NAME,
  CONFIG_FILE_NAME,
  type CodexHooksStatus,
} from "@/lib/codexHooksSetup";
import { setupOpenCodeHooks, getOpenCodeHooksStatus, generatePluginScript, PLUGIN_DIR_NAME, PLUGIN_FILE_NAME, type OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";
import { getHookServerToken, getHookServerUrl } from "@/lib/hookEndpoint";
import { addAiToolPatternsToGitExcludeRemote } from "@/lib/gitExclude";

export async function installKanvibeHooks(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);
  const hookServerToken = getHookServerToken();

  if (sshHost) {
    await addAiToolPatternsToGitExcludeRemote(targetPath, sshHost);
  }

  const installers = [
    {
      provider: "Claude",
      install: () => sshHost
        ? setupRemoteClaudeHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost)
        : setupClaudeHooks(targetPath, taskId, hookServerUrl, hookServerToken),
    },
    {
      provider: "Gemini",
      install: () => sshHost
        ? setupRemoteGeminiHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost)
        : setupGeminiHooks(targetPath, taskId, hookServerUrl, hookServerToken),
    },
    {
      provider: "Codex",
      install: () => sshHost
        ? setupRemoteCodexHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost)
        : setupCodexHooks(targetPath, taskId, hookServerUrl, hookServerToken),
    },
    {
      provider: "OpenCode",
      install: () => sshHost
        ? setupRemoteOpenCodeHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost)
        : setupOpenCodeHooks(targetPath, taskId, hookServerUrl, hookServerToken),
    },
  ];

  if (!sshHost) {
    const results = await Promise.allSettled(installers.map(({ install }) => install()));
    assertHookInstallResults(results, installers.map(({ provider }) => provider));
  } else {
    for (const { provider, install } of installers) {
      try {
        await install();
      } catch (error) {
        console.error(`[hooks] ${provider} install failed`, {
          targetPath,
          taskId,
          sshHost,
          error: error instanceof Error ? error.message : String(error),
        });
        throw error;
      }
    }
  }

  await logHookVerificationStatuses(targetPath, taskId, sshHost);
}

async function setupRemoteClaudeHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const claudeDir = path.posix.join(repoPath, ".claude");
  const hooksDir = path.posix.join(claudeDir, "hooks");
  const settingsPath = path.posix.join(claudeDir, "settings.json");
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"), generateClaudePromptHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-stop-hook.sh"), generateClaudeStopHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-question-hook.sh"), generateClaudeQuestionHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const settings = await readRemoteJsonFile(settingsPath, sshHost);
  const hooks = ((settings.hooks as Record<string, unknown[]>) || {});
  settings.hooks = hooks;

  upsertHookEntry(hooks, "UserPromptSubmit", "kanvibe-prompt-hook.sh", {
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "PreToolUse", "kanvibe-question-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "PostToolUse", "kanvibe-prompt-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "Stop", "kanvibe-stop-hook.sh", {
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh', timeout: 10 }],
  });

  await writeRemoteTextFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", sshHost);
}

async function setupRemoteGeminiHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const geminiDir = path.posix.join(repoPath, ".gemini");
  const hooksDir = path.posix.join(geminiDir, "hooks");
  const settingsPath = path.posix.join(geminiDir, "settings.json");
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"), generateGeminiPromptHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-stop-hook.sh"), generateGeminiStopHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const settings = await readRemoteJsonFile(settingsPath, sshHost);
  const hooks = ((settings.hooks as Record<string, unknown[]>) || {});
  settings.hooks = hooks;

  upsertHookEntry(hooks, "BeforeAgent", "kanvibe-prompt-hook.sh", {
    matcher: "*",
    hooks: [{ type: "command", command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh', timeout: 10000 }],
  });
  upsertHookEntry(hooks, "AfterAgent", "kanvibe-stop-hook.sh", {
    matcher: "*",
    hooks: [{ type: "command", command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh', timeout: 10000 }],
  });

  await writeRemoteTextFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", sshHost);
}

async function setupRemoteCodexHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const codexDir = path.posix.join(repoPath, ".codex");
  const hooksDir = path.posix.join(codexDir, "hooks");
  const configPath = path.posix.join(codexDir, CONFIG_FILE_NAME);
  const hooksPath = path.posix.join(codexDir, HOOKS_FILE_NAME);
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME), generateCodexPromptHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, PERMISSION_HOOK_SCRIPT_NAME), generateCodexPermissionHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, PRE_TOOL_HOOK_SCRIPT_NAME), generateCodexPreToolHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, STOP_HOOK_SCRIPT_NAME), generateCodexStopHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const configContent = await readRemoteTextFile(configPath, sshHost);
  await writeRemoteTextFile(configPath, upsertCodexConfigToml(configContent), sshHost);

  const hooksContent = await readRemoteTextFile(hooksPath, sshHost);
  await writeRemoteTextFile(hooksPath, upsertCodexHooksJson(hooksContent), sshHost);
}

async function setupRemoteOpenCodeHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const pluginDir = path.posix.join(repoPath, ".opencode", PLUGIN_DIR_NAME);
  await execGit(`mkdir -p "${pluginDir}"`, sshHost);
  await writeRemoteTextFile(
    path.posix.join(pluginDir, PLUGIN_FILE_NAME),
    generatePluginScript(hookServerUrl, taskId, hookServerToken),
    sshHost,
  );
}

async function readRemoteJsonFile(filePath: string, sshHost: string): Promise<Record<string, unknown>> {
  const content = await readRemoteTextFile(filePath, sshHost);
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content) as Record<string, unknown>;
  } catch {
    return {};
  }
}

async function readRemoteTextFile(filePath: string, sshHost: string): Promise<string> {
  try {
    return await execGit(`test -f "${filePath}" && cat "${filePath}" || true`, sshHost);
  } catch {
    return "";
  }
}

async function writeRemoteTextFile(filePath: string, content: string, sshHost: string, mode?: number): Promise<void> {
  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  const chmodCommand = mode ? ` && chmod ${mode.toString(8)} "${filePath}"` : "";
  await execGit(
    `mkdir -p "${path.posix.dirname(filePath)}" && printf '%s' '${encodedContent}' | (base64 -d 2>/dev/null || base64 -D) > "${filePath}"${chmodCommand}`,
    sshHost,
  );
}

function upsertHookEntry(
  hooks: Record<string, unknown[]>,
  bucket: string,
  scriptName: string,
  entry: Record<string, unknown>,
) {
  const currentEntries = Array.isArray(hooks[bucket]) ? hooks[bucket] : [];
  hooks[bucket] = currentEntries.filter((value) => !JSON.stringify(value).includes(scriptName));
  hooks[bucket].push(entry);
}

function assertHookInstallResults(results: PromiseSettledResult<unknown>[], providers: string[]) {
  const failureIndex = results.findIndex((result) => result.status === "rejected");
  if (failureIndex === -1) {
    return;
  }

  const failure = results[failureIndex] as PromiseRejectedResult;
  console.error(`[hooks] ${providers[failureIndex]} install failed`, {
    provider: providers[failureIndex],
    error: failure.reason instanceof Error ? failure.reason.message : String(failure.reason),
  });
  throw failure.reason instanceof Error ? failure.reason : new Error("hooks 설정 실패");
}

type HookVerificationStatus = ClaudeHooksStatus | GeminiHooksStatus | CodexHooksStatus | OpenCodeHooksStatus;

async function logHookVerificationStatuses(targetPath: string, taskId: string, sshHost?: string | null) {
  const verifiers = [
    { provider: "Claude", verify: getClaudeHooksStatus },
    { provider: "Gemini", verify: getGeminiHooksStatus },
    { provider: "Codex", verify: getCodexHooksStatus },
    { provider: "OpenCode", verify: getOpenCodeHooksStatus },
  ] as const;

  const results = await Promise.allSettled(verifiers.map(({ verify }) => verify(targetPath, taskId, sshHost)));
  for (const [index, result] of results.entries()) {
    const provider = verifiers[index].provider;
    if (result.status === "fulfilled") {
      logHookVerificationStatus(provider, result.value, targetPath, taskId, sshHost);
      continue;
    }

    console.warn(`[hooks] ${provider} verification unavailable`, {
      provider,
      targetPath,
      taskId,
      sshHost: sshHost ?? null,
      error: result.reason instanceof Error ? result.reason.message : String(result.reason),
    });
  }
}

function logHookVerificationStatus(
  provider: string,
  status: HookVerificationStatus,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
) {
  const failedChecks = Object.entries(status)
    .filter(([key, value]) => key.startsWith("has") && value === false)
    .map(([key]) => key);

  const payload = {
    provider,
    targetPath,
    taskId,
    sshHost: sshHost ?? null,
    installed: status.installed,
    failedChecks,
    boundTaskId: status.boundTaskId ?? null,
    configuredHookServerUrl: status.configuredHookServerUrl ?? null,
    expectedHookServerUrl: status.expectedHookServerUrl ?? null,
    registeredPluginUrls: "registeredPluginUrls" in status && Array.isArray(status.registeredPluginUrls)
      ? status.registeredPluginUrls
      : undefined,
  };

  if (status.installed) {
    console.log(`[hooks] ${provider} verification`, payload);
    return;
  }

  console.warn(`[hooks] ${provider} verification`, payload);
}
