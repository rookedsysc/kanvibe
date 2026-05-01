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
import { getHookServerUrl } from "@/lib/hookEndpoint";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { quoteShellArgument } from "@/lib/hostFileAccess";

export async function installKanvibeHooks(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);

  const installers = [
    {
      provider: "Claude",
      install: () => sshHost
        ? setupRemoteClaudeHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupClaudeHooks(targetPath, taskId, hookServerUrl),
    },
    {
      provider: "Gemini",
      install: () => sshHost
        ? setupRemoteGeminiHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupGeminiHooks(targetPath, taskId, hookServerUrl),
    },
    {
      provider: "Codex",
      install: () => sshHost
        ? setupRemoteCodexHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupCodexHooks(targetPath, taskId, hookServerUrl),
    },
    {
      provider: "OpenCode",
      install: () => sshHost
        ? setupRemoteOpenCodeHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupOpenCodeHooks(targetPath, taskId, hookServerUrl),
    },
  ];

  if (!sshHost) {
    const results = await Promise.allSettled(installers.map(({ install }) => install()));
    assertHookInstallResults(results, installers.map(({ provider }) => provider));
  } else {
    try {
      await addAiToolPatternsToGitExclude(targetPath, sshHost);
    } catch (error) {
      console.warn("[hooks] remote git exclude update failed", {
        targetPath,
        sshHost,
        error: error instanceof Error ? error.message : String(error),
      });
    }

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

  if (sshHost) {
    void logHookVerificationStatuses(targetPath, taskId, sshHost).catch((error) => {
      console.warn("[hooks] remote verification failed", {
        targetPath,
        taskId,
        sshHost,
        error: error instanceof Error ? error.message : String(error),
      });
    });
    return;
  }

  await logHookVerificationStatuses(targetPath, taskId, sshHost);
}

async function setupRemoteClaudeHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const claudeDir = path.posix.join(repoPath, ".claude");
  const hooksDir = path.posix.join(claudeDir, "hooks");
  const settingsPath = path.posix.join(claudeDir, "settings.json");

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

  await writeRemoteTextFiles([
    {
      filePath: path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"),
      content: generateClaudePromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-stop-hook.sh"),
      content: generateClaudeStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-question-hook.sh"),
      content: generateClaudeQuestionHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: settingsPath,
      content: JSON.stringify(settings, null, 2) + "\n",
    },
  ], sshHost);
}

async function setupRemoteGeminiHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const geminiDir = path.posix.join(repoPath, ".gemini");
  const hooksDir = path.posix.join(geminiDir, "hooks");
  const settingsPath = path.posix.join(geminiDir, "settings.json");

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

  await writeRemoteTextFiles([
    {
      filePath: path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"),
      content: generateGeminiPromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-stop-hook.sh"),
      content: generateGeminiStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: settingsPath,
      content: JSON.stringify(settings, null, 2) + "\n",
    },
  ], sshHost);
}

async function setupRemoteCodexHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const codexDir = path.posix.join(repoPath, ".codex");
  const hooksDir = path.posix.join(codexDir, "hooks");
  const configPath = path.posix.join(codexDir, CONFIG_FILE_NAME);
  const hooksPath = path.posix.join(codexDir, HOOKS_FILE_NAME);

  const configContent = await readRemoteTextFile(configPath, sshHost);
  const hooksContent = await readRemoteTextFile(hooksPath, sshHost);

  await writeRemoteTextFiles([
    {
      filePath: path.posix.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME),
      content: generateCodexPromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, PERMISSION_HOOK_SCRIPT_NAME),
      content: generateCodexPermissionHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, PRE_TOOL_HOOK_SCRIPT_NAME),
      content: generateCodexPreToolHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, STOP_HOOK_SCRIPT_NAME),
      content: generateCodexStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: configPath,
      content: upsertCodexConfigToml(configContent),
    },
    {
      filePath: hooksPath,
      content: upsertCodexHooksJson(hooksContent),
    },
  ], sshHost);
}

async function setupRemoteOpenCodeHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const pluginDir = path.posix.join(repoPath, ".opencode", PLUGIN_DIR_NAME);
  await writeRemoteTextFiles([
    {
      filePath: path.posix.join(pluginDir, PLUGIN_FILE_NAME),
      content: generatePluginScript(hookServerUrl, taskId),
    },
  ], sshHost);
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
    return await execGit(
      `test -f ${quoteShellArgument(filePath)} && cat ${quoteShellArgument(filePath)} || true`,
      sshHost,
    );
  } catch {
    return "";
  }
}

interface RemoteTextFile {
  filePath: string;
  content: string;
  mode?: number;
}

async function writeRemoteTextFiles(files: RemoteTextFile[], sshHost: string): Promise<void> {
  const command = files.map(({ filePath, content, mode }) => {
    const encodedContent = Buffer.from(content, "utf-8").toString("base64");
    const parts = [
      `mkdir -p ${quoteShellArgument(path.posix.dirname(filePath))}`,
      `printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quoteShellArgument(filePath)}`,
    ];

    if (mode) {
      parts.push(`chmod ${mode.toString(8)} ${quoteShellArgument(filePath)}`);
    }

    return parts.join(" && ");
  }).join(" && ");

  await execGit(command, sshHost);
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
    .filter(([key]) => !(status.installed && key === "hasReachableHookServer"))
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
